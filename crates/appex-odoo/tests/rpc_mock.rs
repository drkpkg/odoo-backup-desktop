mod common;

use appex_odoo::{
    CheckStatus, Credentials, OdooError, ProbeInput, ProbeWarning, ProtocolPreference, RpcProtocol, SecretKind,
    TransportKind, connect, detect_version, probe,
};
use serde_json::{Map, json};
use url::Url;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn xml_ok(value_xml: &str) -> ResponseTemplate {
    let body = format!(
        "<?xml version='1.0'?>\n<methodResponse>\n<params>\n<param>\n{value_xml}\n</param>\n</params>\n</methodResponse>\n"
    );
    ResponseTemplate::new(200).set_body_raw(body, "text/xml")
}

fn xml_fault(code: i64, message: &str) -> ResponseTemplate {
    let body = format!(
        "<?xml version='1.0'?><methodResponse><fault><value><struct>\
         <member><name>faultCode</name><value><int>{code}</int></value></member>\
         <member><name>faultString</name><value><string>{message}</string></value></member>\
         </struct></value></fault></methodResponse>"
    );
    ResponseTemplate::new(200).set_body_raw(body, "text/xml")
}

const VERSION_17: &str = "<value><struct><member><name>server_version</name><value><string>17.0-20240101</string></value></member>\
<member><name>server_version_info</name><value><array><data><value><int>17</int></value><value><int>0</int></value>\
<value><int>0</int></value><value><string>final</string></value><value><int>0</int></value><value><string></string></value>\
</data></array></value></member><member><name>protocol_version</name><value><int>1</int></value></member></struct></value>";

fn base(server: &MockServer) -> Url {
    Url::parse(&server.uri()).unwrap()
}

fn credentials(kind: SecretKind) -> Credentials {
    Credentials { login: "admin".into(), secret: "s3cret".into(), kind }
}

#[tokio::test]
async fn detects_version_with_web_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/web/version"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"version_info": [19, 0, 0, "final", 0, ""], "version": "19.0"})),
        )
        .mount(&server)
        .await;
    let version = detect_version(&common::client(), &base(&server)).await.unwrap();
    assert_eq!((version.major, version.minor, version.saas), (19, 0, false));
}

#[tokio::test]
async fn detects_version_with_xmlrpc_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/web/version"))
        .respond_with(ResponseTemplate::new(404).set_body_string("<html>404</html>"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .and(body_string_contains("<methodName>version</methodName>"))
        .respond_with(xml_ok(VERSION_17))
        .mount(&server)
        .await;
    let version = detect_version(&common::client(), &base(&server)).await.unwrap();
    assert_eq!(version.major, 17);
    assert_eq!(version.server_version, "17.0-20240101");
}

#[tokio::test]
async fn version_detection_reports_unreachable_servers() {
    let url = Url::parse("http://127.0.0.1:9").unwrap();
    let err = detect_version(&common::client(), &url).await.unwrap_err();
    assert!(matches!(err, OdooError::Connection(_)), "{err:?}");

    let server = MockServer::start().await;
    let err = detect_version(&common::client(), &base(&server)).await.unwrap_err();
    assert!(matches!(err, OdooError::VersionDetection(_)), "{err:?}");
}

#[tokio::test]
async fn xmlrpc_authenticates_once_and_calls_execute_kw() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .and(body_string_contains("<methodName>authenticate</methodName>"))
        .respond_with(xml_ok("<value><int>2</int></value>"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/object"))
        .and(body_string_contains("<string>res.partner</string>"))
        .and(body_string_contains("<name>fields</name>"))
        .respond_with(xml_ok("<value><array><data><value><struct><member><name>id</name><value><int>7</int></value></member><member><name>email</name><value><nil/></value></member></struct></value></data></array></value>"))
        .mount(&server)
        .await;

    let rpc =
        connect(common::client(), base(&server), "db1".into(), credentials(SecretKind::Password), RpcProtocol::XmlRpc);
    assert_eq!(rpc.authenticate().await.unwrap(), 2);
    let mut kwargs = Map::new();
    kwargs.insert("fields".into(), json!(["email"]));
    let result = rpc.call("res.partner", "read", &[7], kwargs).await.unwrap();
    assert_eq!(result, json!([{"id": 7, "email": null}]));
}

#[tokio::test]
async fn xmlrpc_maps_failed_login_and_missing_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .and(body_string_contains("<string>bad</string>"))
        .respond_with(xml_ok("<value><boolean>0</boolean></value>"))
        .mount(&server)
        .await;
    let bad = Credentials { login: "admin".into(), secret: "bad".into(), kind: SecretKind::Password };
    let rpc = connect(common::client(), base(&server), "db1".into(), bad, RpcProtocol::XmlRpc);
    assert!(matches!(rpc.authenticate().await, Err(OdooError::AuthenticationFailed)));

    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .respond_with(xml_ok("<value><int>2</int></value>"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/object"))
        .respond_with(xml_fault(2, "Object appex.backup.api doesn't exist"))
        .mount(&server)
        .await;
    let rpc =
        connect(common::client(), base(&server), "db1".into(), credentials(SecretKind::Password), RpcProtocol::XmlRpc);
    let err = rpc.call("appex.backup.api", "get_info", &[], Map::new()).await.unwrap_err();
    assert!(err.is_model_not_found(), "{err:?}");
}

#[tokio::test]
async fn json2_sends_bearer_and_database_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/json/2/res.users/context_get"))
        .and(header("authorization", "Bearer key-123"))
        .and(header("x-odoo-database", "db19"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"lang": "en_US", "tz": false, "uid": 2})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/json/2/res.partner/read"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = req.body_json().unwrap();
            assert_eq!(body, json!({"ids": [3], "fields": ["name"]}));
            ResponseTemplate::new(200).set_body_json(json!([{"id": 3, "name": "Azure"}]))
        })
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/json/2/appex.backup.api/get_info"))
        .respond_with(ResponseTemplate::new(404).set_body_json(
            json!({"name": "werkzeug.exceptions.NotFound", "message": "the model 'appex.backup.api' does not exist"}),
        ))
        .mount(&server)
        .await;

    let creds = Credentials { login: String::new(), secret: "key-123".into(), kind: SecretKind::ApiKey };
    let rpc = connect(common::client(), base(&server), "db19".into(), creds, RpcProtocol::Json2);
    assert_eq!(rpc.authenticate().await.unwrap(), 2);
    let mut kwargs = Map::new();
    kwargs.insert("fields".into(), json!(["name"]));
    assert_eq!(rpc.call("res.partner", "read", &[3], kwargs).await.unwrap(), json!([{"id": 3, "name": "Azure"}]));
    assert!(rpc.call("appex.backup.api", "get_info", &[], Map::new()).await.unwrap_err().is_model_not_found());

    let wrong = connect(
        common::client(),
        base(&server),
        "db19".into(),
        Credentials { login: String::new(), secret: "wrong".into(), kind: SecretKind::ApiKey },
        RpcProtocol::Json2,
    );
    Mock::given(method("POST"))
        .and(path("/json/2/res.users/context_get"))
        .and(header("authorization", "Bearer wrong"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"name": "werkzeug.exceptions.Unauthorized", "message": "Invalid apikey"})),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    assert!(matches!(wrong.authenticate().await, Err(OdooError::AuthenticationFailed)));

    let password =
        connect(common::client(), base(&server), "db19".into(), credentials(SecretKind::Password), RpcProtocol::Json2);
    assert!(matches!(password.authenticate().await, Err(OdooError::UnsupportedProtocol { .. })));
}

#[tokio::test]
async fn probe_reports_module_and_db_manager() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/web/version"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"version_info": [19, 0, 0, "final", 0, ""], "version": "19.0"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/json/2/res.users/context_get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"uid": 2})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/json/2/appex.backup.api/get_info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"api_version": 1, "module_version": "19.0.1.0.0", "database": "db19", "filestore_supported": true}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/web/database/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jsonrpc": "2.0", "id": 1, "result": ["db19"]})))
        .mount(&server)
        .await;

    let input = ProbeInput {
        base_url: base(&server),
        database: Some("db19".into()),
        credentials: Some(Credentials { login: String::new(), secret: "k".into(), kind: SecretKind::ApiKey }),
        master_password: Some("master".into()),
        protocol_preference: ProtocolPreference::Auto,
    };
    let report = probe(&common::client(), input).await.unwrap();
    assert_eq!(report.version.major, 19);
    assert!(report.supported);
    assert_eq!(report.protocol, Some(RpcProtocol::Json2));
    assert_eq!(report.uid, Some(2));
    assert_eq!(report.auth, CheckStatus::Ok);
    assert_eq!(report.module, CheckStatus::Ok);
    assert_eq!(report.module_api_version, Some(1));
    assert_eq!(report.db_manager, CheckStatus::Ok);
    assert_eq!(report.recommended_transport, Some(TransportKind::AppexModule));
    assert!(report.warnings.contains(&ProbeWarning::InsecureHttp));
    assert!(report.warnings.contains(&ProbeWarning::MasterPasswordOverWire));
    assert!(!report.warnings.contains(&ProbeWarning::DatabaseDerivedFromHost));
}

#[tokio::test]
async fn probe_without_module_and_with_list_db_disabled() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/web/version")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .and(body_string_contains("<methodName>version</methodName>"))
        .respond_with(xml_ok(VERSION_17))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/common"))
        .and(body_string_contains("<methodName>authenticate</methodName>"))
        .respond_with(xml_ok("<value><int>2</int></value>"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/xmlrpc/2/object"))
        .respond_with(xml_fault(2, "Object appex.backup.api doesn't exist"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/web/database/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": 200, "message": "Odoo Server Error", "data": {"name": "odoo.exceptions.AccessDenied", "message": "Access Denied"}}
        })))
        .mount(&server)
        .await;

    let input = ProbeInput {
        base_url: base(&server),
        database: Some("db17".into()),
        credentials: Some(credentials(SecretKind::Password)),
        master_password: None,
        protocol_preference: ProtocolPreference::Auto,
    };
    let report = probe(&common::client(), input).await.unwrap();
    assert_eq!(report.protocol, Some(RpcProtocol::XmlRpc));
    assert_eq!(report.auth, CheckStatus::Ok);
    assert!(matches!(&report.module, CheckStatus::Failed { code, .. } if code == "module_not_installed"));
    assert!(matches!(&report.db_manager, CheckStatus::Failed { code, .. } if code == "db_manager_disabled"));
    assert_eq!(report.recommended_transport, None);
}

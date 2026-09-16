use std::collections::HashMap;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use obd_storage::StorageError;
use obd_storage::gdrive::{GoogleEndpoints, OAuthClient, PendingAuthorization};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn endpoints(server: &MockServer) -> GoogleEndpoints {
    GoogleEndpoints::with_base(&Url::parse(&format!("{}/", server.uri())).unwrap())
}

fn client(secret: Option<&str>) -> OAuthClient {
    OAuthClient { client_id: "cid.apps.googleusercontent.com".into(), client_secret: secret.map(SecretString::from) }
}

fn query(url: &Url) -> HashMap<String, String> {
    url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

async fn start(server: &MockServer, secret: Option<&str>) -> (PendingAuthorization, HashMap<String, String>) {
    let pending = PendingAuthorization::start_with_endpoints(http(), client(secret), endpoints(server)).await.unwrap();
    let params = query(pending.authorize_url());
    (pending, params)
}

/// Simulates the browser following the redirect.
async fn browser_get(redirect_uri: &str, path_and_query: &str) -> (u16, String) {
    let response = http().get(format!("{redirect_uri}{path_and_query}")).send().await.unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn authorize_url_has_pkce_state_and_offline_access() {
    let server = MockServer::start().await;
    let (pending, params) = start(&server, None).await;

    assert!(pending.authorize_url().as_str().starts_with(&format!("{}/auth?", server.uri())));
    assert_eq!(params["client_id"], "cid.apps.googleusercontent.com");
    assert_eq!(params["redirect_uri"], pending.redirect_uri());
    assert!(pending.redirect_uri().starts_with("http://127.0.0.1:"));
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["scope"], "https://www.googleapis.com/auth/drive.file");
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["code_challenge"].len(), 43);
    assert!(params["state"].len() >= 32);
    assert_eq!(params["access_type"], "offline");
    assert_eq!(params["prompt"], "consent");

    let (other, other_params) = start(&server, None).await;
    assert_ne!(other.redirect_uri(), pending.redirect_uri());
    assert_ne!(other_params["state"], params["state"]);
    assert_ne!(other_params["code_challenge"], params["code_challenge"]);
}

#[tokio::test]
async fn empty_client_id_is_not_configured() {
    let server = MockServer::start().await;
    let client = OAuthClient { client_id: "  ".into(), client_secret: None };
    let err = PendingAuthorization::start_with_endpoints(http(), client, endpoints(&server)).await.unwrap_err();
    assert!(matches!(err, StorageError::NotConfigured(_)));
}

#[tokio::test]
async fn successful_flow_exchanges_code_with_verifier() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=code-123"))
        .and(body_string_contains("client_secret=csecret"))
        .and(body_string_contains("code_verifier="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-9", "expires_in": 3599, "refresh_token": "rt-9", "scope": "https://www.googleapis.com/auth/drive.file"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .and(header("authorization", "Bearer at-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "user": {"emailAddress": "ops@example.com", "displayName": "Ops"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (pending, params) = start(&server, Some("csecret")).await;
    let redirect_uri = pending.redirect_uri().to_owned();
    let flow = tokio::spawn(pending.finish(CancellationToken::new(), Duration::from_secs(10)));

    // Unrelated requests from the browser are answered and ignored.
    let (status, _) = browser_get(&redirect_uri, "/favicon.ico").await;
    assert_eq!(status, 404);

    let (status, body) =
        browser_get(&redirect_uri, &format!("/?state={}&code=code-123&scope=x", params["state"])).await;
    assert_eq!(status, 200);
    assert!(body.contains("autorización completada, puedes cerrar esta ventana"));

    let account = flow.await.unwrap().unwrap();
    assert_eq!(account.refresh_token.expose_secret(), "rt-9");
    assert_eq!(account.email.as_deref(), Some("ops@example.com"));
    assert_eq!(account.display_name.as_deref(), Some("Ops"));

    // The verifier sent to the token endpoint matches the challenge in the consent URL.
    let requests = server.received_requests().await.unwrap();
    let token_request = requests.iter().find(|r| r.url.path() == "/token").unwrap();
    let form: HashMap<String, String> =
        url::form_urlencoded::parse(&token_request.body).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    assert_eq!(form["redirect_uri"], redirect_uri);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes()));
    assert_eq!(challenge, params["code_challenge"]);
    assert!(form["code_verifier"].len() >= 43);
}

#[tokio::test]
async fn wrong_state_is_rejected_and_denial_ends_the_flow() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let (pending, params) = start(&server, None).await;
    let redirect_uri = pending.redirect_uri().to_owned();
    let flow = tokio::spawn(pending.finish(CancellationToken::new(), Duration::from_secs(10)));

    let (status, body) = browser_get(&redirect_uri, "/?state=forged&code=evil").await;
    assert_eq!(status, 400);
    assert!(body.contains("no válida"));
    assert!(!flow.is_finished());

    let (status, body) = browser_get(&redirect_uri, &format!("/?state={}&error=access_denied", params["state"])).await;
    assert_eq!(status, 200);
    assert!(body.contains("cancelada"));

    let err = flow.await.unwrap().unwrap_err();
    assert!(matches!(&err, StorageError::AuthorizationDenied(e) if e == "access_denied"), "{err:?}");
}

#[tokio::test]
async fn missing_refresh_token_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at", "expires_in": 3600})))
        .mount(&server)
        .await;

    let (pending, params) = start(&server, None).await;
    let redirect_uri = pending.redirect_uri().to_owned();
    let flow = tokio::spawn(pending.finish(CancellationToken::new(), Duration::from_secs(10)));
    let (status, body) = browser_get(&redirect_uri, &format!("/?state={}&code=c", params["state"])).await;
    assert_eq!(status, 200);
    assert!(body.contains("no se pudo completar"));

    let err = flow.await.unwrap().unwrap_err();
    assert!(matches!(&err, StorageError::AuthorizationDenied(m) if m.contains("no refresh token")), "{err:?}");
}

#[tokio::test]
async fn rejected_code_is_an_authorization_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": "invalid_grant", "error_description": "Bad Request"})),
        )
        .mount(&server)
        .await;

    let (pending, params) = start(&server, None).await;
    let redirect_uri = pending.redirect_uri().to_owned();
    let flow = tokio::spawn(pending.finish(CancellationToken::new(), Duration::from_secs(10)));
    browser_get(&redirect_uri, &format!("/?state={}&code=stale", params["state"])).await;
    let err = flow.await.unwrap().unwrap_err();
    assert!(matches!(err, StorageError::AuthorizationDenied(_)), "{err:?}");
}

#[tokio::test]
async fn silent_connections_do_not_block_the_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at", "refresh_token": "rt"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let (pending, params) = start(&server, None).await;
    let redirect_uri = pending.redirect_uri().to_owned();
    let flow = tokio::spawn(pending.finish(CancellationToken::new(), Duration::from_secs(10)));

    // A browser pre-connect that never sends a request.
    let _idle = tokio::net::TcpStream::connect(redirect_uri.trim_start_matches("http://")).await.unwrap();

    let (status, _) = browser_get(&redirect_uri, &format!("/?state={}&code=c", params["state"])).await;
    assert_eq!(status, 200);
    // about.get failing is not fatal: the account is connected without email.
    let account = flow.await.unwrap().unwrap();
    assert_eq!(account.refresh_token.expose_secret(), "rt");
    assert_eq!(account.email, None);
}

#[tokio::test]
async fn finish_honors_timeout_and_cancellation() {
    let server = MockServer::start().await;

    let (pending, _) = start(&server, None).await;
    let err = pending.finish(CancellationToken::new(), Duration::from_millis(50)).await.unwrap_err();
    assert!(matches!(err, StorageError::Timeout));

    let (pending, _) = start(&server, None).await;
    let cancel = CancellationToken::new();
    let flow = tokio::spawn(pending.finish(cancel.clone(), Duration::from_secs(30)));
    cancel.cancel();
    let err = flow.await.unwrap().unwrap_err();
    assert!(matches!(err, StorageError::Cancelled));
}

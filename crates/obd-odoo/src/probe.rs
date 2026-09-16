//! Connection diagnostics shown in the instance form ("Probar conexión").

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Client;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use url::{Host, Url};

use crate::net::{endpoint, map_reqwest, read_text_limited};
use crate::rpc::{Credentials, ProtocolPreference, RpcProtocol, SecretKind, connect, select_protocol};
use crate::transport::TransportKind;
use crate::{OdooError, OdooVersion, Result, detect_version};

/// Model exposed by the `obd_backup` module.
pub const MODULE_API_MODEL: &str = "obd.backup.api";
/// API version understood by this client.
pub const MODULE_API_VERSION: u32 = 1;

const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct ProbeInput {
    pub base_url: Url,
    /// `None` → derived from the first DNS label of the host (`dbfilter = ^%d$`).
    pub database: Option<String>,
    pub credentials: Option<Credentials>,
    pub master_password: Option<SecretString>,
    pub protocol_preference: ProtocolPreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Failed { code: String, message: String },
    Skipped { reason: String },
}

impl CheckStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    fn failed(err: &OdooError) -> Self {
        Self::Failed { code: err.code().to_owned(), message: err.to_string() }
    }

    fn skipped(reason: &str) -> Self {
        Self::Skipped { reason: reason.to_owned() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeWarning {
    /// URL is http:// — credentials travel in clear text.
    InsecureHttp,
    /// Version outside 15.0–19.0.
    UnsupportedVersion,
    /// XML-RPC selected on Odoo 19 (deprecated, removed in Odoo 22).
    DeprecatedXmlRpc,
    /// Database name was derived from the subdomain.
    DatabaseDerivedFromHost,
    /// DB manager transport sends the master password; if the server still uses the
    /// default "admin" master password, Odoo replaces it with the one sent.
    MasterPasswordOverWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub base_url: String,
    pub https: bool,
    pub version: OdooVersion,
    pub supported: bool,
    pub database: Option<String>,
    pub protocol: Option<RpcProtocol>,
    pub uid: Option<i64>,
    pub auth: CheckStatus,
    /// `obd.backup.api/get_info` answered with a compatible `api_version`.
    pub module: CheckStatus,
    pub module_api_version: Option<u32>,
    /// Database manager enabled (`list_db = True`) — checked via `/web/database/list`.
    pub db_manager: CheckStatus,
    pub recommended_transport: Option<TransportKind>,
    pub warnings: Vec<ProbeWarning>,
}

/// Runs all checks. Only fails when the server/version cannot be reached at all;
/// every other problem is reported inside the [`ProbeReport`].
pub async fn probe(http: &Client, input: ProbeInput) -> Result<ProbeReport> {
    let mut warnings = Vec::new();
    let https = input.base_url.scheme() == "https";
    if !https {
        warnings.push(ProbeWarning::InsecureHttp);
    }

    let version = detect_version(http, &input.base_url).await?;
    let supported = version.is_supported();
    if !supported {
        warnings.push(ProbeWarning::UnsupportedVersion);
    }

    let database = match input.database.as_deref().map(str::trim).filter(|db| !db.is_empty()) {
        Some(db) => Some(db.to_owned()),
        None => {
            let derived = database_from_host(&input.base_url);
            if derived.is_some() {
                warnings.push(ProbeWarning::DatabaseDerivedFromHost);
            }
            derived
        }
    };
    if input.master_password.is_some() {
        warnings.push(ProbeWarning::MasterPasswordOverWire);
    }

    let mut protocol = None;
    let mut uid = None;
    let mut module_api_version = None;
    let (auth, module) = match (&input.credentials, &database) {
        (None, _) => (CheckStatus::skipped("no_credentials"), CheckStatus::skipped("no_credentials")),
        (Some(_), None) => (CheckStatus::skipped("no_database"), CheckStatus::skipped("no_database")),
        (Some(credentials), Some(db)) => match select_protocol(&version, input.protocol_preference, credentials.kind) {
            Err(err) => (CheckStatus::failed(&err), CheckStatus::skipped("auth_failed")),
            Ok(selected) => {
                protocol = Some(selected);
                if selected == RpcProtocol::XmlRpc && version.major >= 19 {
                    warnings.push(ProbeWarning::DeprecatedXmlRpc);
                }
                let rpc = connect(http.clone(), input.base_url.clone(), db.clone(), credentials.clone(), selected);
                match with_timeout(rpc.authenticate()).await {
                    Err(err) => (CheckStatus::failed(&err), CheckStatus::skipped("auth_failed")),
                    Ok(user_id) => {
                        uid = Some(user_id);
                        let module = match with_timeout(rpc.call(MODULE_API_MODEL, "get_info", &[], Map::new())).await {
                            Ok(info) => match module_api_version_of(&info) {
                                Some(MODULE_API_VERSION) => {
                                    module_api_version = Some(MODULE_API_VERSION);
                                    CheckStatus::Ok
                                }
                                other => {
                                    let found = other.unwrap_or(0);
                                    module_api_version = other;
                                    CheckStatus::failed(&OdooError::ModuleApiIncompatible(found))
                                }
                            },
                            Err(err) if err.is_model_not_found() => CheckStatus::failed(&OdooError::ModuleNotInstalled),
                            Err(err) => CheckStatus::failed(&err),
                        };
                        (CheckStatus::Ok, module)
                    }
                }
            }
        },
    };

    let db_manager = check_db_manager(http, &input.base_url, database.as_deref()).await;

    let api_key = input.credentials.as_ref().is_some_and(|c| c.kind == SecretKind::ApiKey);
    let recommended_transport = if module.is_ok() && api_key {
        Some(TransportKind::ObdModule)
    } else if db_manager.is_ok() && input.master_password.is_some() {
        Some(TransportKind::DbManager)
    } else {
        None
    };

    Ok(ProbeReport {
        base_url: input.base_url.to_string(),
        https,
        version,
        supported,
        database,
        protocol,
        uid,
        auth,
        module,
        module_api_version,
        db_manager,
        recommended_transport,
        warnings,
    })
}

async fn with_timeout<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(CHECK_TIMEOUT, future).await.map_err(|_| OdooError::Timeout)?
}

fn module_api_version_of(info: &Value) -> Option<u32> {
    info.get("api_version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())
}

/// `POST /web/database/list` (JSON-RPC). With `list_db = False` Odoo answers with an
/// `AccessDenied` error; the list is filtered by `dbfilter` using the Host header.
pub(crate) async fn check_db_manager(http: &Client, base_url: &Url, database: Option<&str>) -> CheckStatus {
    match list_databases(http, base_url).await {
        Ok(databases) => match database {
            Some(db) if !databases.iter().any(|name| name == db) => CheckStatus::Failed {
                code: "database_not_listed".into(),
                message: format!("database {db:?} is not listed by the database manager (dbfilter/host)"),
            },
            _ => CheckStatus::Ok,
        },
        Err(err) => CheckStatus::failed(&err),
    }
}

async fn list_databases(http: &Client, base_url: &Url) -> Result<Vec<String>> {
    let url = endpoint(base_url, "web/database/list")?;
    let response = http
        .post(url)
        .json(&json!({"jsonrpc": "2.0", "method": "call", "params": {}, "id": 1}))
        .timeout(CHECK_TIMEOUT)
        .send()
        .await
        .map_err(map_reqwest)?;
    let status = response.status();
    if !status.is_success() {
        let text = read_text_limited(response, 1024).await;
        return Err(OdooError::HttpStatus {
            status: status.as_u16(),
            message: text.trim().chars().take(200).collect(),
        });
    }
    let body: Value = response.json().await.map_err(|err| OdooError::Protocol(err.to_string()))?;
    if let Some(error) = body.get("error") {
        let name = error.pointer("/data/name").and_then(Value::as_str).unwrap_or_default();
        let message = error
            .pointer("/data/message")
            .or_else(|| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        return Err(if name.contains("AccessDenied") {
            OdooError::DatabaseManagerDisabled
        } else {
            OdooError::Rpc { code: "rpc".into(), message }
        });
    }
    let list = body
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| OdooError::Protocol(format!("unexpected body: {body}")))?;
    Ok(list.iter().filter_map(Value::as_str).map(str::to_owned).collect())
}

/// `https://cliente1.nube.com` → `Some("cliente1")`; strips a leading `www.`;
/// returns `None` for IP addresses and single-label hosts.
pub fn database_from_host(url: &Url) -> Option<String> {
    let domain = match url.host()? {
        Host::Domain(domain) => domain,
        Host::Ipv4(_) | Host::Ipv6(_) => return None,
    };
    if domain.parse::<IpAddr>().is_ok() {
        return None;
    }
    let domain = domain.strip_prefix("www.").unwrap_or(domain);
    let (first, rest) = domain.split_once('.')?;
    if first.is_empty() || rest.is_empty() {
        return None;
    }
    Some(first.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(url: &str) -> Option<String> {
        database_from_host(&Url::parse(url).unwrap())
    }

    #[test]
    fn derives_database_from_subdomain() {
        assert_eq!(db("https://cliente1.nube.com").as_deref(), Some("cliente1"));
        assert_eq!(db("https://cliente1.nube.com:8443/web").as_deref(), Some("cliente1"));
        assert_eq!(db("https://www.cliente2.nube.com").as_deref(), Some("cliente2"));
        assert_eq!(db("http://it15.localhost:18015").as_deref(), Some("it15"));
        assert_eq!(db("http://localhost:8069"), None);
        assert_eq!(db("http://127.0.0.1:8069"), None);
        assert_eq!(db("http://[::1]:8069"), None);
    }
}

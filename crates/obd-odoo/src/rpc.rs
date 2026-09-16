//! Protocol adapters. XML-RPC (`/xmlrpc/2/common`, `/xmlrpc/2/object`) is used for
//! Odoo 15–18; JSON-2 (`POST /json/2/<model>/<method>`, bearer API key) for Odoo ≥ 19.

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::{OdooError, OdooVersion, Result};

/// `OdooError::Rpc.code` used when the called model does not exist on the database.
pub const MODEL_NOT_FOUND: &str = "model_not_found";

/// First major version that ships JSON-2.
const JSON2_SINCE: u16 = 19;
/// First major version without `/xmlrpc` and `/jsonrpc` (announced in Odoo 19).
const XMLRPC_REMOVED_IN: u16 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcProtocol {
    XmlRpc,
    Json2,
}

impl RpcProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::XmlRpc => "XML-RPC",
            Self::Json2 => "JSON-2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolPreference {
    #[default]
    Auto,
    XmlRpc,
    Json2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    ApiKey,
}

/// Login + password or API key. XML-RPC accepts both; JSON-2 requires an API key.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub login: String,
    pub secret: SecretString,
    pub kind: SecretKind,
}

/// Chooses the protocol for a server version.
///
/// `Auto`: JSON-2 when `major >= 19` and the secret is an API key, XML-RPC otherwise.
/// Explicit `Json2` on `major < 19` or with a password → `UnsupportedProtocol`.
/// Explicit `XmlRpc` is allowed on 15–19 (deprecated on 19).
pub fn select_protocol(version: &OdooVersion, preference: ProtocolPreference, kind: SecretKind) -> Result<RpcProtocol> {
    let unsupported = |protocol: RpcProtocol, reason: String| OdooError::UnsupportedProtocol {
        protocol: protocol.label().to_owned(),
        reason,
    };
    let json2_available = version.major >= JSON2_SINCE;
    let xmlrpc_available = version.major < XMLRPC_REMOVED_IN;

    match preference {
        ProtocolPreference::Auto => {
            if json2_available && kind == SecretKind::ApiKey {
                Ok(RpcProtocol::Json2)
            } else if xmlrpc_available {
                Ok(RpcProtocol::XmlRpc)
            } else {
                Err(unsupported(RpcProtocol::Json2, "JSON-2 requires an API key".into()))
            }
        }
        ProtocolPreference::XmlRpc => {
            if xmlrpc_available {
                Ok(RpcProtocol::XmlRpc)
            } else {
                Err(unsupported(RpcProtocol::XmlRpc, format!("XML-RPC was removed in Odoo {XMLRPC_REMOVED_IN}")))
            }
        }
        ProtocolPreference::Json2 => {
            if !json2_available {
                Err(unsupported(RpcProtocol::Json2, format!("JSON-2 requires Odoo {JSON2_SINCE}.0 or newer")))
            } else if kind != SecretKind::ApiKey {
                Err(unsupported(RpcProtocol::Json2, "JSON-2 requires an API key".into()))
            } else {
                Ok(RpcProtocol::Json2)
            }
        }
    }
}

#[async_trait]
pub trait OdooRpc: Send + Sync {
    fn protocol(&self) -> RpcProtocol;
    fn database(&self) -> &str;

    /// Verifies the credentials and returns the user id.
    async fn authenticate(&self) -> Result<i64>;

    /// Calls `model.method`. `ids` is empty for `@api.model` methods; `kwargs` are
    /// passed by name (JSON-2 body / XML-RPC `execute_kw` kwargs).
    ///
    /// A missing model is reported as `OdooError::Rpc` with code [`MODEL_NOT_FOUND`]
    /// (see [`OdooError::is_model_not_found`]).
    async fn call(&self, model: &str, method: &str, ids: &[i64], kwargs: Map<String, Value>) -> Result<Value>;
}

/// Builds the adapter for `protocol`. Authentication happens lazily on first call.
pub fn connect(
    http: Client,
    base_url: Url,
    database: String,
    credentials: Credentials,
    protocol: RpcProtocol,
) -> Arc<dyn OdooRpc> {
    match protocol {
        RpcProtocol::XmlRpc => Arc::new(crate::xmlrpc::XmlRpcClient::new(http, base_url, database, credentials)),
        RpcProtocol::Json2 => Arc::new(crate::json2::Json2Client::new(http, base_url, database, credentials)),
    }
}

/// Whether a server message reports that `model` does not exist.
///
/// Odoo 15–19 XML-RPC: `Object <model> doesn't exist` (UserError).
/// Odoo 19 JSON-2: `the model '<model>' does not exist` (404).
pub(crate) fn is_missing_model_message(message: &str, model: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains(&model.to_ascii_lowercase())
        && (lower.contains("doesn't exist") || lower.contains("does not exist"))
        && !lower.contains("method")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u16) -> OdooVersion {
        OdooVersion { major, minor: 0, server_version: format!("{major}.0"), saas: false }
    }

    #[test]
    fn auto_selection_matrix() {
        use ProtocolPreference::Auto;
        use SecretKind::{ApiKey, Password};
        for major in 15..=18 {
            assert_eq!(select_protocol(&version(major), Auto, ApiKey).unwrap(), RpcProtocol::XmlRpc);
            assert_eq!(select_protocol(&version(major), Auto, Password).unwrap(), RpcProtocol::XmlRpc);
        }
        assert_eq!(select_protocol(&version(19), Auto, ApiKey).unwrap(), RpcProtocol::Json2);
        assert_eq!(select_protocol(&version(19), Auto, Password).unwrap(), RpcProtocol::XmlRpc);
        assert_eq!(select_protocol(&version(22), Auto, ApiKey).unwrap(), RpcProtocol::Json2);
        assert!(matches!(select_protocol(&version(22), Auto, Password), Err(OdooError::UnsupportedProtocol { .. })));
    }

    #[test]
    fn explicit_preferences() {
        use SecretKind::{ApiKey, Password};
        assert_eq!(select_protocol(&version(19), ProtocolPreference::XmlRpc, ApiKey).unwrap(), RpcProtocol::XmlRpc);
        assert_eq!(select_protocol(&version(15), ProtocolPreference::XmlRpc, Password).unwrap(), RpcProtocol::XmlRpc);
        assert!(select_protocol(&version(22), ProtocolPreference::XmlRpc, Password).is_err());
        assert!(select_protocol(&version(18), ProtocolPreference::Json2, ApiKey).is_err());
        assert!(select_protocol(&version(19), ProtocolPreference::Json2, Password).is_err());
        assert_eq!(select_protocol(&version(19), ProtocolPreference::Json2, ApiKey).unwrap(), RpcProtocol::Json2);
    }

    #[test]
    fn missing_model_messages() {
        assert!(is_missing_model_message("Object obd.backup.api doesn't exist", "obd.backup.api"));
        assert!(is_missing_model_message("the model 'obd.backup.api' does not exist", "obd.backup.api"));
        assert!(!is_missing_model_message("Object res.partner doesn't exist", "obd.backup.api"));
        assert!(!is_missing_model_message("The method 'obd.backup.api.nope' does not exist", "obd.backup.api"));
    }
}

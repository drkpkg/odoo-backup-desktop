//! Server version detection (no credentials required).
//!
//! 1. `GET /web/version` (Odoo ≥ 19, `rpc` module) → `{"version_info": [...], "version": "19.0"}`
//! 2. otherwise XML-RPC `POST /xmlrpc/2/common` → `version()` → `server_version_info`

use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::net::{endpoint, map_reqwest};
use crate::{OdooError, Result};

/// Version detection is cheap; do not wait for a hanging server forever.
const DETECTION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OdooVersion {
    /// Major series, e.g. 17 for "17.0" and for "saas~17.2".
    pub major: u16,
    /// Minor series: 0 for LTS releases, 2 for "saas~17.2".
    pub minor: u16,
    /// Raw `server_version` string reported by the server.
    pub server_version: String,
    pub saas: bool,
}

impl OdooVersion {
    /// Parses strings such as "17.0", "17.0+e", "saas~17.2", "19.0-20250918".
    pub fn parse(server_version: &str) -> Option<Self> {
        let raw = server_version.trim();
        let (numbers, saas) = match raw.strip_prefix("saas~").or_else(|| raw.strip_prefix("saas-")) {
            Some(rest) => (rest, true),
            None => (raw, false),
        };
        let (major, rest) = leading_number(numbers)?;
        let minor = match rest.strip_prefix('.') {
            Some(after_dot) => leading_number(after_dot).map_or(0, |(minor, _)| minor),
            None if saas => 0,
            None => return None,
        };
        Some(Self { major, minor, server_version: raw.to_owned(), saas })
    }

    /// Builds a version from `server_version_info` (`[17, 0, 0, "final", 0, ""]` or
    /// `["saas~17", 2, 0, "final", 0, ""]`) when `server_version` is not parseable.
    fn from_version_info(info: &Value, server_version: &str) -> Option<Self> {
        let items = info.as_array()?;
        let (major, saas) = match items.first()? {
            Value::Number(n) => (u16::try_from(n.as_u64()?).ok()?, false),
            Value::String(s) => {
                let parsed = Self::parse(s)?;
                (parsed.major, parsed.saas)
            }
            _ => return None,
        };
        let minor = items.get(1).and_then(Value::as_u64).and_then(|m| u16::try_from(m).ok()).unwrap_or(0);
        let raw = if server_version.is_empty() { format!("{major}.{minor}") } else { server_version.to_owned() };
        Some(Self { major, minor, server_version: raw, saas: saas || server_version.starts_with("saas") })
    }

    pub fn is_supported(&self) -> bool {
        crate::SUPPORTED_MAJOR_VERSIONS.contains(&self.major)
    }

    /// Label for the UI, e.g. "17.0" or "saas~17.2".
    pub fn label(&self) -> String {
        if self.saas { format!("saas~{}.{}", self.major, self.minor) } else { format!("{}.{}", self.major, self.minor) }
    }
}

fn leading_number(input: &str) -> Option<(u16, &str)> {
    let digits = input.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value = input[..digits].parse().ok()?;
    Some((value, &input[digits..]))
}

/// Detects the Odoo version of the server at `base_url`.
pub async fn detect_version(http: &Client, base_url: &Url) -> Result<OdooVersion> {
    let web_version_error = match detect_with_web_version(http, base_url).await {
        Ok(version) => return Ok(version),
        Err(err) => err,
    };
    match detect_with_xmlrpc(http, base_url).await {
        Ok(version) => Ok(version),
        // Both endpoints unreachable: report the network problem itself.
        Err(err @ (OdooError::Connection(_) | OdooError::Timeout | OdooError::InvalidUrl(_))) => Err(err),
        Err(err) => Err(OdooError::VersionDetection(format!("/web/version: {web_version_error}; XML-RPC: {err}"))),
    }
}

async fn detect_with_web_version(http: &Client, base_url: &Url) -> Result<OdooVersion> {
    let url = endpoint(base_url, "web/version")?;
    let response = http.get(url).timeout(DETECTION_TIMEOUT).send().await.map_err(map_reqwest)?;
    if response.status() != StatusCode::OK {
        return Err(OdooError::HttpStatus { status: response.status().as_u16(), message: "no /web/version".into() });
    }
    let body: Value = response.json().await.map_err(|err| OdooError::Protocol(err.to_string()))?;
    let server_version = body.get("version").and_then(Value::as_str).unwrap_or_default();
    OdooVersion::parse(server_version)
        .or_else(|| body.get("version_info").and_then(|info| OdooVersion::from_version_info(info, server_version)))
        .ok_or_else(|| OdooError::Protocol(format!("unexpected /web/version body: {body}")))
}

async fn detect_with_xmlrpc(http: &Client, base_url: &Url) -> Result<OdooVersion> {
    let result = tokio::time::timeout(DETECTION_TIMEOUT, crate::xmlrpc::call(http, base_url, "common", "version", &[]))
        .await
        .map_err(|_| OdooError::Timeout)??;
    let server_version = result.get("server_version").and_then(Value::as_str).unwrap_or_default();
    OdooVersion::parse(server_version)
        .or_else(|| {
            result.get("server_version_info").and_then(|info| OdooVersion::from_version_info(info, server_version))
        })
        .ok_or_else(|| OdooError::Protocol(format!("unexpected version() result: {result}")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_version_strings() {
        let v = OdooVersion::parse("17.0").unwrap();
        assert_eq!((v.major, v.minor, v.saas), (17, 0, false));
        assert_eq!(v.label(), "17.0");

        let v = OdooVersion::parse("19.0+e-20250918").unwrap();
        assert_eq!((v.major, v.minor, v.saas), (19, 0, false));
        assert_eq!(v.server_version, "19.0+e-20250918");

        let v = OdooVersion::parse("saas~17.2").unwrap();
        assert_eq!((v.major, v.minor, v.saas), (17, 2, true));
        assert_eq!(v.label(), "saas~17.2");

        let v = OdooVersion::parse(" 15.0-20230101 ").unwrap();
        assert_eq!((v.major, v.minor), (15, 0));
        assert!(v.is_supported());

        assert!(!OdooVersion::parse("14.0").unwrap().is_supported());
        assert!(!OdooVersion::parse("20.0").unwrap().is_supported());
        assert!(OdooVersion::parse("master").is_none());
        assert!(OdooVersion::parse("").is_none());
        assert!(OdooVersion::parse("17").is_none());
    }

    #[test]
    fn builds_from_version_info() {
        let v = OdooVersion::from_version_info(&json!([16, 0, 0, "final", 0, ""]), "").unwrap();
        assert_eq!((v.major, v.minor, v.server_version.as_str()), (16, 0, "16.0"));
        let v = OdooVersion::from_version_info(&json!(["saas~17", 4, 0, "final", 0, ""]), "odd").unwrap();
        assert_eq!((v.major, v.minor, v.saas), (17, 4, true));
        assert!(OdooVersion::from_version_info(&json!({}), "").is_none());
    }
}

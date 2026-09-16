//! HTTP helpers shared by the RPC adapters and transports.

use std::error::Error as _;
use std::future::Future;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{OdooError, Result};

/// Joins `path` (relative, no leading slash needed) to the instance base URL,
/// keeping any sub-path of the base (`https://host/odoo/` + `web/version`).
pub(crate) fn endpoint(base_url: &Url, path: &str) -> Result<Url> {
    let mut base = base_url.clone();
    base.set_query(None);
    base.set_fragment(None);
    if !base.path().ends_with('/') {
        let with_slash = format!("{}/", base.path());
        base.set_path(&with_slash);
    }
    base.join(path.trim_start_matches('/')).map_err(|err| OdooError::InvalidUrl(err.to_string()))
}

/// Maps a transport-level reqwest error to [`OdooError`].
pub(crate) fn map_reqwest(err: reqwest::Error) -> OdooError {
    if err.is_timeout() {
        OdooError::Timeout
    } else if err.is_builder() {
        OdooError::InvalidUrl(error_chain(&err))
    } else if err.is_decode() {
        OdooError::Protocol(error_chain(&err))
    } else {
        OdooError::Connection(error_chain(&err))
    }
}

/// `outer: inner: innermost` — reqwest hides the useful cause in `source()`.
pub(crate) fn error_chain(err: &reqwest::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        source = cause.source();
    }
    message
}

/// Reads at most `limit` bytes of the body as (lossy) UTF-8. Used for error pages.
pub(crate) async fn read_text_limited(response: reqwest::Response, limit: usize) -> String {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        let room = limit.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= limit {
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// Runs `future` unless `cancel` fires first.
pub(crate) async fn cancellable<F: Future>(cancel: &CancellationToken, future: F) -> Result<F::Output> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(OdooError::Cancelled),
        output = future => Ok(output),
    }
}

/// Lowercase hex encoding.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_keeps_sub_path_and_drops_query() {
        let base = Url::parse("https://erp.example.com/odoo?x=1").unwrap();
        assert_eq!(endpoint(&base, "/web/version").unwrap().as_str(), "https://erp.example.com/odoo/web/version");
        let root = Url::parse("https://erp.example.com").unwrap();
        assert_eq!(endpoint(&root, "xmlrpc/2/common").unwrap().as_str(), "https://erp.example.com/xmlrpc/2/common");
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(to_hex(&[0x00, 0xab, 0xff]), "00abff");
    }
}

//! Small crate-private helpers: randomness, encodings and retry backoff.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::{Result, StorageError};

pub(crate) fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).map_err(|e| StorageError::Fatal(format!("system randomness unavailable: {e}")))?;
    Ok(buf)
}

pub(crate) fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

/// Maps a transport-level reqwest error. The URL is stripped because resumable
/// session URIs act as upload capabilities.
pub(crate) fn map_reqwest_error(err: reqwest::Error) -> StorageError {
    if err.is_timeout() { StorageError::Timeout } else { StorageError::Transient(err.without_url().to_string()) }
}

/// Bounded exponential backoff with jitter, used for retryable errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first one (minimum 1).
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 5, base_delay: Duration::from_millis(500), max_delay: Duration::from_secs(30) }
    }
}

impl RetryPolicy {
    pub(crate) fn attempts(&self) -> u32 {
        self.max_attempts.max(1)
    }

    /// Delay before retry number `failures` (1-based). A server-provided
    /// `Retry-After` wins over the computed delay; both are capped at `max_delay`.
    pub(crate) fn delay(&self, failures: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(after) = retry_after {
            return after.min(self.max_delay);
        }
        let exp = failures.saturating_sub(1).min(16);
        let base = self.base_delay.saturating_mul(1u32 << exp).min(self.max_delay);
        let jitter_pct = random_bytes(1).map(|b| u32::from(b[0]) % 26).unwrap_or(0);
        (base + base.mul_f64(f64::from(jitter_pct) / 100.0)).min(self.max_delay)
    }
}

/// Escapes a value for a Drive `q` string literal.
pub(crate) fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_base64url() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn escape_quotes_and_backslashes() {
        assert_eq!(escape_query(r"O'Brien \ Co"), r"O\'Brien \\ Co");
    }

    #[test]
    fn backoff_is_bounded() {
        let policy =
            RetryPolicy { max_attempts: 5, base_delay: Duration::from_millis(100), max_delay: Duration::from_secs(1) };
        assert!(policy.delay(1, None) >= Duration::from_millis(100));
        assert!(policy.delay(1, None) <= Duration::from_millis(125));
        assert_eq!(policy.delay(30, None), Duration::from_secs(1));
        assert_eq!(policy.delay(1, Some(Duration::from_secs(7))), Duration::from_secs(1));
    }
}

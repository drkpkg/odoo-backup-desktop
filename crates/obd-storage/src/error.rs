use std::time::Duration;

use thiserror::Error;

pub type Result<T, E = StorageError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("authorization expired or revoked; reconnect the account")]
    AuthExpired,
    #[error("authorization was denied: {0}")]
    AuthorizationDenied(String),
    #[error("storage quota exceeded: {0}")]
    QuotaExceeded(String),
    #[error("rate limited")]
    RateLimited { retry_after: Option<Duration> },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("temporary error: {0}")]
    Transient(String),
    #[error("error: {0}")]
    Fatal(String),
    #[error("operation timed out")]
    Timeout,
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StorageError {
    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured(_) => "storage_not_configured",
            Self::AuthExpired => "storage_auth_expired",
            Self::AuthorizationDenied(_) => "storage_authorization_denied",
            Self::QuotaExceeded(_) => "storage_quota_exceeded",
            Self::RateLimited { .. } => "storage_rate_limited",
            Self::NotFound(_) => "storage_not_found",
            Self::Transient(_) => "storage_transient",
            Self::Fatal(_) => "storage_fatal",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Io(_) => "io",
        }
    }

    /// Whether retrying the same operation may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Transient(_) | Self::Timeout)
    }
}

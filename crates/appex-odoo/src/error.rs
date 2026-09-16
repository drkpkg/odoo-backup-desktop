use thiserror::Error;

pub type Result<T, E = OdooError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum OdooError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("could not connect to the server: {0}")]
    Connection(String),
    #[error("request timed out")]
    Timeout,
    #[error("unexpected HTTP status {status}: {message}")]
    HttpStatus { status: u16, message: String },
    #[error("could not detect the Odoo version: {0}")]
    VersionDetection(String),
    #[error("Odoo {0} is not supported (supported: 15.0 to 19.0)")]
    UnsupportedVersion(String),
    #[error("protocol {protocol} is not available: {reason}")]
    UnsupportedProtocol { protocol: String, reason: String },
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("access denied: {0}")]
    AccessDenied(String),
    #[error("database management is disabled on the server (list_db = False)")]
    DatabaseManagerDisabled,
    #[error("the appex_backup module is not installed on this database")]
    ModuleNotInstalled,
    #[error("incompatible appex_backup module API version {0}")]
    ModuleApiIncompatible(u32),
    #[error("RPC error: {message}")]
    Rpc { code: String, message: String },
    #[error("server reported a backup error: {0}")]
    ServerBackupError(String),
    #[error("backup was not ready before the timeout")]
    PrepareTimeout,
    #[error("invalid backup file: {0}")]
    InvalidBackup(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl OdooError {
    /// The called model does not exist on the database (e.g. module not installed).
    pub fn is_model_not_found(&self) -> bool {
        matches!(self, Self::Rpc { code, .. } if code == crate::rpc::MODEL_NOT_FOUND)
    }

    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "invalid_url",
            Self::Connection(_) => "connection",
            Self::Timeout => "timeout",
            Self::HttpStatus { .. } => "http_status",
            Self::VersionDetection(_) => "version_detection",
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::UnsupportedProtocol { .. } => "unsupported_protocol",
            Self::AuthenticationFailed => "authentication_failed",
            Self::AccessDenied(_) => "access_denied",
            Self::DatabaseManagerDisabled => "db_manager_disabled",
            Self::ModuleNotInstalled => "module_not_installed",
            Self::ModuleApiIncompatible(_) => "module_api_incompatible",
            Self::Rpc { .. } => "rpc",
            Self::ServerBackupError(_) => "server_backup_error",
            Self::PrepareTimeout => "prepare_timeout",
            Self::InvalidBackup(_) => "invalid_backup",
            Self::Protocol(_) => "protocol",
            Self::Cancelled => "cancelled",
            Self::Io(_) => "io",
        }
    }
}

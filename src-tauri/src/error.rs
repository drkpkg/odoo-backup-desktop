use serde::Serialize;

/// Error returned by every command: `{ code, message }`.
///
/// `code` is stable and translated by the UI; `message` is technical detail and
/// must never contain secrets.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

pub type CommandResult<T> = Result<T, CommandError>;

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    pub fn vault_locked() -> Self {
        Self::new("vault_locked", "the vault is locked")
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<obd_vault::VaultError> for CommandError {
    fn from(err: obd_vault::VaultError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}

impl From<obd_odoo::OdooError> for CommandError {
    fn from(err: obd_odoo::OdooError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}

impl From<obd_storage::StorageError> for CommandError {
    fn from(err: obd_storage::StorageError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}

impl From<rusqlite::Error> for CommandError {
    fn from(err: rusqlite::Error) -> Self {
        Self::new("history_db", err.to_string())
    }
}

impl From<std::io::Error> for CommandError {
    fn from(err: std::io::Error) -> Self {
        Self::new("io", err.to_string())
    }
}

impl From<tokio::task::JoinError> for CommandError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::internal(format!("background task failed: {err}"))
    }
}

impl From<tauri::Error> for CommandError {
    fn from(err: tauri::Error) -> Self {
        Self::internal(err.to_string())
    }
}

use thiserror::Error;

pub type Result<T, E = PluginError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("storage key is invalid: {0}")]
    StorageKeyInvalid(String),
    #[error("plugin storage limit exceeded ({limit} bytes)")]
    StorageLimit { limit: usize },
    #[error("invalid plugin id: {0}")]
    InvalidId(String),
    #[error("corrupted store file {path}: {message}")]
    CorruptedStore { path: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl PluginError {
    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::StorageKeyInvalid(_) => "plugin_storage_key_invalid",
            Self::StorageLimit { .. } => "plugin_storage_limit",
            Self::InvalidId(_) => "plugin_not_found",
            Self::CorruptedStore { .. } => "plugin_store_corrupted",
            Self::Io(_) => "io",
        }
    }
}

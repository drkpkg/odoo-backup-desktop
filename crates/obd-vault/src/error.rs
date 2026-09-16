use thiserror::Error;

pub type Result<T, E = VaultError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault file not found")]
    NotFound,
    #[error("vault file already exists")]
    AlreadyExists,
    #[error("wrong master password")]
    WrongPassword,
    #[error("no master password is configured for this vault")]
    PasswordNotEnabled,
    #[error("the OS keychain is not available")]
    KeychainUnavailable,
    #[error("the OS keychain is not enabled for this vault")]
    KeychainNotEnabled,
    #[error("the vault key is missing from the OS keychain")]
    KeychainKeyMissing,
    #[error("the key stored in the OS keychain does not open this vault")]
    KeychainKeyMismatch,
    #[error("vault file is corrupted: {0}")]
    Corrupted(String),
    #[error("unsupported vault format version {0}")]
    UnsupportedVersion(u8),
    #[error("invalid options: {0}")]
    InvalidOptions(String),
    #[error("keychain error: {0}")]
    Keyring(String),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl VaultError {
    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "vault_not_found",
            Self::AlreadyExists => "vault_exists",
            Self::WrongPassword => "vault_wrong_password",
            Self::PasswordNotEnabled => "vault_password_not_enabled",
            Self::KeychainUnavailable => "keychain_unavailable",
            Self::KeychainNotEnabled => "keychain_not_enabled",
            Self::KeychainKeyMissing => "keychain_key_missing",
            Self::KeychainKeyMismatch => "keychain_key_mismatch",
            Self::Corrupted(_) => "vault_corrupted",
            Self::UnsupportedVersion(_) => "vault_unsupported_version",
            Self::InvalidOptions(_) => "vault_invalid_options",
            Self::Keyring(_) => "keychain_error",
            Self::Serde(_) => "vault_serde",
            Self::Io(_) => "io",
        }
    }
}

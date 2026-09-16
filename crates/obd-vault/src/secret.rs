use std::fmt;

use secrecy::SecretString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

/// A secret string stored inside the vault payload.
///
/// Serializable (the whole payload is encrypted), zeroized on drop and
/// redacted in `Debug`. Never send it to the webview.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretField(Zeroizing<String>);

impl SecretField {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_secret_string(&self) -> SecretString {
        SecretString::from(self.0.as_str())
    }
}

impl fmt::Debug for SecretField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretField([redacted])")
    }
}

impl Serialize for SecretField {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

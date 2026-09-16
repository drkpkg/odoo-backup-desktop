//! Cryptographic primitives: randomness, Argon2id and XChaCha20-Poly1305.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::format::{KEY_LEN, NONCE_LEN, SALT_LEN, TAG_LEN, WRAPPED_DEK_LEN};
use crate::{KdfParams, Result, VaultError};

/// AAD binding a wrapped DEK to its purpose.
const DEK_WRAP_AAD: &[u8] = b"OBDVAULT-dek-v1";

pub(crate) type Key = Zeroizing<[u8; KEY_LEN]>;

pub(crate) fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|e| std::io::Error::other(format!("system random generator failed: {e}")))?;
    Ok(out)
}

pub(crate) fn random_key() -> Result<Key> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::fill(key.as_mut()).map_err(|e| std::io::Error::other(format!("system random generator failed: {e}")))?;
    Ok(key)
}

pub(crate) fn argon2_params(kdf: &KdfParams) -> Result<Params> {
    Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.parallelism, Some(KEY_LEN))
        .map_err(|e| VaultError::InvalidOptions(format!("invalid key derivation parameters: {e}")))
}

/// Derives the password key with Argon2id (v0x13).
pub(crate) fn derive_password_key(password: &SecretString, salt: &[u8; SALT_LEN], kdf: &KdfParams) -> Result<Key> {
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params(kdf)?);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.expose_secret().as_bytes(), salt, key.as_mut())
        .map_err(|e| VaultError::InvalidOptions(format!("key derivation failed: {e}")))?;
    Ok(key)
}

fn cipher(key: &[u8; KEY_LEN]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&(*key).into())
}

/// Encrypts in place; `buffer` holds the plaintext and receives ciphertext + tag.
pub(crate) fn seal(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], aad: &[u8], buffer: &mut Vec<u8>) -> Result<()> {
    buffer.reserve(TAG_LEN);
    cipher(key)
        .encrypt_in_place(&XNonce::from(*nonce), aad, buffer)
        .map_err(|_| VaultError::Corrupted("encryption failed".into()))
}

/// Decrypts in place. Returns `false` when authentication fails.
pub(crate) fn open(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], aad: &[u8], buffer: &mut Vec<u8>) -> bool {
    cipher(key).decrypt_in_place(&XNonce::from(*nonce), aad, buffer).is_ok()
}

pub(crate) fn wrap_dek(
    password_key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    dek: &[u8; KEY_LEN],
) -> Result<[u8; WRAPPED_DEK_LEN]> {
    let mut buffer = Zeroizing::new(dek.to_vec());
    seal(password_key, nonce, DEK_WRAP_AAD, &mut buffer)?;
    let mut out = [0u8; WRAPPED_DEK_LEN];
    out.copy_from_slice(&buffer);
    Ok(out)
}

/// Returns `None` when the password key does not authenticate the wrapped DEK.
pub(crate) fn unwrap_dek(
    password_key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    wrapped: &[u8; WRAPPED_DEK_LEN],
) -> Option<Key> {
    let mut buffer = Zeroizing::new(wrapped.to_vec());
    if !open(password_key, nonce, DEK_WRAP_AAD, &mut buffer) || buffer.len() != KEY_LEN {
        return None;
    }
    let mut dek = Zeroizing::new([0u8; KEY_LEN]);
    dek.copy_from_slice(&buffer);
    Some(dek)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_aad_binding() {
        let key = random_key().unwrap();
        let nonce = random_array::<NONCE_LEN>().unwrap();
        let mut buffer = b"hello vault".to_vec();
        seal(&key, &nonce, b"aad", &mut buffer).unwrap();
        assert_eq!(buffer.len(), 11 + TAG_LEN);

        let mut wrong_aad = buffer.clone();
        assert!(!open(&key, &nonce, b"other", &mut wrong_aad));

        assert!(open(&key, &nonce, b"aad", &mut buffer));
        assert_eq!(buffer, b"hello vault");
    }

    #[test]
    fn dek_wrapping() {
        let kdf = KdfParams::insecure_for_tests();
        let salt = random_array::<SALT_LEN>().unwrap();
        let nonce = random_array::<NONCE_LEN>().unwrap();
        let dek = random_key().unwrap();

        let pk = derive_password_key(&SecretString::from("correct horse"), &salt, &kdf).unwrap();
        let wrapped = wrap_dek(&pk, &nonce, &dek).unwrap();
        assert_eq!(*unwrap_dek(&pk, &nonce, &wrapped).unwrap(), *dek);

        let wrong = derive_password_key(&SecretString::from("battery staple"), &salt, &kdf).unwrap();
        assert!(unwrap_dek(&wrong, &nonce, &wrapped).is_none());
    }
}

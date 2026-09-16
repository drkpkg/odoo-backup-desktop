//! Binary layout of `vault.bin` (format version 1).
//!
//! ```text
//! magic "OBDVAULT" (8) | version u8 = 1 | flags u8 (bit0 keychain, bit1 password)
//! m_cost_kib u32 LE | t_cost u32 LE | parallelism u32 LE | salt [16]
//! dek_nonce [24] | wrapped_dek [48]   (zeros if no password)
//! payload_nonce [24]
//! ciphertext (XChaCha20-Poly1305, AAD = all previous bytes)
//! ```

use crate::{KdfParams, Result, VaultError, VaultInfo};

pub(crate) const MAGIC: &[u8; 8] = b"OBDVAULT";
pub(crate) const FORMAT_VERSION: u8 = 1;

pub(crate) const FLAG_KEYCHAIN: u8 = 0b01;
pub(crate) const FLAG_PASSWORD: u8 = 0b10;
const KNOWN_FLAGS: u8 = FLAG_KEYCHAIN | FLAG_PASSWORD;

pub(crate) const KEY_LEN: usize = 32;
pub(crate) const SALT_LEN: usize = 16;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const TAG_LEN: usize = 16;
pub(crate) const WRAPPED_DEK_LEN: usize = KEY_LEN + TAG_LEN;

pub(crate) const HEADER_LEN: usize = MAGIC.len() + 1 + 1 + 12 + SALT_LEN + NONCE_LEN + WRAPPED_DEK_LEN + NONCE_LEN;

/// Upper bounds accepted when reading a header, so a tampered file cannot make
/// the password derivation allocate or loop without limit.
const MAX_M_COST_KIB: u32 = 4 * 1024 * 1024;
const MAX_T_COST: u32 = 64;
const MAX_PARALLELISM: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub flags: u8,
    pub kdf: KdfParams,
    pub salt: [u8; SALT_LEN],
    pub dek_nonce: [u8; NONCE_LEN],
    pub wrapped_dek: [u8; WRAPPED_DEK_LEN],
    pub payload_nonce: [u8; NONCE_LEN],
}

impl Header {
    pub fn keychain_enabled(&self) -> bool {
        self.flags & FLAG_KEYCHAIN != 0
    }

    pub fn password_enabled(&self) -> bool {
        self.flags & FLAG_PASSWORD != 0
    }

    pub fn info(&self) -> VaultInfo {
        VaultInfo { keychain_enabled: self.keychain_enabled(), password_enabled: self.password_enabled() }
    }

    /// Clears the password material (flag, KDF params, salt, wrapped DEK).
    pub fn clear_password(&mut self) {
        self.flags &= !FLAG_PASSWORD;
        self.kdf = KdfParams { m_cost_kib: 0, t_cost: 0, parallelism: 0 };
        self.salt = [0; SALT_LEN];
        self.dek_nonce = [0; NONCE_LEN];
        self.wrapped_dek = [0; WRAPPED_DEK_LEN];
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        let mut w = Writer { buf: &mut out, pos: 0 };
        w.put(MAGIC);
        w.put(&[FORMAT_VERSION, self.flags]);
        w.put(&self.kdf.m_cost_kib.to_le_bytes());
        w.put(&self.kdf.t_cost.to_le_bytes());
        w.put(&self.kdf.parallelism.to_le_bytes());
        w.put(&self.salt);
        w.put(&self.dek_nonce);
        w.put(&self.wrapped_dek);
        w.put(&self.payload_nonce);
        debug_assert_eq!(w.pos, HEADER_LEN);
        out
    }

    /// Parses the header and returns it with the remaining ciphertext.
    pub fn decode(bytes: &[u8]) -> Result<(Self, &[u8])> {
        if bytes.len() < MAGIC.len() + 1 || &bytes[..MAGIC.len()] != MAGIC {
            return Err(VaultError::Corrupted("not an Odoo Backup Desktop vault file".into()));
        }
        let version = bytes[MAGIC.len()];
        if version != FORMAT_VERSION {
            return Err(VaultError::UnsupportedVersion(version));
        }
        if bytes.len() < HEADER_LEN + TAG_LEN {
            return Err(VaultError::Corrupted("file is truncated".into()));
        }

        let mut r = Reader { buf: bytes, pos: MAGIC.len() + 1 };
        let flags = r.array::<1>()[0];
        let kdf = KdfParams {
            m_cost_kib: u32::from_le_bytes(r.array()),
            t_cost: u32::from_le_bytes(r.array()),
            parallelism: u32::from_le_bytes(r.array()),
        };
        let header = Header {
            flags,
            kdf,
            salt: r.array(),
            dek_nonce: r.array(),
            wrapped_dek: r.array(),
            payload_nonce: r.array(),
        };
        debug_assert_eq!(r.pos, HEADER_LEN);

        if flags & !KNOWN_FLAGS != 0 || flags & KNOWN_FLAGS == 0 {
            return Err(VaultError::Corrupted("invalid header flags".into()));
        }
        if header.password_enabled() && !kdf_within_bounds(&kdf) {
            return Err(VaultError::Corrupted("invalid key derivation parameters".into()));
        }
        Ok((header, &bytes[HEADER_LEN..]))
    }
}

fn kdf_within_bounds(kdf: &KdfParams) -> bool {
    (1..=MAX_PARALLELISM).contains(&kdf.parallelism)
        && (1..=MAX_T_COST).contains(&kdf.t_cost)
        && kdf.m_cost_kib >= 8 * kdf.parallelism
        && kdf.m_cost_kib <= MAX_M_COST_KIB
}

struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) {
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        out.copy_from_slice(&self.buf[self.pos..self.pos + N]);
        self.pos += N;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header {
            flags: FLAG_KEYCHAIN | FLAG_PASSWORD,
            kdf: KdfParams::default(),
            salt: [1; SALT_LEN],
            dek_nonce: [2; NONCE_LEN],
            wrapped_dek: [3; WRAPPED_DEK_LEN],
            payload_nonce: [4; NONCE_LEN],
        }
    }

    #[test]
    fn header_roundtrip() {
        let header = sample();
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(&[9; TAG_LEN + 5]);
        let (decoded, rest) = Header::decode(&bytes).unwrap();
        assert_eq!(decoded, header);
        assert_eq!(rest.len(), TAG_LEN + 5);
        assert_eq!(HEADER_LEN, 134);
    }

    #[test]
    fn rejects_bad_magic_version_flags_and_kdf() {
        let mut bytes = sample().encode().to_vec();
        bytes.extend_from_slice(&[0; TAG_LEN]);

        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert!(matches!(Header::decode(&bad_magic), Err(VaultError::Corrupted(_))));

        let mut bad_version = bytes.clone();
        bad_version[8] = 7;
        assert!(matches!(Header::decode(&bad_version), Err(VaultError::UnsupportedVersion(7))));

        let mut no_flags = bytes.clone();
        no_flags[9] = 0;
        assert!(matches!(Header::decode(&no_flags), Err(VaultError::Corrupted(_))));

        let mut unknown_flag = bytes.clone();
        unknown_flag[9] = 0b100;
        assert!(matches!(Header::decode(&unknown_flag), Err(VaultError::Corrupted(_))));

        let mut huge_memory = sample();
        huge_memory.kdf.m_cost_kib = u32::MAX;
        let mut huge = huge_memory.encode().to_vec();
        huge.extend_from_slice(&[0; TAG_LEN]);
        assert!(matches!(Header::decode(&huge), Err(VaultError::Corrupted(_))));

        assert!(matches!(Header::decode(&bytes[..HEADER_LEN]), Err(VaultError::Corrupted(_))));
    }
}

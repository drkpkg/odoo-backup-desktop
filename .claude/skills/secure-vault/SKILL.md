---
name: secure-vault
description: Use when touching crates/obd-vault or any code that stores, loads, passes or displays credentials in Odoo Backup Desktop — vault file format, encryption/KDF parameters, OS keychain (keyring-core), master password flows, SecretField/SecretString handling, logging or IPC of secrets.
---

# Encrypted vault and secret handling

Everything that identifies or opens an Odoo instance (URL, DB, login, password/API key, master
password) and the Google refresh token lives **inside** `vault.bin`. Threat model: protects against
leaked/copied files, backups of the profile and disk theft. It does **not** protect against malware
running as the same user (keychain and DPAPI are readable by same-user processes) — document, don't
over-promise.

## Crypto (crate versions checked 2026-09-16)

| Item | Choice |
|---|---|
| Payload AEAD | XChaCha20-Poly1305 (`chacha20poly1305` 0.11), 24-byte random nonce per write |
| DEK | 32 random bytes (`getrandom` 0.4), held in `Zeroizing` while unlocked |
| Password KDF | Argon2id (`argon2` 0.6), default m = 64 MiB, t = 3, p = 1, 16-byte salt (RFC 9106 2nd option; above OWASP minimum 19 MiB/t=2) |
| Keychain | `keyring-core` 1.0 + `zbus-secret-service-keyring-store` 1.0.1 (`rt-async-io-crypto-rust`; never `rt-tokio-*`, see `tauri2-desktop` skill) on Linux, `windows-native-keyring-store` 1.1 on Windows, `apple-native-keyring-store` 1.0 on macOS |
| Secrets in memory | `secrecy` 0.10 `SecretString` for call arguments, `obd_vault::SecretField` inside the payload, `zeroize` 1.9 |

Not used on purpose: SQLCipher (overkill, OpenSSL pain on Windows), Stronghold (being removed),
`age` (scrypt, not Argon2), JS-exposed keyring plugins.

## File format v1 (`vault.bin`)

```
"OBDVAULT" | version u8 = 1 | flags u8 (bit0 keychain, bit1 password)
m_cost_kib u32 LE | t_cost u32 LE | parallelism u32 LE | salt [16]
dek_nonce [24] | wrapped_dek [48]        (zeros when no password)
payload_nonce [24]
ciphertext                               (AAD = every byte before it)
```

- `wrapped_dek` = XChaCha20-Poly1305(Argon2id(password, salt), AAD `"OBDVAULT-dek-v1"`).
- Header is authenticated through the payload AAD → flipping flags/params = `Corrupted`.
- Unknown version → `UnsupportedVersion`. Bump the version for any layout change and keep a reader
  for old versions.
- Writes are atomic: temp file in the same dir → write → fsync → rename (→ fsync dir on Unix);
  permissions `0600` (dir `0700`).

## Keychain rules

- Store only the DEK as base64 (≈44 chars) under service `io.github.drkpkg.odoo-backup-desktop`, account `vault-dek`.
  Windows Credential Manager blobs max out at 2560 bytes (UTF-16 → ~1280 chars): never store tokens
  or the payload there.
- Linux Secret Service may be missing (i3/Hyprland/headless) or locked; KDE Plasma 6 provides it via
  `ksecretd`. Treat `NoStorageAccess`/`PlatformFailure` as "keychain unavailable" and fall back to the
  master password — **never** fall back to plaintext or to kernel keyutils (not persistent).
- A DEK in the keychain that doesn't open the vault → `KeychainKeyMismatch` → ask for the password.
- Keychain calls block (D-Bus): call through `spawn_blocking`. `init_os_keystore()` once at startup.
- Invariants: at least one unlock method; removing the password requires keychain enabled; disabling
  the keychain requires a password.

## Using secrets in the app

- Commands receive secrets as `Option<String>` and convert immediately to `SecretField`; empty/absent
  means "keep existing". Views expose only `hasSecret` / `hasMasterPassword`.
- Build clients with `secret.to_secret_string()`; call `expose_secret()` only at the last moment
  (HTTP header/body). Never format secrets into URLs, error messages or logs.
- Auto-lock (setting `autoLockMinutes`) drops decrypted data and the DEK; emit `vault-locked`.
- `SecretField` Debug prints `[redacted]`; keep it that way. Avoid `Clone` of decrypted data beyond need.

## Tests to keep green

Create/unlock with keychain, password and both; wrong password; keychain mismatch; tampered header
and ciphertext; save/load roundtrip; password/keychain toggling invariants; `0600` permissions; use
`KdfParams::insecure_for_tests()` and `MemoryKeyStore` in unit tests. The real OS keychain test is
`#[ignore]` and must use a unique account name and clean up.

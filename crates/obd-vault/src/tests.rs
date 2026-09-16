use std::fs;

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use super::*;
use crate::format::HEADER_LEN;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Payload {
    instances: Vec<String>,
    token: SecretField,
}

fn payload(n: usize) -> Payload {
    Payload { instances: (0..n).map(|i| format!("instance-{i}")).collect(), token: SecretField::new("s3cr3t-token") }
}

fn password(value: &str) -> SecretString {
    SecretString::from(value)
}

fn options(use_keychain: bool, master_password: Option<&str>) -> CreateOptions {
    CreateOptions { use_keychain, master_password: master_password.map(password), kdf: KdfParams::insecure_for_tests() }
}

struct Fixture {
    _dir: TempDir,
    file: VaultFile,
    keystore: MemoryKeyStore,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let file = VaultFile::new(dir.path().join("data").join("vault.bin"));
    Fixture { _dir: dir, file, keystore: MemoryKeyStore::new(true) }
}

#[test]
fn keychain_only_roundtrip() {
    let f = fixture();
    let mut vault = f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();
    assert_eq!(vault.info(), VaultInfo { keychain_enabled: true, password_enabled: false });
    assert_eq!(vault.load::<Payload>().unwrap(), payload(1));

    vault.save(&payload(3)).unwrap();
    drop(vault);

    let reopened = f.file.unlock(&f.keystore, UnlockMethod::Keychain).unwrap();
    assert_eq!(reopened.load::<Payload>().unwrap(), payload(3));
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("x"))),
        Err(VaultError::PasswordNotEnabled)
    ));
}

#[test]
fn password_only_roundtrip() {
    let f = fixture();
    let vault = f.file.create(&f.keystore, options(false, Some("correct horse")), &payload(2)).unwrap();
    assert_eq!(vault.info(), VaultInfo { keychain_enabled: false, password_enabled: true });
    assert!(f.keystore.load().unwrap().is_none(), "DEK must not be stored in the keychain");
    drop(vault);

    let reopened = f.file.unlock(&f.keystore, UnlockMethod::Password(password("correct horse"))).unwrap();
    assert_eq!(reopened.load::<Payload>().unwrap(), payload(2));
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainNotEnabled)));
}

#[test]
fn both_methods_open_the_same_vault() {
    let f = fixture();
    f.file.create(&f.keystore, options(true, Some("pw")), &payload(4)).unwrap();
    let by_keychain = f.file.unlock(&f.keystore, UnlockMethod::Keychain).unwrap();
    let by_password = f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))).unwrap();
    assert_eq!(by_keychain.load::<Payload>().unwrap(), by_password.load::<Payload>().unwrap());
    assert_eq!(f.file.info().unwrap(), VaultInfo { keychain_enabled: true, password_enabled: true });
}

#[test]
fn create_validates_options_and_existing_file() {
    let f = fixture();
    assert!(matches!(
        f.file.create(&f.keystore, options(false, None), &payload(0)),
        Err(VaultError::InvalidOptions(_))
    ));
    assert!(matches!(
        f.file.create(&f.keystore, options(false, Some("")), &payload(0)),
        Err(VaultError::InvalidOptions(_))
    ));
    let bad_kdf =
        CreateOptions { kdf: KdfParams { m_cost_kib: 1, t_cost: 0, parallelism: 1 }, ..options(false, Some("pw")) };
    assert!(matches!(f.file.create(&f.keystore, bad_kdf, &payload(0)), Err(VaultError::InvalidOptions(_))));
    assert!(!f.file.exists());

    let unavailable = MemoryKeyStore::new(false);
    assert!(matches!(
        f.file.create(&unavailable, options(true, None), &payload(0)),
        Err(VaultError::KeychainUnavailable)
    ));
    assert!(!f.file.exists());

    f.file.create(&f.keystore, options(true, None), &payload(0)).unwrap();
    assert!(matches!(f.file.create(&f.keystore, options(true, None), &payload(0)), Err(VaultError::AlreadyExists)));
}

#[test]
fn wrong_password_is_reported() {
    let f = fixture();
    f.file.create(&f.keystore, options(false, Some("right")), &payload(1)).unwrap();
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("wrong"))),
        Err(VaultError::WrongPassword)
    ));
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Password(password(""))), Err(VaultError::WrongPassword)));
}

#[test]
fn keychain_missing_mismatch_and_unavailable() {
    let f = fixture();
    f.file.create(&f.keystore, options(true, Some("pw")), &payload(1)).unwrap();

    f.keystore.store(&[7u8; 32]).unwrap();
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainKeyMismatch)));

    f.keystore.store(&[7u8; 5]).unwrap();
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainKeyMismatch)));

    f.keystore.delete().unwrap();
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainKeyMissing)));

    let unavailable = MemoryKeyStore::new(false);
    assert!(matches!(f.file.unlock(&unavailable, UnlockMethod::Keychain), Err(VaultError::KeychainUnavailable)));

    // The master password still works.
    f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))).unwrap();
}

#[test]
fn tampered_header_is_detected() {
    let f = fixture();
    f.file.create(&f.keystore, options(true, Some("pw")), &payload(1)).unwrap();
    let original = fs::read(f.file.path()).unwrap();

    // Flip a byte of the payload nonce (part of the AAD, not used by the password unwrap).
    let mut tampered = original.clone();
    tampered[HEADER_LEN - 1] ^= 0x01;
    fs::write(f.file.path(), &tampered).unwrap();
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))),
        Err(VaultError::Corrupted(_))
    ));
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainKeyMismatch)));

    // Clearing the keychain flag still authenticates as corrupted with the password.
    let mut flags = original.clone();
    flags[9] &= !0b01;
    fs::write(f.file.path(), &flags).unwrap();
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))),
        Err(VaultError::Corrupted(_))
    ));
}

#[test]
fn tampered_ciphertext_is_detected() {
    let f = fixture();
    let vault = f.file.create(&f.keystore, options(true, Some("pw")), &payload(1)).unwrap();
    let mut bytes = fs::read(f.file.path()).unwrap();
    let last = bytes.len() - 1;
    bytes[HEADER_LEN + 3] ^= 0x80;
    bytes[last] ^= 0x01;
    fs::write(f.file.path(), &bytes).unwrap();

    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))),
        Err(VaultError::Corrupted(_))
    ));
    assert!(matches!(vault.load::<Payload>(), Err(VaultError::Corrupted(_))));

    bytes.truncate(HEADER_LEN + 4);
    fs::write(f.file.path(), &bytes).unwrap();
    assert!(matches!(f.file.info(), Err(VaultError::Corrupted(_))));
}

#[test]
fn unsupported_version_and_foreign_files() {
    let f = fixture();
    f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();
    let mut bytes = fs::read(f.file.path()).unwrap();
    bytes[8] = 2;
    fs::write(f.file.path(), &bytes).unwrap();
    assert!(matches!(f.file.info(), Err(VaultError::UnsupportedVersion(2))));
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::UnsupportedVersion(2))));

    fs::write(f.file.path(), b"{\"not\": \"a vault\"}").unwrap();
    assert!(matches!(f.file.info(), Err(VaultError::Corrupted(_))));
}

#[test]
fn missing_file_is_not_found() {
    let f = fixture();
    assert!(!f.file.exists());
    assert!(matches!(f.file.info(), Err(VaultError::NotFound)));
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::NotFound)));
}

#[test]
fn info_reads_header_without_secrets() {
    let f = fixture();
    f.file.create(&MemoryKeyStore::new(true), options(true, None), &payload(1)).unwrap();
    // A different (empty) keystore: info must not need the key.
    assert_eq!(f.file.info().unwrap(), VaultInfo { keychain_enabled: true, password_enabled: false });

    let bytes = fs::read(f.file.path()).unwrap();
    let haystack = String::from_utf8_lossy(&bytes);
    assert!(!haystack.contains("s3cr3t-token"));
    assert!(!haystack.contains("instance-0"));
}

#[test]
fn master_password_rules() {
    let f = fixture();
    let mut vault = f.file.create(&f.keystore, options(false, Some("old")), &payload(1)).unwrap();
    let kdf = KdfParams::insecure_for_tests();

    // Cannot remove the only unlock method.
    assert!(matches!(vault.set_master_password(None, kdf), Err(VaultError::InvalidOptions(_))));
    assert!(matches!(vault.set_master_password(Some(password("")), kdf), Err(VaultError::InvalidOptions(_))));

    vault.set_master_password(Some(password("new")), kdf).unwrap();
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("old"))),
        Err(VaultError::WrongPassword)
    ));
    f.file.unlock(&f.keystore, UnlockMethod::Password(password("new"))).unwrap();

    vault.set_keychain_enabled(&f.keystore, true).unwrap();
    vault.set_master_password(None, kdf).unwrap();
    assert_eq!(f.file.info().unwrap(), VaultInfo { keychain_enabled: true, password_enabled: false });
    assert!(matches!(
        f.file.unlock(&f.keystore, UnlockMethod::Password(password("new"))),
        Err(VaultError::PasswordNotEnabled)
    ));
    let reopened = f.file.unlock(&f.keystore, UnlockMethod::Keychain).unwrap();
    assert_eq!(reopened.load::<Payload>().unwrap(), payload(1));
}

#[test]
fn keychain_toggle_rules() {
    let f = fixture();
    let mut vault = f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();

    // Cannot disable the only unlock method.
    assert!(matches!(vault.set_keychain_enabled(&f.keystore, false), Err(VaultError::InvalidOptions(_))));

    vault.set_master_password(Some(password("pw")), KdfParams::insecure_for_tests()).unwrap();
    vault.set_keychain_enabled(&f.keystore, false).unwrap();
    assert!(f.keystore.load().unwrap().is_none());
    assert_eq!(vault.info(), VaultInfo { keychain_enabled: false, password_enabled: true });
    assert!(matches!(f.file.unlock(&f.keystore, UnlockMethod::Keychain), Err(VaultError::KeychainNotEnabled)));

    // Enabling on an unavailable keychain changes nothing.
    assert!(matches!(
        vault.set_keychain_enabled(&MemoryKeyStore::new(false), true),
        Err(VaultError::KeychainUnavailable)
    ));
    assert_eq!(f.file.info().unwrap(), VaultInfo { keychain_enabled: false, password_enabled: true });

    vault.set_keychain_enabled(&f.keystore, true).unwrap();
    let reopened = f.file.unlock(&f.keystore, UnlockMethod::Keychain).unwrap();
    assert_eq!(reopened.load::<Payload>().unwrap(), payload(1));

    // Re-enabling repairs a keychain entry deleted outside the app.
    f.keystore.delete().unwrap();
    vault.set_keychain_enabled(&f.keystore, true).unwrap();
    f.file.unlock(&f.keystore, UnlockMethod::Keychain).unwrap();
}

#[test]
fn save_replaces_file_atomically_without_leftovers() {
    let f = fixture();
    let mut vault = f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();
    for n in 0..5 {
        vault.save(&payload(n)).unwrap();
    }
    let dir = f.file.path().parent().unwrap();
    let names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names, vec!["vault.bin".to_string()]);
    assert_eq!(vault.load::<Payload>().unwrap(), payload(4));
}

#[cfg(unix)]
#[test]
fn vault_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let f = fixture();
    let mut vault = f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();
    let mode = fs::metadata(f.file.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    fs::set_permissions(f.file.path(), fs::Permissions::from_mode(0o644)).unwrap();
    vault.save(&payload(2)).unwrap();
    let mode = fs::metadata(f.file.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "save must restore private permissions");
}

#[test]
fn serde_errors_do_not_leak_payload_values() {
    #[derive(Debug, Deserialize)]
    struct Wrong {
        #[allow(dead_code)]
        token: u32,
    }
    let f = fixture();
    let vault = f.file.create(&f.keystore, options(true, None), &payload(1)).unwrap();
    let err = vault.load::<Wrong>().unwrap_err();
    assert!(matches!(err, VaultError::Serde(_)));
    assert!(!err.to_string().contains("s3cr3t-token"), "{err}");
}

#[test]
fn secret_field_redacts_debug_and_roundtrips() {
    let secret = SecretField::new("hunter2");
    assert_eq!(format!("{secret:?}"), "SecretField([redacted])");
    assert!(!format!("{:?}", payload(1)).contains("s3cr3t-token"));

    let json = serde_json::to_string(&secret).unwrap();
    assert_eq!(json, "\"hunter2\"");
    let back: SecretField = serde_json::from_str(&json).unwrap();
    assert_eq!(back.expose(), "hunter2");
    assert!(!back.is_empty());
    assert!(SecretField::default().is_empty());

    use secrecy::ExposeSecret;
    assert_eq!(back.to_secret_string().expose_secret(), "hunter2");
}

#[test]
fn default_kdf_params_are_accepted() {
    let f = fixture();
    let opts = CreateOptions { use_keychain: false, master_password: Some(password("pw")), kdf: KdfParams::default() };
    let vault = f.file.create(&f.keystore, opts, &payload(1)).unwrap();
    assert!(vault.info().password_enabled);
    f.file.unlock(&f.keystore, UnlockMethod::Password(password("pw"))).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn works_inside_spawn_blocking() {
    let f = fixture();
    let file = f.file.clone();
    let keystore = std::sync::Arc::new(f.keystore);
    let ks = keystore.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        let mut vault = file.create(ks.as_ref(), options(true, Some("pw")), &payload(1))?;
        vault.save(&payload(2))?;
        let reopened = file.unlock(ks.as_ref(), UnlockMethod::Keychain)?;
        reopened.load::<Payload>()
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(loaded, payload(2));
}

/// Real OS keychain round trip. Run with `cargo test -p obd-vault -- --ignored`.
#[test]
#[ignore = "touches the real OS keychain"]
fn os_keychain_roundtrip() {
    run_os_keychain_roundtrip();
}

/// Same as above, from a `spawn_blocking` thread of a multi-thread tokio runtime
/// (how the app calls the vault).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "touches the real OS keychain"]
async fn os_keychain_roundtrip_inside_spawn_blocking() {
    tokio::task::spawn_blocking(run_os_keychain_roundtrip).await.unwrap();
}

fn run_os_keychain_roundtrip() {
    init_os_keystore().expect("OS keychain must be available for this test");
    let account = format!("vault-test-{}", hex_suffix());
    let keystore = OsKeyStore::new("io.github.drkpkg.odoo-backup-desktop.tests", account);
    assert!(keystore.is_available());
    assert!(keystore.load().unwrap().is_none());

    let dir = tempfile::tempdir().unwrap();
    let file = VaultFile::new(dir.path().join("vault.bin"));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut vault = file.create(&keystore, options(true, None), &payload(1)).unwrap();
        vault.save(&payload(2)).unwrap();
        let stored = keystore.load().unwrap().expect("DEK stored");
        assert_eq!(stored.len(), 32);

        let reopened = file.unlock(&keystore, UnlockMethod::Keychain).unwrap();
        assert_eq!(reopened.load::<Payload>().unwrap(), payload(2));
    }));

    keystore.delete().unwrap();
    assert!(keystore.load().unwrap().is_none());
    keystore.delete().unwrap(); // deleting a missing entry is not an error
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn hex_suffix() -> String {
    crate::crypto::random_array::<6>().unwrap().iter().map(|b| format!("{b:02x}")).collect()
}

mod common;

use appex_odoo::{OdooError, validate_backup_zip};
use common::{ZipSpec, backup_zip};
use zip::CompressionMethod;

fn write(dir: &tempfile::TempDir, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join("backup.zip");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn invalid_message(result: appex_odoo::Result<appex_odoo::BackupManifest>) -> String {
    match result {
        Err(OdooError::InvalidBackup(message)) => message,
        other => panic!("expected InvalidBackup, got {other:?}"),
    }
}

#[test]
fn accepts_a_complete_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &backup_zip(&ZipSpec::default()));
    let manifest = validate_backup_zip(&path, Some("cliente1")).unwrap();
    assert_eq!(manifest.db_name, "cliente1");
    assert_eq!(manifest.odoo_version, "17.0-20240101");
    assert_eq!(manifest.major_version, "17.0");
    assert_eq!(manifest.pg_version, "16.4");
    assert_eq!(manifest.module_count, 2);
    assert_eq!(manifest.filestore_files, 2);
    // Without expectation any db name is fine.
    assert!(validate_backup_zip(&path, None).is_ok());
}

#[test]
fn rejects_missing_dump() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &backup_zip(&ZipSpec { include_dump: false, ..ZipSpec::default() }));
    assert!(invalid_message(validate_backup_zip(&path, None)).contains("dump.sql is missing"));
}

#[test]
fn rejects_truncated_dump() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &backup_zip(&ZipSpec { dump_footer: false, ..ZipSpec::default() }));
    assert!(invalid_message(validate_backup_zip(&path, None)).contains("incomplete"));
}

#[test]
fn rejects_missing_manifest_and_wrong_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &backup_zip(&ZipSpec { include_manifest: false, ..ZipSpec::default() }));
    assert!(invalid_message(validate_backup_zip(&path, None)).contains("manifest.json is missing"));

    let path = write(&dir, &backup_zip(&ZipSpec::default()));
    assert!(invalid_message(validate_backup_zip(&path, Some("other"))).contains("expected \"other\""));
}

#[test]
fn rejects_corrupted_crc() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = backup_zip(&ZipSpec { compression: CompressionMethod::Stored, ..ZipSpec::default() });
    let marker = b"MARKER-DATA";
    let pos = bytes.windows(marker.len()).position(|w| w == marker).expect("stored data present");
    bytes[pos] = b'X';
    let path = write(&dir, &bytes);
    assert!(invalid_message(validate_backup_zip(&path, None)).contains("corrupted entry dump.sql"));
}

#[test]
fn rejects_non_zip_and_truncated_archives() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, b"<html>Database backup error</html>");
    assert!(invalid_message(validate_backup_zip(&path, None)).contains("not a valid zip"));

    let bytes = backup_zip(&ZipSpec::default());
    let path = write(&dir, &bytes[..bytes.len() / 2]);
    assert!(validate_backup_zip(&path, None).is_err());
}

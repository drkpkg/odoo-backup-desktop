use std::fs::{self, File};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use appex_storage::local::{LocalFolderAdapter, slugify};
use appex_storage::{RetentionPolicy, StorageAdapter, StorageError, TargetId, TargetSpec, UploadMeta, apply_retention};
use chrono::Utc;
use tokio_util::sync::CancellationToken;

fn spec(name: &str) -> TargetSpec {
    TargetSpec { instance_id: "inst-1".into(), instance_name: name.into() }
}

fn meta() -> UploadMeta {
    UploadMeta {
        instance_id: "inst-1".into(),
        database: "cliente1".into(),
        sha256: "abc".into(),
        created_at: Utc::now(),
    }
}

fn write_with_mtime(path: &std::path::Path, content: &[u8], age: Duration) {
    fs::write(path, content).unwrap();
    File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - age).unwrap();
}

type Calls = Arc<Mutex<Vec<(u64, u64)>>>;

fn recorder() -> (appex_storage::UploadProgressFn, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    (Arc::new(move |sent, total| sink.lock().unwrap().push((sent, total))), calls)
}

#[test]
fn slugify_examples() {
    assert_eq!(slugify("Cliente Uno S.A."), "cliente-uno-s-a");
    assert_eq!(slugify("Compañía Ñandú"), "compania-nandu");
    assert_eq!(slugify("odoo_prod 2026"), "odoo_prod-2026");
    assert_eq!(slugify("--Prod  --  Norte--"), "prod-norte");
    assert_eq!(slugify("a__b"), "a-b");
    assert_eq!(slugify("  --  "), "instance");
    assert_eq!(slugify("日本"), "instance");
}

#[tokio::test]
async fn ensure_target_creates_absolute_instance_dir() {
    let root = tempfile::tempdir().unwrap();
    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("Cliente Uno")).await.unwrap();
    let dir = std::path::PathBuf::from(&target.0);
    assert!(dir.is_absolute());
    assert!(dir.is_dir());
    assert_eq!(dir, root.path().join("cliente-uno"));
    // Idempotent.
    assert_eq!(adapter.ensure_target(&spec("Cliente Uno")).await.unwrap(), target);
}

#[tokio::test]
async fn upload_copies_file_with_progress() {
    let root = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();
    let source = source_dir.path().join("cliente1_2026-09-16_10-00-00.zip");
    let content: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&source, &content).unwrap();

    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("Cliente Uno")).await.unwrap();
    let (progress, calls) = recorder();
    let object = adapter.upload(&target, &source, &meta(), progress, CancellationToken::new()).await.unwrap();

    let dest = root.path().join("cliente-uno/cliente1_2026-09-16_10-00-00.zip");
    assert_eq!(fs::read(&dest).unwrap(), content);
    assert!(!root.path().join("cliente-uno/cliente1_2026-09-16_10-00-00.zip.part").exists());
    assert_eq!(object.name, "cliente1_2026-09-16_10-00-00.zip");
    assert_eq!(object.size, Some(content.len() as u64));
    assert_eq!(object.instance_id.as_deref(), Some("inst-1"));
    assert_eq!(object.id, dest.to_str().unwrap());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.last(), Some(&(content.len() as u64, content.len() as u64)));
    assert!(calls.len() > 1);
}

#[tokio::test]
async fn upload_of_a_file_already_in_target_does_not_copy() {
    let root = tempfile::tempdir().unwrap();
    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("Cliente Uno")).await.unwrap();
    let file = std::path::PathBuf::from(&target.0).join("backup.zip");
    fs::write(&file, b"PK-data").unwrap();

    let (progress, _) = recorder();
    let object = adapter.upload(&target, &file, &meta(), progress, CancellationToken::new()).await.unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"PK-data");
    assert_eq!(object.size, Some(7));
}

#[tokio::test]
async fn cancelled_upload_leaves_nothing_behind() {
    let root = tempfile::tempdir().unwrap();
    let source_dir = tempfile::tempdir().unwrap();
    let source = source_dir.path().join("big.zip");
    fs::write(&source, vec![7u8; 4 * 1024 * 1024]).unwrap();

    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("x")).await.unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (progress, _) = recorder();
    let err = adapter.upload(&target, &source, &meta(), progress, cancel).await.unwrap_err();
    assert!(matches!(err, StorageError::Cancelled));
    let leftovers: Vec<_> = fs::read_dir(&target.0).unwrap().collect();
    assert!(leftovers.is_empty());
}

#[tokio::test]
async fn list_returns_only_zip_files_newest_first() {
    let root = tempfile::tempdir().unwrap();
    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("x")).await.unwrap();
    let dir = std::path::PathBuf::from(&target.0);

    write_with_mtime(&dir.join("old.zip"), b"old", Duration::from_secs(3 * 86_400));
    write_with_mtime(&dir.join("new.ZIP"), b"newer", Duration::from_secs(60));
    fs::write(dir.join("partial.zip.part"), b"x").unwrap();
    fs::write(dir.join("notes.txt"), b"x").unwrap();
    fs::create_dir(dir.join("folder.zip")).unwrap();

    let objects = adapter.list_backups(&target).await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["new.ZIP", "old.zip"]);
    assert_eq!(objects[0].size, Some(5));
    assert!(objects[0].created_at > objects[1].created_at);

    let missing = TargetId(root.path().join("does-not-exist").to_str().unwrap().to_owned());
    assert!(adapter.list_backups(&missing).await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_and_retention() {
    let root = tempfile::tempdir().unwrap();
    let adapter = LocalFolderAdapter::new(root.path());
    let target = adapter.ensure_target(&spec("x")).await.unwrap();
    let dir = std::path::PathBuf::from(&target.0);
    write_with_mtime(&dir.join("a.zip"), b"a", Duration::from_secs(5 * 86_400));
    write_with_mtime(&dir.join("b.zip"), b"b", Duration::from_secs(2 * 86_400));
    write_with_mtime(&dir.join("c.zip"), b"c", Duration::from_secs(60));

    let policy = RetentionPolicy { keep_last: Some(2), max_age_days: None };
    let deleted = apply_retention(&adapter, &target, &policy).await.unwrap();
    assert_eq!(deleted.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), vec!["a.zip"]);
    assert!(!dir.join("a.zip").exists());

    let policy = RetentionPolicy { keep_last: None, max_age_days: Some(1) };
    let deleted = apply_retention(&adapter, &target, &policy).await.unwrap();
    assert_eq!(deleted.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), vec!["b.zip"]);
    assert!(dir.join("c.zip").exists());

    let remaining = adapter.list_backups(&target).await.unwrap();
    adapter.delete(&remaining[0]).await.unwrap();
    assert!(matches!(adapter.delete(&remaining[0]).await, Err(StorageError::NotFound(_))));
}

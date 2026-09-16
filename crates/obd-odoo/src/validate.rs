//! Integrity checks for Odoo backup zips (`dump.sql`, `manifest.json`, `filestore/`).

use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{OdooError, Result};

/// Last line written by `pg_dump` in plain format when it finishes successfully.
const PG_DUMP_FOOTER: &str = "-- PostgreSQL database dump complete";
/// Bytes kept from the end of `dump.sql` to look for the footer.
const TAIL_BYTES: usize = 4096;
/// `manifest.json` is tiny; refuse absurd sizes.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub db_name: String,
    /// `version` from manifest.json, e.g. "17.0-20240101".
    pub odoo_version: String,
    pub major_version: String,
    pub pg_version: String,
    pub module_count: usize,
    pub filestore_files: u64,
}

/// Blocking: verifies every entry CRC, `dump.sql` presence and pg_dump footer
/// (`-- PostgreSQL database dump complete`), parses `manifest.json` and, when
/// `expected_db` is given, checks `db_name`. Run it with `spawn_blocking`.
pub fn validate_backup_zip(path: &Path, expected_db: Option<&str>) -> Result<BackupManifest> {
    validate_backup_zip_cancellable(path, expected_db, &AtomicBool::new(false))
}

/// Same as [`validate_backup_zip`], aborting with `Cancelled` soon after `cancel` is set.
pub fn validate_backup_zip_cancellable(
    path: &Path,
    expected_db: Option<&str>,
    cancel: &AtomicBool,
) -> Result<BackupManifest> {
    let invalid = |message: String| OdooError::InvalidBackup(message);

    let file = File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|err| invalid(format!("not a valid zip archive: {err}")))?;

    let mut dump_tail: Option<Vec<u8>> = None;
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut filestore_files = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];

    for index in 0..archive.len() {
        let mut entry =
            archive.by_index(index).map_err(|err| invalid(format!("unreadable zip entry #{index}: {err}")))?;
        let name = entry.name().to_owned();
        if entry.is_dir() {
            continue;
        }
        let mut tail = TailWriter::default();
        let mut manifest = Vec::new();
        let is_dump = name == "dump.sql";
        let is_manifest = name == "manifest.json";
        if is_manifest && entry.size() > MAX_MANIFEST_BYTES {
            return Err(invalid("manifest.json is too large".into()));
        }

        // Reading an entry to the end makes the zip crate verify its CRC-32.
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(OdooError::Cancelled);
            }
            let read = match entry.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(invalid(format!("corrupted entry {name}: {err}"))),
            };
            if is_dump {
                tail.write_all(&buffer[..read])?;
            } else if is_manifest {
                manifest.extend_from_slice(&buffer[..read]);
            }
        }

        if is_dump {
            dump_tail = Some(tail.into_inner());
        } else if is_manifest {
            manifest_bytes = Some(manifest);
        } else if name.starts_with("filestore/") {
            filestore_files += 1;
        }
    }

    let tail = dump_tail.ok_or_else(|| invalid("dump.sql is missing".into()))?;
    if !String::from_utf8_lossy(&tail).contains(PG_DUMP_FOOTER) {
        return Err(invalid("dump.sql is incomplete (pg_dump footer not found)".into()));
    }

    let manifest_bytes = manifest_bytes.ok_or_else(|| invalid("manifest.json is missing".into()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|err| invalid(format!("manifest.json is not valid JSON: {err}")))?;
    let db_name = manifest
        .get("db_name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("manifest.json has no db_name".into()))?
        .to_owned();

    if let Some(expected) = expected_db
        && expected != db_name
    {
        return Err(invalid(format!("backup belongs to database {db_name:?}, expected {expected:?}")));
    }

    Ok(BackupManifest {
        db_name,
        odoo_version: value_to_string(manifest.get("version")),
        major_version: value_to_string(manifest.get("major_version")),
        pg_version: value_to_string(manifest.get("pg_version")),
        module_count: manifest.get("modules").and_then(Value::as_object).map_or(0, |m| m.len()),
        filestore_files,
    })
}

fn value_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Keeps only the last [`TAIL_BYTES`] bytes written.
#[derive(Default)]
struct TailWriter {
    buf: Vec<u8>,
}

impl TailWriter {
    fn into_inner(self) -> Vec<u8> {
        self.buf
    }
}

impl Write for TailWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.len() >= TAIL_BYTES {
            self.buf.clear();
            self.buf.extend_from_slice(&data[data.len() - TAIL_BYTES..]);
        } else {
            self.buf.extend_from_slice(data);
            if self.buf.len() > TAIL_BYTES {
                let excess = self.buf.len() - TAIL_BYTES;
                self.buf.drain(..excess);
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

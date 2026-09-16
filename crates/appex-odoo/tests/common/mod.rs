//! Helpers shared by the wiremock-based tests.
#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use appex_odoo::{BackupPhase, ProgressFn};
use sha2::{Digest, Sha256};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

pub struct ZipSpec<'a> {
    pub db_name: &'a str,
    pub dump_footer: bool,
    pub include_dump: bool,
    pub include_manifest: bool,
    pub filestore_files: usize,
    pub compression: CompressionMethod,
}

impl Default for ZipSpec<'_> {
    fn default() -> Self {
        Self {
            db_name: "cliente1",
            dump_footer: true,
            include_dump: true,
            include_manifest: true,
            filestore_files: 2,
            compression: CompressionMethod::Deflated,
        }
    }
}

/// Builds a zip shaped like Odoo's `dump_db` output.
pub fn backup_zip(spec: &ZipSpec<'_>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(spec.compression);
    if spec.include_dump {
        writer.start_file("dump.sql", options).unwrap();
        writer.write_all(b"--\n-- PostgreSQL database dump\n--\nSET statement_timeout = 0;\n").unwrap();
        writer.write_all("INSERT INTO t VALUES (1, 'MARKER-DATA-0123456789');\n".repeat(200).as_bytes()).unwrap();
        if spec.dump_footer {
            writer.write_all(b"--\n-- PostgreSQL database dump complete\n--\n\n").unwrap();
        }
    }
    if spec.include_manifest {
        writer.start_file("manifest.json", options).unwrap();
        let manifest = serde_json::json!({
            "odoo_dump": "1",
            "db_name": spec.db_name,
            "version": "17.0-20240101",
            "version_info": [17, 0, 0, "final", 0, ""],
            "major_version": "17.0",
            "pg_version": "16.4",
            "modules": {"base": "17.0.1.3", "web": "17.0.1.0"},
        });
        writer.write_all(manifest.to_string().as_bytes()).unwrap();
    }
    for i in 0..spec.filestore_files {
        writer.start_file(format!("filestore/ab/abcdef{i:034}"), options).unwrap();
        writer.write_all(format!("attachment {i}").as_bytes()).unwrap();
    }
    writer.add_directory("filestore/checklist/", options).unwrap();
    writer.finish().unwrap().into_inner()
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Collects progress events.
pub fn recorder() -> (ProgressFn, Arc<Mutex<Vec<BackupPhase>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (Arc::new(move |phase| sink.lock().unwrap().push(phase)), events)
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

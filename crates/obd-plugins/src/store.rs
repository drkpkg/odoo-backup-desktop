//! On-disk stores (all JSON, atomic writes, created lazily):
//! - `plugins.json`: [`PluginConfig`] (disabled plugins, developer mode, dev folders).
//! - `plugin-settings.json`: non-secret settings per plugin.
//! - `plugin-data/<id>.json`: key/value storage per plugin (1 MiB limit).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::manifest::is_valid_plugin_id;
use crate::{PluginError, Result};

pub const STORAGE_LIMIT_BYTES: usize = 1024 * 1024;
const MAX_KEY_LEN: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginConfig {
    pub disabled: Vec<String>,
    pub developer_mode: bool,
    pub dev_plugin_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginConfigStore {
    file: PathBuf,
}

impl PluginConfigStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// Missing or unreadable file → defaults (logged).
    pub fn load(&self) -> PluginConfig {
        let bytes = match std::fs::read(&self.file) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return PluginConfig::default(),
            Err(err) => {
                tracing::warn!(path = %self.file.display(), error = %err, "could not read plugin config, using defaults");
                return PluginConfig::default();
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!(path = %self.file.display(), error = %err, "plugin config is invalid, using defaults");
            PluginConfig::default()
        })
    }

    /// Deduplicates `disabled` and `dev_plugin_paths` before writing.
    pub fn save(&self, config: &PluginConfig) -> Result<()> {
        let config = PluginConfig {
            disabled: dedup(&config.disabled),
            developer_mode: config.developer_mode,
            dev_plugin_paths: dedup(&config.dev_plugin_paths),
        };
        let json = serde_json::to_vec_pretty(&config).map_err(std::io::Error::other)?;
        write_atomic(&self.file, &json, false)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PluginSettingsStore {
    file: PathBuf,
}

impl PluginSettingsStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// Stored non-secret values for a plugin (empty when none).
    pub fn get(&self, plugin_id: &str) -> Result<Map<String, Value>> {
        check_plugin_id(plugin_id)?;
        let all = self.read_all()?;
        match all.get(plugin_id) {
            None => Ok(Map::new()),
            Some(Value::Object(values)) => Ok(values.clone()),
            Some(_) => Err(corrupted(&self.file, format!("settings of {plugin_id:?} are not an object"))),
        }
    }

    /// Replaces the plugin's values (an empty map removes the plugin entry).
    pub fn set(&self, plugin_id: &str, values: &Map<String, Value>) -> Result<()> {
        check_plugin_id(plugin_id)?;
        let mut all = self.read_all()?;
        if values.is_empty() {
            all.remove(plugin_id);
        } else {
            all.insert(plugin_id.to_owned(), Value::Object(values.clone()));
        }
        let json = serde_json::to_vec_pretty(&Value::Object(all)).map_err(std::io::Error::other)?;
        write_atomic(&self.file, &json, true)?;
        Ok(())
    }

    fn read_all(&self) -> Result<Map<String, Value>> {
        read_json_object(&self.file)
    }
}

#[derive(Debug, Clone)]
pub struct PluginStorage {
    dir: PathBuf,
}

impl PluginStorage {
    /// `dir` is `<data>/plugin-data`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Key must match `^[A-Za-z0-9._-]{1,128}$`.
    pub fn get(&self, plugin_id: &str, key: &str) -> Result<Option<Value>> {
        check_plugin_id(plugin_id)?;
        check_key(key)?;
        Ok(read_json_object(&self.file(plugin_id))?.get(key).cloned())
    }

    /// `None` removes the key. Fails with `StorageLimit` when the plugin's file would exceed
    /// [`STORAGE_LIMIT_BYTES`].
    ///
    /// Removing the last key deletes the file. Storing JSON `null` is treated as removal.
    pub fn set(&self, plugin_id: &str, key: &str, value: Option<Value>) -> Result<()> {
        check_plugin_id(plugin_id)?;
        check_key(key)?;
        let file = self.file(plugin_id);
        let mut data = read_json_object(&file)?;
        match value {
            None | Some(Value::Null) => {
                if data.remove(key).is_none() {
                    return Ok(());
                }
            }
            Some(value) => {
                data.insert(key.to_owned(), value);
            }
        }
        if data.is_empty() {
            return match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.into()),
            };
        }
        let json = serde_json::to_vec(&Value::Object(data)).map_err(std::io::Error::other)?;
        if json.len() > STORAGE_LIMIT_BYTES {
            return Err(PluginError::StorageLimit { limit: STORAGE_LIMIT_BYTES });
        }
        write_atomic(&file, &json, true)?;
        Ok(())
    }

    fn file(&self, plugin_id: &str) -> PathBuf {
        self.dir.join(format!("{plugin_id}.json"))
    }
}

fn check_plugin_id(plugin_id: &str) -> Result<()> {
    if is_valid_plugin_id(plugin_id) { Ok(()) } else { Err(PluginError::InvalidId(plugin_id.to_owned())) }
}

/// `^[A-Za-z0-9._-]{1,128}$`.
pub fn is_valid_storage_key(key: &str) -> bool {
    (1..=MAX_KEY_LEN).contains(&key.len())
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn check_key(key: &str) -> Result<()> {
    if is_valid_storage_key(key) { Ok(()) } else { Err(PluginError::StorageKeyInvalid(key.to_owned())) }
}

fn corrupted(path: &Path, message: String) -> PluginError {
    PluginError::CorruptedStore { path: path.display().to_string(), message }
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(err) => return Err(err.into()),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(corrupted(path, "expected a JSON object".into())),
        Err(err) => Err(corrupted(path, format!("invalid JSON at line {}, column {}", err.line(), err.column()))),
    }
}

fn dedup(items: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.iter().filter(|item| seen.insert(item.as_str())).cloned().collect()
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames it over `path`.
/// `private` files get mode `0600` on Unix.
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("store");
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = parent.join(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()));

    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if private { 0o600 } else { 0o644 });
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn config_roundtrip_dedup_and_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        let store = PluginConfigStore::new(dir.path().join("nested/plugins.json"));
        assert_eq!(store.load(), PluginConfig::default());

        let config = PluginConfig {
            disabled: vec!["a-b".into(), "c-d".into(), "a-b".into()],
            developer_mode: true,
            dev_plugin_paths: vec!["/x".into(), "/x".into(), "/y".into()],
        };
        store.save(&config).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.disabled, ["a-b", "c-d"]);
        assert_eq!(loaded.dev_plugin_paths, ["/x", "/y"]);
        assert!(loaded.developer_mode);

        let raw: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("nested/plugins.json")).unwrap()).unwrap();
        assert_eq!(raw["developerMode"], json!(true));

        std::fs::write(dir.path().join("nested/plugins.json"), b"{ broken").unwrap();
        assert_eq!(store.load(), PluginConfig::default());

        // Missing keys use defaults.
        std::fs::write(dir.path().join("nested/plugins.json"), br#"{"developerMode": true}"#).unwrap();
        assert_eq!(store.load(), PluginConfig { developer_mode: true, ..PluginConfig::default() });
    }

    #[test]
    fn settings_store_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plugin-settings.json");
        let store = PluginSettingsStore::new(&file);

        assert!(store.get("hello-obd").unwrap().is_empty());
        let values = json!({"endpoint": "https://api.example.com", "retries": 3}).as_object().unwrap().clone();
        store.set("hello-obd", &values).unwrap();
        store.set("other-plugin", &json!({"x": true}).as_object().unwrap().clone()).unwrap();
        assert_eq!(store.get("hello-obd").unwrap(), values);
        assert_eq!(store.get("other-plugin").unwrap()["x"], json!(true));

        store.set("hello-obd", &Map::new()).unwrap();
        assert!(store.get("hello-obd").unwrap().is_empty());
        assert_eq!(store.get("other-plugin").unwrap()["x"], json!(true));

        assert!(matches!(store.get("Bad_Id"), Err(PluginError::InvalidId(_))));
        assert!(matches!(store.set("../x", &Map::new()), Err(PluginError::InvalidId(_))));

        std::fs::write(&file, br#"{"hello-obd": 5}"#).unwrap();
        assert_eq!(store.get("hello-obd").unwrap_err().code(), "plugin_store_corrupted");
        std::fs::write(&file, b"not json").unwrap();
        assert!(matches!(store.get("other-plugin"), Err(PluginError::CorruptedStore { .. })));
        assert!(matches!(store.set("other-plugin", &Map::new()), Err(PluginError::CorruptedStore { .. })));
    }

    #[test]
    fn storage_get_set_remove() {
        let dir = tempfile::tempdir().unwrap();
        let storage = PluginStorage::new(dir.path().join("plugin-data"));
        assert_eq!(storage.get("hello-obd", "counter").unwrap(), None);

        storage.set("hello-obd", "counter", Some(json!(1))).unwrap();
        storage.set("hello-obd", "prefs.theme", Some(json!({"dark": true}))).unwrap();
        assert_eq!(storage.get("hello-obd", "counter").unwrap(), Some(json!(1)));
        assert_eq!(storage.get("hello-obd", "prefs.theme").unwrap(), Some(json!({"dark": true})));
        assert_eq!(storage.get("other-plugin", "counter").unwrap(), None);

        storage.set("hello-obd", "counter", Some(Value::Null)).unwrap();
        assert_eq!(storage.get("hello-obd", "counter").unwrap(), None);
        storage.set("hello-obd", "missing", None).unwrap();
        storage.set("hello-obd", "prefs.theme", None).unwrap();
        assert!(!dir.path().join("plugin-data/hello-obd.json").exists());
    }

    #[test]
    fn storage_validates_ids_keys_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = PluginStorage::new(dir.path());
        for bad in ["", "with space", "slash/key", &"k".repeat(129), "ñ"] {
            assert_eq!(storage.get("hello-obd", bad).unwrap_err().code(), "plugin_storage_key_invalid", "{bad:?}");
        }
        assert!(storage.get("hello-obd", &"k".repeat(128)).is_ok());
        assert!(matches!(storage.get("../../etc", "k"), Err(PluginError::InvalidId(_))));
        assert!(matches!(storage.set("_sdk", "k", Some(json!(1))), Err(PluginError::InvalidId(_))));

        storage.set("hello-obd", "small", Some(json!("ok"))).unwrap();
        let big = "x".repeat(STORAGE_LIMIT_BYTES);
        let err = storage.set("hello-obd", "big", Some(json!(big))).unwrap_err();
        assert_eq!(err.code(), "plugin_storage_limit");
        assert_eq!(storage.get("hello-obd", "big").unwrap(), None);
        assert_eq!(storage.get("hello-obd", "small").unwrap(), Some(json!("ok")));

        std::fs::write(dir.path().join("hello-obd.json"), b"[1,2]").unwrap();
        assert!(matches!(storage.get("hello-obd", "small"), Err(PluginError::CorruptedStore { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let settings = PluginSettingsStore::new(dir.path().join("plugin-settings.json"));
        settings.set("hello-obd", &json!({"a": 1}).as_object().unwrap().clone()).unwrap();
        let storage = PluginStorage::new(dir.path().join("plugin-data"));
        storage.set("hello-obd", "k", Some(json!(1))).unwrap();

        for file in [dir.path().join("plugin-settings.json"), dir.path().join("plugin-data/hello-obd.json")] {
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", file.display());
        }
        // No temporary files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }
}

//! Plugin folder scanning.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::MANIFEST_FILE;
use crate::manifest::{Issue, Manifest, Severity, resolve_manifest_file};
use crate::schema::SettingsSchema;

/// Priority when ids collide: `Dev` > `User` > `Builtin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Builtin,
    User,
    Dev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// Every direct subfolder containing `plugin.json` is a plugin (builtin/user roots).
    Container,
    /// The folder itself is a plugin (dev folders).
    Single,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRoot {
    pub path: PathBuf,
    pub source: PluginSource,
    pub kind: RootKind,
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Canonicalized plugin folder.
    pub dir: PathBuf,
    pub source: PluginSource,
    /// `None` when `plugin.json` is missing or invalid (see `issues`).
    pub manifest: Option<Manifest>,
    /// Loaded when the manifest declares `contributes.settings` and the file is valid.
    pub settings_schema: Option<SettingsSchema>,
    pub issues: Vec<crate::manifest::Issue>,
    /// Another plugin with the same id and higher priority exists.
    pub shadowed: bool,
}

impl DiscoveredPlugin {
    /// Manifest id, or the folder name when the manifest could not be read.
    pub fn id(&self) -> String {
        match &self.manifest {
            Some(manifest) => manifest.id.clone(),
            None => self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|issue| issue.severity == crate::manifest::Severity::Error)
    }
}

/// Scans the roots. Missing roots are ignored. Result is sorted by id, then by priority (highest
/// first); lower-priority duplicates are marked `shadowed`. Never panics on bad input: every
/// problem becomes an [`Issue`].
///
/// In `Container` roots, hidden subfolders (`.git`) and plain files are skipped; any other
/// subfolder is reported, with a `manifest_missing` issue when it has no `plugin.json`. When two
/// plugins with the same id have the same source, the one whose folder sorts first wins.
pub fn discover(roots: &[PluginRoot]) -> Vec<DiscoveredPlugin> {
    let mut plugins = Vec::new();
    for root in roots {
        match root.kind {
            RootKind::Single => {
                if root.path.is_dir() {
                    plugins.push(load_plugin(&root.path, root.source));
                } else {
                    tracing::debug!(path = %root.path.display(), "plugin folder not found, skipped");
                }
            }
            RootKind::Container => {
                let entries = match std::fs::read_dir(&root.path) {
                    Ok(entries) => entries,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        tracing::warn!(path = %root.path.display(), error = %err, "could not read plugin folder");
                        continue;
                    }
                };
                let mut dirs: Vec<PathBuf> = entries
                    .filter_map(|entry| match entry {
                        Ok(entry) => Some(entry.path()),
                        Err(err) => {
                            tracing::warn!(path = %root.path.display(), error = %err, "could not read plugin folder entry");
                            None
                        }
                    })
                    .filter(|path| {
                        let hidden = path.file_name().and_then(|n| n.to_str()).is_none_or(|n| n.starts_with('.'));
                        !hidden && path.is_dir()
                    })
                    .collect();
                dirs.sort();
                plugins.extend(dirs.iter().map(|dir| load_plugin(dir, root.source)));
            }
        }
    }

    plugins.sort_by(|a, b| (a.id(), Reverse(a.source), &a.dir).cmp(&(b.id(), Reverse(b.source), &b.dir)));
    let mut previous_id: Option<String> = None;
    for plugin in &mut plugins {
        let id = plugin.id();
        plugin.shadowed = previous_id.as_deref() == Some(id.as_str());
        previous_id = Some(id);
    }
    plugins
}

/// Loads and validates one plugin folder (manifest + settings schema).
pub fn load_plugin(dir: &Path, source: PluginSource) -> DiscoveredPlugin {
    let mut plugin = DiscoveredPlugin {
        dir: dir.to_path_buf(),
        source,
        manifest: None,
        settings_schema: None,
        issues: Vec::new(),
        shadowed: false,
    };

    let canonical = match std::fs::canonicalize(dir) {
        Ok(canonical) => canonical,
        Err(err) => {
            tracing::warn!(path = %dir.display(), error = %err, "could not resolve plugin folder");
            plugin.issues.push(Issue::error(
                "manifest_unreadable",
                format!("plugin folder is not readable: {err}"),
                None,
            ));
            return plugin;
        }
    };
    plugin.dir = canonical.clone();

    let manifest_path = canonical.join(MANIFEST_FILE);
    let json = match std::fs::read_to_string(&manifest_path) {
        Ok(json) => json,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            plugin.issues.push(Issue::error(
                "manifest_missing",
                "plugin.json was not found in the plugin folder",
                None,
            ));
            return plugin;
        }
        Err(err) => {
            plugin.issues.push(Issue::error(
                "manifest_unreadable",
                format!("plugin.json could not be read: {err}"),
                None,
            ));
            return plugin;
        }
    };

    let manifest = match Manifest::parse(&json) {
        Ok(manifest) => manifest,
        Err(issue) => {
            plugin.issues.push(issue);
            return plugin;
        }
    };
    plugin.issues = manifest.validate(&canonical);

    let settings_path_ok = !plugin
        .issues
        .iter()
        .any(|issue| issue.severity == Severity::Error && issue.field.as_deref() == Some("contributes.settings"));
    if let Some(settings) = &manifest.contributes.settings
        && settings_path_ok
    {
        plugin.settings_schema = load_settings_schema(&canonical, settings, &mut plugin.issues);
    }
    plugin.manifest = Some(manifest);
    plugin
}

fn load_settings_schema(canonical_dir: &Path, path: &str, issues: &mut Vec<Issue>) -> Option<SettingsSchema> {
    let field = Some("contributes.settings");
    let file = match resolve_manifest_file(canonical_dir, path) {
        Ok(file) => file,
        Err(mut issue) => {
            issue.field = field.map(str::to_owned);
            issues.push(issue);
            return None;
        }
    };
    let json = match std::fs::read_to_string(&file) {
        Ok(json) => json,
        Err(err) => {
            issues.push(Issue::error(
                "settings_schema_invalid",
                format!("settings schema could not be read: {err}"),
                field,
            ));
            return None;
        }
    };
    let schema = match SettingsSchema::parse(&json) {
        Ok(schema) => schema,
        Err(mut issue) => {
            issue.field = field.map(str::to_owned);
            issues.push(issue);
            return None;
        }
    };
    let problems = schema.check();
    if problems.is_empty() {
        return Some(schema);
    }
    issues.extend(problems.into_iter().map(|problem| {
        let location = problem.field.unwrap_or_default();
        Issue { field: field.map(str::to_owned), message: format!("{location}: {}", problem.message), ..problem }
    }));
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn make_plugin(dir: &Path, id: &str, version: &str) {
        write(
            dir,
            "plugin.json",
            &json!({
                "id": id,
                "name": format!("Plugin {id}"),
                "version": version,
                "apiVersion": 1,
                "contributes": {
                    "pages": [{ "id": "main", "title": "Main", "path": "ui/index.html" }],
                    "settings": "settings.schema.json"
                }
            })
            .to_string(),
        );
        write(dir, "ui/index.html", "<html></html>");
        write(
            dir,
            "settings.schema.json",
            r#"{"type":"object","properties":{"token":{"type":"string","secret":true}}}"#,
        );
    }

    fn container(path: &Path, source: PluginSource) -> PluginRoot {
        PluginRoot { path: path.to_path_buf(), source, kind: RootKind::Container }
    }

    #[test]
    fn loads_valid_plugin_with_settings() {
        let root = tempfile::tempdir().unwrap();
        make_plugin(&root.path().join("hello"), "hello-obd", "0.1.0");
        let plugins = discover(&[container(root.path(), PluginSource::User)]);
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];
        assert_eq!(plugin.id(), "hello-obd");
        assert!(!plugin.has_errors(), "{:?}", plugin.issues);
        assert!(!plugin.shadowed);
        assert_eq!(plugin.dir, std::fs::canonicalize(root.path().join("hello")).unwrap());
        assert_eq!(plugin.settings_schema.as_ref().unwrap().secret_keys(), ["token"]);
    }

    #[test]
    fn missing_roots_are_ignored_and_bad_folders_reported() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("empty-folder")).unwrap();
        std::fs::create_dir_all(root.path().join(".git")).unwrap();
        write(root.path(), "README.txt", "not a plugin");
        write(&root.path().join("broken"), "plugin.json", "{ nope");

        let plugins = discover(&[
            container(&root.path().join("does-not-exist"), PluginSource::Builtin),
            PluginRoot { path: root.path().join("missing-dev"), source: PluginSource::Dev, kind: RootKind::Single },
            container(root.path(), PluginSource::User),
        ]);
        let summary: Vec<(String, Vec<&str>)> =
            plugins.iter().map(|p| (p.id(), p.issues.iter().map(|i| i.code.as_str()).collect())).collect();
        assert_eq!(
            summary,
            vec![
                ("broken".to_owned(), vec!["manifest_invalid"]),
                ("empty-folder".to_owned(), vec!["manifest_missing"])
            ]
        );
        assert!(plugins.iter().all(|p| p.manifest.is_none() && p.has_errors()));
    }

    #[test]
    fn single_root_loads_the_folder_itself() {
        let dev = tempfile::tempdir().unwrap();
        make_plugin(dev.path(), "dev-plugin", "1.0.0");
        let plugins = discover(&[PluginRoot {
            path: dev.path().to_path_buf(),
            source: PluginSource::Dev,
            kind: RootKind::Single,
        }]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id(), "dev-plugin");
        assert_eq!(plugins[0].source, PluginSource::Dev);
    }

    #[test]
    fn duplicates_are_shadowed_by_priority() {
        let builtin = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let dev = tempfile::tempdir().unwrap();
        make_plugin(&builtin.path().join("same"), "same-id", "1.0.0");
        make_plugin(&builtin.path().join("other"), "aaa-first", "1.0.0");
        make_plugin(&user.path().join("same"), "same-id", "2.0.0");
        make_plugin(&user.path().join("same-copy"), "same-id", "2.1.0");
        make_plugin(dev.path(), "same-id", "3.0.0");

        let plugins = discover(&[
            container(builtin.path(), PluginSource::Builtin),
            container(user.path(), PluginSource::User),
            PluginRoot { path: dev.path().to_path_buf(), source: PluginSource::Dev, kind: RootKind::Single },
        ]);
        let summary: Vec<(String, PluginSource, String, bool)> = plugins
            .iter()
            .map(|p| (p.id(), p.source, p.manifest.as_ref().unwrap().version.clone(), p.shadowed))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("aaa-first".into(), PluginSource::Builtin, "1.0.0".into(), false),
                ("same-id".into(), PluginSource::Dev, "3.0.0".into(), false),
                ("same-id".into(), PluginSource::User, "2.0.0".into(), true),
                ("same-id".into(), PluginSource::User, "2.1.0".into(), true),
                ("same-id".into(), PluginSource::Builtin, "1.0.0".into(), true),
            ]
        );
    }

    #[test]
    fn invalid_settings_schema_is_reported_on_contributes_settings() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        make_plugin(&dir, "schema-bad", "1.0.0");
        write(
            &dir,
            "settings.schema.json",
            r#"{"type":"object","properties":{"n":{"type":"integer","minimum":5,"maximum":1}}}"#,
        );
        let plugin = load_plugin(&dir, PluginSource::User);
        assert!(plugin.settings_schema.is_none());
        assert_eq!(plugin.issues.len(), 1);
        let issue = &plugin.issues[0];
        assert_eq!(issue.code, "settings_schema_invalid");
        assert_eq!(issue.field.as_deref(), Some("contributes.settings"));
        assert!(issue.message.starts_with("properties.n.minimum:"), "{}", issue.message);

        write(&dir, "settings.schema.json", "{ nope");
        let plugin = load_plugin(&dir, PluginSource::User);
        assert_eq!(plugin.issues[0].code, "settings_schema_invalid");
        assert!(plugin.manifest.is_some());

        std::fs::remove_file(dir.join("settings.schema.json")).unwrap();
        let plugin = load_plugin(&dir, PluginSource::User);
        let codes: Vec<&str> = plugin.issues.iter().map(|i| i.code.as_str()).collect();
        assert_eq!(codes, ["path_not_found"]);
        assert!(plugin.settings_schema.is_none());
    }

    #[test]
    fn manifest_validation_issues_are_kept() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        make_plugin(&dir, "Bad_Id", "1.0.0");
        let plugin = load_plugin(&dir, PluginSource::User);
        assert!(plugin.has_errors());
        assert_eq!(plugin.id(), "Bad_Id");
        assert!(plugin.manifest.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_plugin_folders_are_canonicalized() {
        let root = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        make_plugin(real.path(), "linked", "1.0.0");
        std::os::unix::fs::symlink(real.path(), root.path().join("linked")).unwrap();
        let plugins = discover(&[container(root.path(), PluginSource::User)]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].dir, std::fs::canonicalize(real.path()).unwrap());
        assert!(!plugins[0].has_errors(), "{:?}", plugins[0].issues);
    }
}

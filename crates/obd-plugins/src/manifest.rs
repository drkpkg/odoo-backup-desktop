//! `plugin.json` model and validation rules (see "Reglas de validación" in `docs/plugins.md`).
//!
//! Issue codes produced here and by discovery (stable, used by the UI):
//!
//! | Code | Severity | Meaning |
//! |---|---|---|
//! | `manifest_missing` | error | `plugin.json` not found in the folder |
//! | `manifest_unreadable` | error | `plugin.json` (or the folder) could not be read |
//! | `manifest_invalid` | error | invalid JSON, wrong shape or unknown keys |
//! | `invalid_id` | error | plugin id does not match `^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$` or is reserved |
//! | `invalid_name` | error | name empty or longer than 80 characters |
//! | `invalid_version` | error | version is not SemVer |
//! | `unsupported_api_version` | error | `apiVersion` is not [`crate::API_VERSION`] |
//! | `invalid_contribution_id` | error | page/menu/window/destination id does not match `^[a-z0-9][a-z0-9_-]{0,62}$` |
//! | `duplicate_contribution_id` | error | two pages/menus/windows/destinations share an id |
//! | `invalid_label` | error | title/label empty or longer than 80 characters |
//! | `invalid_path` | error | path empty, absolute, with `..`, `.`/hidden or empty segments, backslashes, `:` or NUL |
//! | `path_not_found` | error | the referenced file does not exist (or is not a regular file) |
//! | `path_outside_plugin` | error | the file resolves (symlinks) outside the plugin folder |
//! | `invalid_menu_target` | error | a menu needs exactly one of `page` or `window` |
//! | `unknown_page` | error | menu references a page id that is not declared |
//! | `unknown_window` | error | menu references a window id that is not declared |
//! | `invalid_icon` | error | icon does not match `^[a-z0-9-]{1,40}$` |
//! | `invalid_window_size` | error | width/height outside 320–4096 |
//! | `invalid_network_host` | error | network permission is not a host or `*.domain` wildcard |
//! | `backend_not_supported` | warning | `backend`, `destinations` or `hooks` declared (phase B) |
//! | `settings_schema_invalid` | error | settings schema could not be parsed or is inconsistent |

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::SDK_SEGMENT;

const MAX_NAME_CHARS: usize = 80;
const MAX_LABEL_CHARS: usize = 80;
const MIN_WINDOW_SIZE: u32 = 320;
const MAX_WINDOW_SIZE: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A validation problem. Errors prevent loading; warnings don't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    /// Stable code, see the module documentation for the list.
    pub code: String,
    pub message: String,
    /// JSON path of the offending field, e.g. `contributes.menus[1].page`.
    pub field: Option<String>,
}

impl Issue {
    pub fn error(code: impl Into<String>, message: impl Into<String>, field: Option<&str>) -> Self {
        Self { severity: Severity::Error, code: code.into(), message: message.into(), field: field.map(str::to_owned) }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>, field: Option<&str>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.into(),
            message: message.into(),
            field: field.map(str::to_owned),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub contributes: Contributions,
    #[serde(default)]
    pub permissions: Permissions,
    /// WebAssembly backend (phase B).
    #[serde(default)]
    pub backend: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contributions {
    #[serde(default)]
    pub pages: Vec<Page>,
    #[serde(default)]
    pub menus: Vec<Menu>,
    #[serde(default)]
    pub windows: Vec<Window>,
    /// Path to the settings schema JSON file.
    #[serde(default)]
    pub settings: Option<String>,
    /// Backup destinations (phase B).
    #[serde(default)]
    pub destinations: Vec<Destination>,
    /// Backup hooks such as `after_backup` (phase B).
    #[serde(default)]
    pub hooks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuLocation {
    Sidebar,
    InstanceActions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Menu {
    pub id: String,
    pub location: MenuLocation,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub page: Option<String>,
    #[serde(default)]
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub path: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Destination {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub settings: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Permissions {
    /// Hosts (`api.example.com`) or subdomain wildcards (`*.example.com`) the plugin UI may call.
    #[serde(default)]
    pub network: Vec<String>,
}

impl Manifest {
    /// Parses `plugin.json`. JSON/shape errors are returned as a single `manifest_invalid` issue.
    pub fn parse(json: &str) -> std::result::Result<Self, Issue> {
        serde_json::from_str(json).map_err(|err| {
            Issue::error(
                "manifest_invalid",
                format!("plugin.json is invalid: {err} (line {}, column {})", err.line(), err.column()),
                None,
            )
        })
    }

    /// Checks every rule of the contract against the plugin folder (`plugin_dir`).
    pub fn validate(&self, plugin_dir: &Path) -> Vec<Issue> {
        let mut issues = Vec::new();
        let canonical_dir = std::fs::canonicalize(plugin_dir).unwrap_or_else(|_| plugin_dir.to_path_buf());

        if !is_valid_plugin_id(&self.id) {
            issues.push(Issue::error(
                "invalid_id",
                format!("plugin id {:?} must match ^[a-z0-9][a-z0-9-]{{1,62}}[a-z0-9]$ and not be reserved", self.id),
                Some("id"),
            ));
        }
        let name_len = self.name.trim().chars().count();
        if name_len == 0 || self.name.chars().count() > MAX_NAME_CHARS {
            issues.push(Issue::error("invalid_name", "name must have between 1 and 80 characters", Some("name")));
        }
        if semver::Version::parse(&self.version).is_err() {
            issues.push(Issue::error(
                "invalid_version",
                format!("version {:?} is not SemVer (MAJOR.MINOR.PATCH)", self.version),
                Some("version"),
            ));
        }
        if self.api_version != crate::API_VERSION {
            issues.push(Issue::error(
                "unsupported_api_version",
                format!("apiVersion {} is not supported (expected {})", self.api_version, crate::API_VERSION),
                Some("apiVersion"),
            ));
        }

        let contributes = &self.contributes;

        // Pages.
        let mut page_ids = HashSet::new();
        for (index, page) in contributes.pages.iter().enumerate() {
            let base = format!("contributes.pages[{index}]");
            check_contribution_id(&mut issues, &mut page_ids, &page.id, &base);
            check_label(&mut issues, &page.title, &format!("{base}.title"));
            check_file(&mut issues, &canonical_dir, &page.path, &format!("{base}.path"));
        }

        // Windows.
        let mut window_ids = HashSet::new();
        for (index, window) in contributes.windows.iter().enumerate() {
            let base = format!("contributes.windows[{index}]");
            check_contribution_id(&mut issues, &mut window_ids, &window.id, &base);
            check_label(&mut issues, &window.title, &format!("{base}.title"));
            check_file(&mut issues, &canonical_dir, &window.path, &format!("{base}.path"));
            for (dimension, value) in [("width", window.width), ("height", window.height)] {
                if let Some(value) = value
                    && !(MIN_WINDOW_SIZE..=MAX_WINDOW_SIZE).contains(&value)
                {
                    issues.push(Issue::error(
                        "invalid_window_size",
                        format!("{dimension} must be between {MIN_WINDOW_SIZE} and {MAX_WINDOW_SIZE}"),
                        Some(&format!("{base}.{dimension}")),
                    ));
                }
            }
        }

        // Menus.
        let mut menu_ids = HashSet::new();
        for (index, menu) in contributes.menus.iter().enumerate() {
            let base = format!("contributes.menus[{index}]");
            check_contribution_id(&mut issues, &mut menu_ids, &menu.id, &base);
            check_label(&mut issues, &menu.label, &format!("{base}.label"));
            if let Some(icon) = &menu.icon
                && !is_valid_icon(icon)
            {
                issues.push(Issue::error(
                    "invalid_icon",
                    format!("icon {icon:?} must match ^[a-z0-9-]{{1,40}}$"),
                    Some(&format!("{base}.icon")),
                ));
            }
            match (&menu.page, &menu.window) {
                (Some(page), None) => {
                    if !contributes.pages.iter().any(|p| &p.id == page) {
                        issues.push(Issue::error(
                            "unknown_page",
                            format!("page {page:?} is not declared in contributes.pages"),
                            Some(&format!("{base}.page")),
                        ));
                    }
                }
                (None, Some(window)) => {
                    if !contributes.windows.iter().any(|w| &w.id == window) {
                        issues.push(Issue::error(
                            "unknown_window",
                            format!("window {window:?} is not declared in contributes.windows"),
                            Some(&format!("{base}.window")),
                        ));
                    }
                }
                _ => issues.push(Issue::error(
                    "invalid_menu_target",
                    "a menu needs exactly one of \"page\" or \"window\"",
                    Some(&base),
                )),
            }
        }

        // Settings schema file (content is checked by discovery).
        if let Some(settings) = &contributes.settings {
            check_file(&mut issues, &canonical_dir, settings, "contributes.settings");
        }

        // Network permissions.
        for (index, host) in self.permissions.network.iter().enumerate() {
            if !is_valid_network_host(host) {
                issues.push(Issue::error(
                    "invalid_network_host",
                    format!("{host:?} must be a host name (api.example.com) or a subdomain wildcard (*.example.com)"),
                    Some(&format!("permissions.network[{index}]")),
                ));
            }
        }

        // Phase B contributions: validated but not loaded yet.
        if let Some(backend) = &self.backend {
            check_file(&mut issues, &canonical_dir, backend, "backend");
            issues.push(Issue::warning(
                "backend_not_supported",
                "plugin backends are not supported yet; the backend is ignored",
                Some("backend"),
            ));
        }
        if !contributes.destinations.is_empty() {
            let mut destination_ids = HashSet::new();
            for (index, destination) in contributes.destinations.iter().enumerate() {
                let base = format!("contributes.destinations[{index}]");
                check_contribution_id(&mut issues, &mut destination_ids, &destination.id, &base);
                check_label(&mut issues, &destination.label, &format!("{base}.label"));
                if let Some(settings) = &destination.settings {
                    check_file(&mut issues, &canonical_dir, settings, &format!("{base}.settings"));
                }
            }
            issues.push(Issue::warning(
                "backend_not_supported",
                "backup destinations need plugin backend support, which is not available yet",
                Some("contributes.destinations"),
            ));
        }
        if !contributes.hooks.is_empty() {
            issues.push(Issue::warning(
                "backend_not_supported",
                "backup hooks need plugin backend support, which is not available yet",
                Some("contributes.hooks"),
            ));
        }

        issues
    }

    pub fn page(&self, id: &str) -> Option<&Page> {
        self.contributes.pages.iter().find(|page| page.id == id)
    }

    pub fn window(&self, id: &str) -> Option<&Window> {
        self.contributes.windows.iter().find(|window| window.id == id)
    }
}

/// `^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$` and not the reserved `_sdk`.
pub fn is_valid_plugin_id(id: &str) -> bool {
    if id == SDK_SEGMENT {
        return false;
    }
    let bytes = id.as_bytes();
    if !(3..=64).contains(&bytes.len()) {
        return false;
    }
    let edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let inner = |b: u8| edge(b) || b == b'-';
    edge(bytes[0]) && edge(bytes[bytes.len() - 1]) && bytes.iter().all(|&b| inner(b))
}

/// `^[a-z0-9][a-z0-9_-]{0,62}$` (pages, menus, windows, destinations).
pub fn is_valid_contribution_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    first && bytes.iter().all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// `^[a-z0-9-]{1,40}$`.
pub fn is_valid_icon(icon: &str) -> bool {
    (1..=40).contains(&icon.len()) && icon.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A lowercase host name (`api.example.com`, `localhost`) or a subdomain wildcard with at least
/// two labels after it (`*.example.com`). No scheme, port, path or uppercase letters.
pub fn is_valid_network_host(host: &str) -> bool {
    let (wildcard, name) = match host.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, host),
    };
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if wildcard && labels.len() < 2 {
        return false;
    }
    labels.iter().all(|label| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes[0] != b'-'
            && bytes[bytes.len() - 1] != b'-'
            && bytes.iter().all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// Syntax check of a manifest-relative path. Returns the path segments when valid.
///
/// Rejected: empty, absolute (`/x`), backslashes, `:` (drive letters, schemes), NUL, and empty,
/// `.`, `..` or hidden (`.x`) segments.
pub(crate) fn relative_segments(path: &str) -> Option<Vec<&str>> {
    if path.is_empty() || path.contains(['\\', ':', '\0']) || path.starts_with('/') {
        return None;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty() || segment.starts_with('.')) {
        return None;
    }
    Some(segments)
}

/// Resolves a manifest path to a regular file inside `canonical_dir`.
pub(crate) fn resolve_manifest_file(canonical_dir: &Path, path: &str) -> std::result::Result<PathBuf, Issue> {
    let Some(segments) = relative_segments(path) else {
        return Err(Issue::error(
            "invalid_path",
            format!("{path:?} must be a relative path inside the plugin folder (no \"..\", hidden or empty segments)"),
            None,
        ));
    };
    let candidate = segments.iter().fold(canonical_dir.to_path_buf(), |acc, segment| acc.join(segment));
    let Ok(canonical) = std::fs::canonicalize(&candidate) else {
        return Err(Issue::error("path_not_found", format!("{path:?} does not exist"), None));
    };
    if !canonical.starts_with(canonical_dir) {
        return Err(Issue::error("path_outside_plugin", format!("{path:?} resolves outside the plugin folder"), None));
    }
    if !canonical.is_file() {
        return Err(Issue::error("path_not_found", format!("{path:?} is not a file"), None));
    }
    Ok(canonical)
}

fn check_file(issues: &mut Vec<Issue>, canonical_dir: &Path, path: &str, field: &str) {
    if let Err(mut issue) = resolve_manifest_file(canonical_dir, path) {
        issue.field = Some(field.to_owned());
        issues.push(issue);
    }
}

fn check_contribution_id(issues: &mut Vec<Issue>, seen: &mut HashSet<String>, id: &str, base: &str) {
    let field = format!("{base}.id");
    if !is_valid_contribution_id(id) {
        issues.push(Issue::error(
            "invalid_contribution_id",
            format!("id {id:?} must match ^[a-z0-9][a-z0-9_-]{{0,62}}$"),
            Some(&field),
        ));
    }
    if !seen.insert(id.to_owned()) {
        issues.push(Issue::error("duplicate_contribution_id", format!("id {id:?} is declared twice"), Some(&field)));
    }
}

fn check_label(issues: &mut Vec<Issue>, label: &str, field: &str) {
    if label.trim().is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        issues.push(Issue::error("invalid_label", "must have between 1 and 80 characters", Some(field)));
    }
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

    fn valid_manifest() -> serde_json::Value {
        json!({
            "id": "hello-obd",
            "name": "Hola OBD",
            "version": "0.1.0",
            "apiVersion": 1,
            "description": "Plugin de ejemplo",
            "contributes": {
                "pages": [{ "id": "main", "title": "Hola", "path": "ui/index.html" }],
                "menus": [
                    { "id": "sidebar", "location": "sidebar", "label": "Hola", "icon": "sparkles", "page": "main" },
                    { "id": "instance", "location": "instance_actions", "label": "Ver", "icon": "eye", "window": "detail" }
                ],
                "windows": [{ "id": "detail", "title": "Detalle", "path": "ui/detail.html", "width": 720, "height": 520 }],
                "settings": "settings.schema.json"
            },
            "permissions": { "network": ["api.example.com", "*.example.org"] }
        })
    }

    fn plugin_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "ui/index.html", "<html></html>");
        write(dir.path(), "ui/detail.html", "<html></html>");
        write(dir.path(), "settings.schema.json", "{}");
        dir
    }

    fn issues_for(value: serde_json::Value, dir: &Path) -> Vec<Issue> {
        Manifest::parse(&value.to_string()).unwrap().validate(dir)
    }

    fn codes(issues: &[Issue]) -> Vec<(&str, Option<&str>)> {
        issues.iter().map(|i| (i.code.as_str(), i.field.as_deref())).collect()
    }

    #[test]
    fn valid_manifest_has_no_issues() {
        let dir = plugin_dir();
        let issues = issues_for(valid_manifest(), dir.path());
        assert!(issues.is_empty(), "{issues:?}");
        let manifest = Manifest::parse(&valid_manifest().to_string()).unwrap();
        assert_eq!(manifest.page("main").unwrap().path, "ui/index.html");
        assert_eq!(manifest.window("detail").unwrap().width, Some(720));
        assert_eq!(manifest.contributes.menus[1].location, MenuLocation::InstanceActions);
    }

    #[test]
    fn parse_errors_are_single_manifest_invalid_issue() {
        let err = Manifest::parse("{ not json").unwrap_err();
        assert_eq!(err.code, "manifest_invalid");
        assert_eq!(err.severity, Severity::Error);
        assert!(err.message.contains("line 1"), "{}", err.message);

        let mut unknown = valid_manifest();
        unknown["contributs"] = json!({});
        let err = Manifest::parse(&unknown.to_string()).unwrap_err();
        assert!(err.message.contains("contributs"), "{}", err.message);

        let mut unknown_nested = valid_manifest();
        unknown_nested["contributes"]["pages"][0]["titel"] = json!("x");
        assert_eq!(Manifest::parse(&unknown_nested.to_string()).unwrap_err().code, "manifest_invalid");

        let mut bad_location = valid_manifest();
        bad_location["contributes"]["menus"][0]["location"] = json!("toolbar");
        assert_eq!(Manifest::parse(&bad_location.to_string()).unwrap_err().code, "manifest_invalid");
    }

    #[test]
    fn plugin_id_rules() {
        for ok in ["abc", "hello-obd", "a1b", "s3-storage", &"a".repeat(64)] {
            assert!(is_valid_plugin_id(ok), "{ok}");
        }
        for bad in ["ab", "_sdk", "-abc", "abc-", "Abc", "a_b", "a.b", "", &"a".repeat(65), "héllo"] {
            assert!(!is_valid_plugin_id(bad), "{bad}");
        }
    }

    #[test]
    fn top_level_fields_are_validated() {
        let dir = plugin_dir();
        let mut value = valid_manifest();
        value["id"] = json!("Bad_Id");
        value["name"] = json!("  ");
        value["version"] = json!("1.0");
        value["apiVersion"] = json!(2);
        let issues = issues_for(value, dir.path());
        assert_eq!(
            codes(&issues),
            vec![
                ("invalid_id", Some("id")),
                ("invalid_name", Some("name")),
                ("invalid_version", Some("version")),
                ("unsupported_api_version", Some("apiVersion")),
            ]
        );

        let mut long_name = valid_manifest();
        long_name["name"] = json!("x".repeat(81));
        assert_eq!(codes(&issues_for(long_name, dir.path())), vec![("invalid_name", Some("name"))]);

        let mut prerelease = valid_manifest();
        prerelease["version"] = json!("1.2.3-beta.1+build.5");
        assert!(issues_for(prerelease, dir.path()).is_empty());
    }

    #[test]
    fn contribution_ids_labels_and_duplicates() {
        let dir = plugin_dir();
        let mut value = valid_manifest();
        value["contributes"]["pages"] = json!([
            { "id": "main", "title": "Hola", "path": "ui/index.html" },
            { "id": "main", "title": "", "path": "ui/index.html" },
            { "id": "Bad", "title": "x", "path": "ui/index.html" }
        ]);
        let issues = issues_for(value, dir.path());
        assert_eq!(
            codes(&issues),
            vec![
                ("duplicate_contribution_id", Some("contributes.pages[1].id")),
                ("invalid_label", Some("contributes.pages[1].title")),
                ("invalid_contribution_id", Some("contributes.pages[2].id")),
            ]
        );
    }

    #[test]
    fn menu_targets_icons_and_windows() {
        let dir = plugin_dir();
        let mut value = valid_manifest();
        value["contributes"]["menus"] = json!([
            { "id": "both", "location": "sidebar", "label": "x", "page": "main", "window": "detail" },
            { "id": "none", "location": "sidebar", "label": "x" },
            { "id": "ghost-page", "location": "sidebar", "label": "x", "page": "nope" },
            { "id": "ghost-window", "location": "instance_actions", "label": "x", "window": "nope" },
            { "id": "icon", "location": "sidebar", "label": "x", "icon": "Bad Icon", "page": "main" }
        ]);
        value["contributes"]["windows"][0]["width"] = json!(100);
        value["contributes"]["windows"][0]["height"] = json!(5000);
        let issues = issues_for(value, dir.path());
        assert_eq!(
            codes(&issues),
            vec![
                ("invalid_window_size", Some("contributes.windows[0].width")),
                ("invalid_window_size", Some("contributes.windows[0].height")),
                ("invalid_menu_target", Some("contributes.menus[0]")),
                ("invalid_menu_target", Some("contributes.menus[1]")),
                ("unknown_page", Some("contributes.menus[2].page")),
                ("unknown_window", Some("contributes.menus[3].window")),
                ("invalid_icon", Some("contributes.menus[4].icon")),
            ]
        );
    }

    #[test]
    fn paths_must_stay_inside_the_plugin() {
        let dir = plugin_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.html", "x");
        let cases = [
            ("", "invalid_path"),
            ("/etc/passwd", "invalid_path"),
            ("../secret.html", "invalid_path"),
            ("ui/../ui/index.html", "invalid_path"),
            ("./ui/index.html", "invalid_path"),
            ("ui//index.html", "invalid_path"),
            ("ui\\index.html", "invalid_path"),
            ("C:/Windows/win.ini", "invalid_path"),
            ("C:\\Windows\\win.ini", "invalid_path"),
            ("\\\\server\\share\\x", "invalid_path"),
            (".hidden/index.html", "invalid_path"),
            ("ui/missing.html", "path_not_found"),
            ("ui", "path_not_found"),
        ];
        for (path, code) in cases {
            let mut value = valid_manifest();
            value["contributes"]["pages"][0]["path"] = json!(path);
            let issues = issues_for(value, dir.path());
            assert_eq!(codes(&issues), vec![(code, Some("contributes.pages[0].path"))], "path {path:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_the_plugin_is_rejected() {
        let dir = plugin_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.html", "x");
        std::os::unix::fs::symlink(outside.path().join("secret.html"), dir.path().join("ui/link.html")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked-dir")).unwrap();
        // A symlink that stays inside is fine.
        std::os::unix::fs::symlink(dir.path().join("ui/index.html"), dir.path().join("ui/alias.html")).unwrap();

        for (path, expected) in [
            ("ui/link.html", Some("path_outside_plugin")),
            ("linked-dir/secret.html", Some("path_outside_plugin")),
            ("ui/alias.html", None),
        ] {
            let mut value = valid_manifest();
            value["contributes"]["pages"][0]["path"] = json!(path);
            let issues = issues_for(value, dir.path());
            assert_eq!(issues.first().map(|i| i.code.as_str()), expected, "path {path:?}");
        }
    }

    #[test]
    fn network_hosts() {
        for ok in ["api.example.com", "localhost", "*.example.org", "192.168.1.10", "a-b.example.com"] {
            assert!(is_valid_network_host(ok), "{ok}");
        }
        for bad in [
            "",
            "*",
            "*.com",
            "https://api.example.com",
            "api.example.com:443",
            "api.example.com/path",
            "API.example.com",
            "-bad.example.com",
            "bad-.example.com",
            "a..b",
            "*.*.example.com",
            "x; script-src *",
        ] {
            assert!(!is_valid_network_host(bad), "{bad}");
        }
        let dir = plugin_dir();
        let mut value = valid_manifest();
        value["permissions"]["network"] = json!(["api.example.com", "https://x.com"]);
        assert_eq!(
            codes(&issues_for(value, dir.path())),
            vec![("invalid_network_host", Some("permissions.network[1]"))]
        );
    }

    #[test]
    fn phase_b_fields_produce_warnings() {
        let dir = plugin_dir();
        write(dir.path(), "backend.wasm", "\0asm");
        write(dir.path(), "s3.schema.json", "{}");
        let mut value = valid_manifest();
        value["backend"] = json!("backend.wasm");
        value["contributes"]["destinations"] = json!([
            { "id": "s3", "label": "Amazon S3", "settings": "s3.schema.json" },
            { "id": "s3", "label": "", "settings": "missing.json" }
        ]);
        value["contributes"]["hooks"] = json!(["after_backup"]);
        let issues = issues_for(value, dir.path());
        assert_eq!(
            codes(&issues),
            vec![
                ("backend_not_supported", Some("backend")),
                ("duplicate_contribution_id", Some("contributes.destinations[1].id")),
                ("invalid_label", Some("contributes.destinations[1].label")),
                ("path_not_found", Some("contributes.destinations[1].settings")),
                ("backend_not_supported", Some("contributes.destinations")),
                ("backend_not_supported", Some("contributes.hooks")),
            ]
        );
        assert!(issues.iter().filter(|i| i.code == "backend_not_supported").all(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn missing_settings_file_is_an_error() {
        let dir = plugin_dir();
        let mut value = valid_manifest();
        value["contributes"]["settings"] = json!("nope.json");
        assert_eq!(codes(&issues_for(value, dir.path())), vec![("path_not_found", Some("contributes.settings"))]);
    }
}

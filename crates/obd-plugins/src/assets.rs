//! Resolving `obd-plugin` requests to files inside a plugin folder.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use thiserror::Error;

use crate::manifest::is_valid_network_host;

/// Origins allowed to frame plugin pages: the app (`tauri://`, Windows `http(s)://tauri.localhost`)
/// and the Vite dev server used by `pnpm tauri dev`.
const FRAME_ANCESTORS: &str = "tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:*";
/// Sources of the plugin protocol on every platform (Windows serves custom schemes over http).
const PROTOCOL_SOURCES: &str = "obd-plugin: http://obd-plugin.localhost";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Canonical path, guaranteed to be inside the plugin folder.
    pub path: PathBuf,
    pub mime: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssetError {
    #[error("not found")]
    NotFound,
    /// Traversal (`..`), absolute paths, hidden segments, or symlinks leaving the folder.
    #[error("forbidden")]
    Forbidden,
}

/// Splits a protocol request path `/<first>/<rest>` into (`first`, `rest`), percent-decoding
/// both. `first` is a plugin id or [`crate::SDK_SEGMENT`]. Query and fragment must already be
/// stripped. Returns `None` when there is no first segment.
///
/// Each part is decoded separately (so `%2F` never creates a new first segment). Invalid UTF-8
/// returns `None`. Decoded NUL or backslash characters are kept and rejected by
/// [`resolve_asset`].
pub fn split_protocol_path(uri_path: &str) -> Option<(String, String)> {
    let trimmed = uri_path.strip_prefix('/').unwrap_or(uri_path);
    let (first, rest) = match trimmed.split_once('/') {
        Some((first, rest)) => (first, rest),
        None => (trimmed, ""),
    };
    if first.is_empty() {
        return None;
    }
    let first = percent_decode_str(first).decode_utf8().ok()?.into_owned();
    let rest = percent_decode_str(rest).decode_utf8().ok()?.into_owned();
    if first.is_empty() {
        return None;
    }
    Some((first, rest))
}

/// Resolves `relative` (already decoded, `/`-separated) inside `plugin_dir`. An empty path or a
/// directory is `NotFound` (no index fallback).
pub fn resolve_asset(plugin_dir: &Path, relative: &str) -> Result<Asset, AssetError> {
    if relative.is_empty() || relative.ends_with('/') {
        return Err(AssetError::NotFound);
    }
    if relative.starts_with('/') || relative.contains(['\\', ':', '\0']) {
        return Err(AssetError::Forbidden);
    }
    let segments: Vec<&str> = relative.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty() || segment.starts_with('.')) {
        return Err(AssetError::Forbidden);
    }

    let root = std::fs::canonicalize(plugin_dir).map_err(|_| AssetError::NotFound)?;
    let candidate = segments.iter().fold(root.clone(), |acc, segment| acc.join(segment));
    let canonical = std::fs::canonicalize(&candidate).map_err(|_| AssetError::NotFound)?;
    if !canonical.starts_with(&root) {
        return Err(AssetError::Forbidden);
    }
    if !canonical.is_file() {
        return Err(AssetError::NotFound);
    }
    let mime = mime_for(&canonical);
    Ok(Asset { path: canonical, mime })
}

/// MIME type by extension (html, js/mjs, css, json, svg, png, jpg/jpeg, gif, webp, ico, woff,
/// woff2, ttf, map, txt, wasm); `application/octet-stream` otherwise. Text types include
/// `; charset=utf-8`.
pub fn mime_for(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// `Content-Security-Policy` for plugin HTML pages: resources only from the plugin protocol
/// (both `obd-plugin:` and `http://obd-plugin.localhost`), inline scripts/styles allowed,
/// `img-src` also `data: blob:`, `connect-src` limited to `https://<host>` for each network
/// permission (wildcards kept as `https://*.example.com`), `object-src 'none'`, `base-uri 'none'`.
///
/// Network entries that are not valid host patterns are ignored (they cannot inject directives);
/// valid ones are sorted and deduplicated. `frame-ancestors` allows the app origins and the local
/// Vite dev server (`http://localhost:*`).
pub fn page_csp(network: &[String]) -> String {
    let mut hosts: Vec<&str> = network.iter().map(String::as_str).filter(|host| is_valid_network_host(host)).collect();
    hosts.sort_unstable();
    hosts.dedup();
    let mut connect = PROTOCOL_SOURCES.to_owned();
    for host in hosts {
        connect.push_str(" https://");
        connect.push_str(host);
    }

    [
        format!("default-src {PROTOCOL_SOURCES}"),
        format!("script-src {PROTOCOL_SOURCES} 'unsafe-inline'"),
        format!("style-src {PROTOCOL_SOURCES} 'unsafe-inline'"),
        format!("img-src {PROTOCOL_SOURCES} data: blob:"),
        format!("font-src {PROTOCOL_SOURCES} data:"),
        format!("connect-src {connect}"),
        "object-src 'none'".to_owned(),
        "base-uri 'none'".to_owned(),
        format!("frame-ancestors {FRAME_ANCESTORS}"),
    ]
    .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("ui/assets")).unwrap();
        std::fs::write(dir.path().join("ui/index.html"), "<html></html>").unwrap();
        std::fs::write(dir.path().join("ui/assets/app.JS"), "export {}").unwrap();
        std::fs::write(dir.path().join("ui/my page.html"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join(".secret")).unwrap();
        std::fs::write(dir.path().join(".secret/key.txt"), "x").unwrap();
        dir
    }

    #[test]
    fn splits_and_decodes_paths() {
        assert_eq!(split_protocol_path("/hello-obd/ui/index.html"), Some(("hello-obd".into(), "ui/index.html".into())));
        assert_eq!(split_protocol_path("hello-obd/ui/index.html"), Some(("hello-obd".into(), "ui/index.html".into())));
        assert_eq!(split_protocol_path("/hello-obd"), Some(("hello-obd".into(), String::new())));
        assert_eq!(split_protocol_path("/hello-obd/"), Some(("hello-obd".into(), String::new())));
        assert_eq!(split_protocol_path("/_sdk/obd-plugin.js"), Some(("_sdk".into(), "obd-plugin.js".into())));
        assert_eq!(split_protocol_path("/p/ui/my%20page.html"), Some(("p".into(), "ui/my page.html".into())));
        // %2F stays inside its part.
        assert_eq!(split_protocol_path("/a%2Fb/c%2F..%2Fd"), Some(("a/b".into(), "c/../d".into())));
        assert_eq!(split_protocol_path("/p/%2e%2e/x"), Some(("p".into(), "../x".into())));
        assert_eq!(split_protocol_path("/p/a%00b"), Some(("p".into(), "a\0b".into())));
        assert_eq!(split_protocol_path("/p/%5Cx"), Some(("p".into(), "\\x".into())));
        assert_eq!(split_protocol_path("/"), None);
        assert_eq!(split_protocol_path(""), None);
        assert_eq!(split_protocol_path("//x"), None);
        assert_eq!(split_protocol_path("/p/%FF"), None);
    }

    #[test]
    fn resolves_files_inside_the_plugin() {
        let dir = plugin_dir();
        let asset = resolve_asset(dir.path(), "ui/index.html").unwrap();
        assert_eq!(asset.path, std::fs::canonicalize(dir.path().join("ui/index.html")).unwrap());
        assert_eq!(asset.mime, "text/html; charset=utf-8");
        assert_eq!(resolve_asset(dir.path(), "ui/assets/app.JS").unwrap().mime, "text/javascript; charset=utf-8");
        assert!(resolve_asset(dir.path(), "ui/my page.html").is_ok());
    }

    #[test]
    fn rejects_traversal_and_hidden_segments() {
        let dir = plugin_dir();
        for forbidden in [
            "../secret",
            "ui/../../etc/passwd",
            "ui/../ui/index.html",
            "./ui/index.html",
            "/etc/passwd",
            "ui//index.html",
            "ui\\index.html",
            "C:/Windows/win.ini",
            ".secret/key.txt",
            "ui/.hidden",
            "ui/index.html\0.png",
        ] {
            assert_eq!(resolve_asset(dir.path(), forbidden), Err(AssetError::Forbidden), "{forbidden:?}");
        }
        for missing in ["", "ui", "ui/", "ui/missing.html", "ui/assets"] {
            assert_eq!(resolve_asset(dir.path(), missing), Err(AssetError::NotFound), "{missing:?}");
        }
        assert_eq!(resolve_asset(&dir.path().join("nope"), "ui/index.html"), Err(AssetError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_leaving_the_plugin() {
        let dir = plugin_dir();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), dir.path().join("ui/leak.txt")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("ui/outside")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("ui/index.html"), dir.path().join("ui/alias.html")).unwrap();

        assert_eq!(resolve_asset(dir.path(), "ui/leak.txt"), Err(AssetError::Forbidden));
        assert_eq!(resolve_asset(dir.path(), "ui/outside/secret.txt"), Err(AssetError::Forbidden));
        assert!(resolve_asset(dir.path(), "ui/alias.html").is_ok());
    }

    #[test]
    fn mime_table() {
        let cases = [
            ("a.html", "text/html; charset=utf-8"),
            ("a.htm", "text/html; charset=utf-8"),
            ("a.mjs", "text/javascript; charset=utf-8"),
            ("a.css", "text/css; charset=utf-8"),
            ("a.json", "application/json; charset=utf-8"),
            ("a.js.map", "application/json; charset=utf-8"),
            ("a.svg", "image/svg+xml; charset=utf-8"),
            ("a.txt", "text/plain; charset=utf-8"),
            ("a.PNG", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.ico", "image/x-icon"),
            ("a.woff", "font/woff"),
            ("a.woff2", "font/woff2"),
            ("a.ttf", "font/ttf"),
            ("backend.wasm", "application/wasm"),
            ("a.exe", "application/octet-stream"),
            ("Makefile", "application/octet-stream"),
        ];
        for (name, mime) in cases {
            assert_eq!(mime_for(Path::new(name)), mime, "{name}");
        }
    }

    #[test]
    fn csp_limits_sources_and_network() {
        let csp = page_csp(&[
            "*.example.org".into(),
            "api.example.com".into(),
            "api.example.com".into(),
            "evil.com; script-src *".into(),
            "https://bad.com".into(),
        ]);
        assert_eq!(
            csp,
            "default-src obd-plugin: http://obd-plugin.localhost; \
             script-src obd-plugin: http://obd-plugin.localhost 'unsafe-inline'; \
             style-src obd-plugin: http://obd-plugin.localhost 'unsafe-inline'; \
             img-src obd-plugin: http://obd-plugin.localhost data: blob:; \
             font-src obd-plugin: http://obd-plugin.localhost data:; \
             connect-src obd-plugin: http://obd-plugin.localhost https://*.example.org https://api.example.com; \
             object-src 'none'; base-uri 'none'; \
             frame-ancestors tauri://localhost http://tauri.localhost https://tauri.localhost http://localhost:*"
        );
        assert!(page_csp(&[]).contains("connect-src obd-plugin: http://obd-plugin.localhost; object-src"));
    }
}

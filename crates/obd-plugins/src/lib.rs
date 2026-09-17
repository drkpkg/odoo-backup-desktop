//! Plugin system core for Odoo Backup Desktop (API v1, see `docs/plugins.md`).
//!
//! Pure, UI-agnostic pieces used by the Tauri app:
//! - [`manifest`]: `plugin.json` parsing and validation.
//! - [`schema`]: settings schema subset (form generation + value validation).
//! - [`discovery`]: scanning plugin folders with source priority (`dev` > `user` > `builtin`).
//! - [`assets`]: resolving `obd-plugin://` requests to files without escaping the plugin folder.
//! - [`store`]: plugin config, non-secret settings and per-plugin key/value storage on disk.

pub mod assets;
pub mod discovery;
pub mod manifest;
pub mod schema;
pub mod store;

mod error;

pub use assets::{Asset, AssetError, mime_for, page_csp, resolve_asset, split_protocol_path};
pub use discovery::{DiscoveredPlugin, PluginRoot, PluginSource, RootKind, discover, load_plugin};
pub use error::{PluginError, Result};
pub use manifest::{
    Contributions, Destination, Issue, Manifest, Menu, MenuLocation, Page, Permissions, Severity, Window,
    is_valid_contribution_id, is_valid_icon, is_valid_network_host, is_valid_plugin_id,
};
pub use schema::{FieldError, Format, PropertyType, SchemaProperty, SecretChanges, SettingsSchema, ValidatedSettings};
pub use store::{
    PluginConfig, PluginConfigStore, PluginSettingsStore, PluginStorage, STORAGE_LIMIT_BYTES, is_valid_storage_key,
};

/// Plugin API version understood by this app.
pub const API_VERSION: u32 = 1;
/// Custom URI scheme serving plugin files.
pub const PROTOCOL_SCHEME: &str = "obd-plugin";
/// Reserved first path segment serving the plugin SDK (`/_sdk/obd-plugin.js`).
pub const SDK_SEGMENT: &str = "_sdk";
/// Manifest file name inside each plugin folder.
pub const MANIFEST_FILE: &str = "plugin.json";

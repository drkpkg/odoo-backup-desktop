//! Odoo 15.0–19.0 client used by Odoo Backup Desktop.
//!
//! - [`version`]: detect the server version without credentials.
//! - [`rpc`]: protocol adapters (XML-RPC for ≤18, JSON-2 for ≥19) behind [`OdooRpc`].
//! - [`probe`]: connection diagnostics used by the UI before saving an instance.
//! - [`transport`]: backup transports ([`DbManagerTransport`], [`ObdModuleTransport`]).
//! - [`validate`]: integrity checks of a downloaded backup zip.

pub mod error;
pub mod probe;
pub mod rpc;
pub mod transport;
pub mod validate;
pub mod version;

mod json2;
mod net;
mod xmlrpc;

pub use error::{OdooError, Result};
pub use probe::{
    CheckStatus, MODULE_API_MODEL, MODULE_API_VERSION, ProbeInput, ProbeReport, ProbeWarning, database_from_host, probe,
};
pub use rpc::{
    Credentials, MODEL_NOT_FOUND, OdooRpc, ProtocolPreference, RpcProtocol, SecretKind, connect, select_protocol,
};
pub use transport::{
    BackupPhase, BackupRequest, BackupTransport, DbManagerTransport, DownloadedBackup, ObdModuleTransport, ProgressFn,
    TransportKind,
};
pub use validate::{BackupManifest, validate_backup_zip, validate_backup_zip_cancellable};
pub use version::{OdooVersion, detect_version};

/// Oldest and newest Odoo major versions supported by this client.
pub const SUPPORTED_MAJOR_VERSIONS: std::ops::RangeInclusive<u16> = 15..=19;

//! Storage destinations for finished backups.
//!
//! Every destination implements [`StorageAdapter`]. Retention is applied by
//! [`retention::apply_retention`] on top of `list_backups` + `delete`, never inside
//! an adapter. Adapters: [`local::LocalFolderAdapter`], [`gdrive::GoogleDriveAdapter`].

pub mod adapter;
pub mod error;
pub mod gdrive;
pub mod local;
pub mod retention;

mod util;

pub use adapter::{Capabilities, RemoteObject, StorageAdapter, TargetId, TargetSpec, UploadMeta, UploadProgressFn};
pub use error::{Result, StorageError};
pub use retention::{RetentionPolicy, apply_retention, select_expired};

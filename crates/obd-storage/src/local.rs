//! Local folder destination. The download folder itself uses this adapter for
//! listing and retention (`<root>/<instance-slug>/*.zip`).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::{
    Capabilities, RemoteObject, Result, StorageAdapter, StorageError, TargetId, TargetSpec, UploadMeta,
    UploadProgressFn,
};

const COPY_BUFFER: usize = 1024 * 1024;

pub struct LocalFolderAdapter {
    root: PathBuf,
}

impl LocalFolderAdapter {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory used for an instance: `<root>/<slug(instance_name)>`.
    pub fn instance_dir(&self, spec: &TargetSpec) -> PathBuf {
        self.root.join(slugify(&spec.instance_name))
    }
}

/// Filesystem-safe slug: lowercase ASCII, digits, `-` and `_`.
///
/// Common Latin accents are transliterated (`Compañía` → `compania`), any other
/// character becomes `-`, runs of separators collapse and leading/trailing
/// separators are trimmed. Never returns an empty string.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut pending_separator: Option<char> = None;

    for ch in name.chars().flat_map(char::to_lowercase) {
        let mapped = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => Some('a'),
            'é' | 'è' | 'ë' | 'ê' => Some('e'),
            'í' | 'ì' | 'ï' | 'î' => Some('i'),
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => Some('o'),
            'ú' | 'ù' | 'ü' | 'û' => Some('u'),
            'ñ' => Some('n'),
            'ç' => Some('c'),
            _ => None,
        };
        match mapped {
            Some(c) => {
                if let Some(sep) = pending_separator.take()
                    && !slug.is_empty()
                {
                    slug.push(sep);
                }
                slug.push(c);
            }
            None => {
                // `_` is kept when it is the only separator in a run; anything else is `-`.
                let sep = if ch == '_' && pending_separator.is_none() { '_' } else { '-' };
                pending_separator = Some(sep);
            }
        }
    }

    if slug.is_empty() { "instance".to_owned() } else { slug }
}

fn path_to_id(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| StorageError::Fatal(format!("path is not valid UTF-8: {}", path.display())))
}

fn modified_at(meta: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    meta.modified().ok().map(DateTime::<Utc>::from)
}

#[async_trait]
impl StorageAdapter for LocalFolderAdapter {
    fn provider_id(&self) -> &'static str {
        "local"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { resumable: false, server_side_checksum: false, max_file_size: None }
    }

    async fn ensure_target(&self, spec: &TargetSpec) -> Result<TargetId> {
        let dir = std::path::absolute(self.instance_dir(spec))?;
        tokio::fs::create_dir_all(&dir).await?;
        Ok(TargetId(path_to_id(&dir)?))
    }

    async fn upload(
        &self,
        target: &TargetId,
        file: &Path,
        meta: &UploadMeta,
        progress: UploadProgressFn,
        cancel: CancellationToken,
    ) -> Result<RemoteObject> {
        let file_name = file
            .file_name()
            .ok_or_else(|| StorageError::Fatal(format!("not a file path: {}", file.display())))?
            .to_owned();
        let dest_dir = PathBuf::from(&target.0);
        tokio::fs::create_dir_all(&dest_dir).await?;
        let dest = dest_dir.join(&file_name);

        let source_meta = tokio::fs::metadata(file).await?;
        let total = source_meta.len();

        // The download folder may already be the destination: nothing to copy.
        let same_file = match (tokio::fs::canonicalize(file).await, tokio::fs::canonicalize(&dest).await) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };

        if !same_file {
            let mut tmp_name = file_name.clone();
            tmp_name.push(".part");
            let tmp = dest_dir.join(tmp_name);
            if let Err(err) = copy_with_progress(file, &tmp, total, &progress, &cancel).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(err);
            }
            tokio::fs::rename(&tmp, &dest).await?;
        }
        progress(total, total);

        let dest_meta = tokio::fs::metadata(&dest).await?;
        Ok(RemoteObject {
            id: path_to_id(&dest)?,
            name: file_name.to_string_lossy().into_owned(),
            size: Some(dest_meta.len()),
            created_at: modified_at(&dest_meta),
            instance_id: Some(meta.instance_id.clone()),
            web_link: None,
        })
    }

    async fn list_backups(&self, target: &TargetId) -> Result<Vec<RemoteObject>> {
        let dir = PathBuf::from(&target.0);
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };

        let mut objects = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.to_ascii_lowercase().ends_with(".zip") {
                continue;
            }
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            objects.push(RemoteObject {
                id: path_to_id(&entry.path())?,
                name,
                size: Some(meta.len()),
                created_at: modified_at(&meta),
                instance_id: None,
                web_link: None,
            });
        }
        objects.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.name.cmp(&a.name)));
        Ok(objects)
    }

    async fn delete(&self, object: &RemoteObject) -> Result<()> {
        match tokio::fs::remove_file(&object.id).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Err(StorageError::NotFound(object.id.clone())),
            Err(err) => Err(err.into()),
        }
    }
}

async fn copy_with_progress(
    source: &Path,
    dest: &Path,
    total: u64,
    progress: &UploadProgressFn,
    cancel: &CancellationToken,
) -> Result<()> {
    let mut reader = tokio::fs::File::open(source).await?;
    let mut writer = tokio::fs::File::create(dest).await?;
    let mut buf = vec![0u8; COPY_BUFFER];
    let mut copied = 0u64;

    loop {
        if cancel.is_cancelled() {
            return Err(StorageError::Cancelled);
        }
        let read = tokio::select! {
            read = reader.read(&mut buf) => read?,
            () = cancel.cancelled() => return Err(StorageError::Cancelled),
        };
        if read == 0 {
            break;
        }
        writer.write_all(&buf[..read]).await?;
        copied += read as u64;
        progress(copied, total);
    }
    writer.flush().await?;
    writer.sync_all().await?;
    Ok(())
}

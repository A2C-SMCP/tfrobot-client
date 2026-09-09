use super::*;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(crate) fn resource_filename(name: Option<&str>, uri: &str) -> String {
    let fallback = uri.rsplit('/').next().unwrap_or("attachment");
    let input = name.filter(|name| !name.is_empty()).unwrap_or(fallback);
    let clean: String = input
        .chars()
        .filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c))
        .take(180)
        .collect();
    let clean = clean.trim().trim_matches('.');
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean.into()
    }
}

impl ChatSessionService {
    /// Only the native command chooses the destination (save dialog or private temporary file).
    pub async fn save_resource(
        &self,
        lease_id: &str,
        id: &str,
        destination: &Path,
    ) -> Result<(), ChatResourceError> {
        let entry = self.resource_entry(lease_id, id)?;
        let parent = destination.parent().ok_or(ChatResourceError::Save)?;
        let temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| ChatResourceError::Save)?;
        self.write_resource(&entry, &temporary).await?;
        // Serialize final publication against handle/lease revocation. No partially downloaded file
        // replaces the user's destination, even on a late response after cancellation.
        let lease = self.lease(lease_id).await?;
        let current = lease.lock().await;
        let entries = self.resources.entries.lock().unwrap();
        if *current.cancelled.borrow() || !entries.contains_key(id) {
            return Err(ChatResourceError::Cancelled);
        }
        temporary
            .persist(destination)
            .map_err(|_| ChatResourceError::Save)?;
        Ok(())
    }

    pub async fn open_resource(
        &self,
        lease_id: &str,
        id: &str,
        name: &str,
        open: impl FnOnce(&Path) -> Result<(), ChatResourceError>,
    ) -> Result<(), ChatResourceError> {
        let extension = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // Never automatically execute downloaded applications, scripts, HTML or desktop shortcuts.
        if !matches!(
            extension.as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "avif"
                | "pdf"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "zip"
                | "mp3"
                | "m4a"
                | "wav"
                | "ogg"
                | "mp4"
                | "mov"
                | "webm"
        ) {
            return Err(ChatResourceError::Unsupported);
        }
        let entry = self.resource_entry(lease_id, id)?;
        {
            let opened = self.resources.opened.lock().unwrap();
            if opened.get(lease_id).is_some_and(|paths| paths.len() >= 32) {
                return Err(ChatResourceError::Busy);
            }
        }
        let temporary = tempfile::Builder::new()
            .prefix("chat-attachment-")
            .suffix(&format!(".{extension}"))
            .tempfile()
            .map_err(|_| ChatResourceError::Save)?;
        self.write_resource(&entry, &temporary).await?;
        let lease = self.lease(lease_id).await?;
        let current = lease.lock().await;
        let entries = self.resources.entries.lock().unwrap();
        if *current.cancelled.borrow() || !entries.contains_key(id) {
            return Err(ChatResourceError::Cancelled);
        }
        let mut opened = self.resources.opened.lock().unwrap();
        let paths = opened.entry(lease_id.into()).or_default();
        if paths.len() >= 32 {
            return Err(ChatResourceError::Busy);
        }
        open(temporary.path())?;
        paths.push(temporary.into_temp_path());
        Ok(())
    }

    pub async fn resource_file_name(
        &self,
        lease_id: &str,
        id: &str,
        name: Option<&str>,
    ) -> Result<String, ChatResourceError> {
        let entry = self.resource_entry(lease_id, id)?;
        self.resource_context(&entry).await?;
        Ok(resource_filename(name, &entry.uri))
    }

    pub async fn await_resource_destination(
        &self,
        lease_id: &str,
        id: &str,
        selected: impl Future<Output = Result<Option<PathBuf>, ChatResourceError>>,
    ) -> Result<Option<PathBuf>, ChatResourceError> {
        let entry = self.resource_entry(lease_id, id)?;
        let (_, cancellation) = self.resource_context(&entry).await?;
        cancellation.run(selected).await
    }

    async fn write_resource(
        &self,
        entry: &ResourceEntry,
        temporary: &tempfile::NamedTempFile,
    ) -> Result<(), ChatResourceError> {
        let mut stream = self.resource_stream(entry, None).await?;
        if stream.response.status() != reqwest::StatusCode::OK {
            return Err(ChatResourceError::Network);
        }
        let file = temporary.reopen().map_err(|_| ChatResourceError::Save)?;
        let mut file = tokio::fs::File::from_std(file);
        while let Some(bytes) = stream.chunk().await? {
            stream
                .cancellation
                .run(async {
                    file.write_all(&bytes)
                        .await
                        .map_err(|_| ChatResourceError::Save)
                })
                .await?;
        }
        stream
            .cancellation
            .run(async { file.flush().await.map_err(|_| ChatResourceError::Save) })
            .await?;
        Ok(())
    }
}

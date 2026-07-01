use crate::{SessionStore, StoredAttachment};
use anyhow::{Context, Result};
use std::path::PathBuf;
use uuid::Uuid;

/// Availability of a staged attachment's backing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentState {
    /// The stored file exists.
    Available,
    /// The stored file is missing.
    Missing,
}

/// Bytes returned for an available attachment preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPreviewBytes {
    /// MIME type associated with the stored file.
    pub mime_type: String,
    /// Raw bytes read from storage.
    pub bytes: Vec<u8>,
}

/// Input used when staging one chat attachment into the session store.
#[derive(Debug, Clone)]
pub struct StageAttachmentInput {
    /// Original display filename.
    pub name: String,
    /// Browser-provided MIME type, normalized by the caller.
    pub mime_type: String,
    /// Uppercase display extension.
    pub extension: String,
    /// Attachment kind.
    pub kind: crate::StoredAttachmentKind,
    /// File bytes to store.
    pub bytes: Vec<u8>,
}

impl SessionStore {
    /// Stages one chat attachment and returns durable metadata.
    pub fn stage_attachment(
        &self,
        session_id: Uuid,
        input: StageAttachmentInput,
    ) -> Result<StoredAttachment> {
        let id = Uuid::new_v4().to_string();
        let dir = self.attachment_dir(session_id, &id)?;
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create attachment dir {}", dir.display()))?;

        let original_tmp = dir.join("original.tmp");
        let original = dir.join("original");
        std::fs::write(&original_tmp, &input.bytes)
            .with_context(|| format!("failed to write attachment {}", original_tmp.display()))?;
        std::fs::rename(&original_tmp, &original)
            .with_context(|| format!("failed to stage attachment {}", original.display()))?;

        let attachment = StoredAttachment {
            id: id.clone(),
            name: sanitize_attachment_name(&input.name),
            mime_type: input.mime_type,
            size: input.bytes.len() as u64,
            extension: input.extension,
            kind: input.kind,
            storage_key: format!("{id}/original"),
        };
        let metadata_tmp = dir.join("metadata.json.tmp");
        let metadata = dir.join("metadata.json");
        std::fs::write(&metadata_tmp, serde_json::to_vec(&attachment)?).with_context(|| {
            format!(
                "failed to write attachment metadata {}",
                metadata_tmp.display()
            )
        })?;
        std::fs::rename(&metadata_tmp, &metadata).with_context(|| {
            format!("failed to stage attachment metadata {}", metadata.display())
        })?;
        Ok(attachment)
    }

    /// Loads staged attachment metadata by id.
    pub fn load_staged_attachments(
        &self,
        session_id: Uuid,
        ids: &[String],
    ) -> Result<Vec<StoredAttachment>> {
        ids.iter()
            .map(|id| {
                validate_attachment_id(id)?;
                let path = self.attachment_dir(session_id, id)?.join("metadata.json");
                let bytes = std::fs::read(&path).with_context(|| {
                    format!("failed to read attachment metadata {}", path.display())
                })?;
                let attachment: StoredAttachment = serde_json::from_slice(&bytes)?;
                if attachment.id != *id {
                    anyhow::bail!("attachment metadata id mismatch for {id}");
                }
                Ok(attachment)
            })
            .collect()
    }

    /// Returns whether the stored original exists.
    pub fn attachment_state(
        &self,
        session_id: Uuid,
        attachment: &StoredAttachment,
    ) -> AttachmentState {
        if self
            .attachment_original_path(session_id, attachment)
            .exists()
        {
            AttachmentState::Available
        } else {
            AttachmentState::Missing
        }
    }

    /// Reads preview bytes for an available attachment.
    pub fn read_attachment_preview(
        &self,
        session_id: Uuid,
        attachment: &StoredAttachment,
    ) -> Result<AttachmentPreviewBytes> {
        validate_attachment_id(&attachment.id)?;
        let path = self.attachment_original_path(session_id, attachment);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read attachment {}", path.display()))?;
        Ok(AttachmentPreviewBytes {
            mime_type: attachment.mime_type.clone(),
            bytes,
        })
    }

    /// Returns the original file path for a stored attachment.
    pub fn attachment_original_path(
        &self,
        session_id: Uuid,
        attachment: &StoredAttachment,
    ) -> PathBuf {
        validate_attachment_id(&attachment.id)
            .and_then(|()| self.attachment_dir(session_id, &attachment.id))
            .unwrap_or_else(|_| self.root().join(".invalid-attachment-id"))
            .join("original")
    }

    fn attachment_dir(&self, session_id: Uuid, attachment_id: &str) -> Result<PathBuf> {
        validate_attachment_id(attachment_id)?;
        Ok(self
            .root()
            .join(format!("{session_id}.attachments"))
            .join(attachment_id))
    }
}

fn validate_attachment_id(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).context("attachment id must be a UUID")?;
    if parsed.to_string() != id {
        anyhow::bail!("attachment id must be canonical UUID");
    }
    Ok(())
}

fn sanitize_attachment_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.is_empty() {
        "attachment".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredAttachmentKind;
    use puffer_config::ConfigPaths;
    use tempfile::tempdir;

    #[test]
    fn stage_attachment_writes_sidecar_and_reports_available() {
        let temp = tempdir().unwrap();
        let paths = ConfigPaths::discover(temp.path());
        let store = SessionStore::from_paths(&paths).unwrap();
        let session = store.create_session(temp.path().to_path_buf()).unwrap();

        let attachment = store
            .stage_attachment(
                session.id,
                StageAttachmentInput {
                    name: "pixel.png".to_string(),
                    mime_type: "image/png".to_string(),
                    extension: "PNG".to_string(),
                    kind: StoredAttachmentKind::Image,
                    bytes: vec![1, 2, 3],
                },
            )
            .unwrap();

        assert_eq!(attachment.name, "pixel.png");
        assert_eq!(
            store.attachment_state(session.id, &attachment),
            AttachmentState::Available
        );
        assert_eq!(
            store
                .read_attachment_preview(session.id, &attachment)
                .unwrap()
                .bytes,
            vec![1, 2, 3]
        );
    }

    #[test]
    fn missing_original_reports_missing() {
        let temp = tempdir().unwrap();
        let paths = ConfigPaths::discover(temp.path());
        let store = SessionStore::from_paths(&paths).unwrap();
        let session = store.create_session(temp.path().to_path_buf()).unwrap();
        let attachment = store
            .stage_attachment(
                session.id,
                StageAttachmentInput {
                    name: "lost.png".to_string(),
                    mime_type: "image/png".to_string(),
                    extension: "PNG".to_string(),
                    kind: StoredAttachmentKind::Image,
                    bytes: vec![4, 5, 6],
                },
            )
            .unwrap();

        std::fs::remove_file(store.attachment_original_path(session.id, &attachment)).unwrap();

        assert_eq!(
            store.attachment_state(session.id, &attachment),
            AttachmentState::Missing
        );
        assert!(store
            .read_attachment_preview(session.id, &attachment)
            .is_err());
    }

    #[test]
    fn load_staged_attachments_rejects_invalid_ids() {
        let temp = tempdir().unwrap();
        let paths = ConfigPaths::discover(temp.path());
        let store = SessionStore::from_paths(&paths).unwrap();
        let session = store.create_session(temp.path().to_path_buf()).unwrap();

        assert!(store
            .load_staged_attachments(session.id, &["../bad".to_string()])
            .is_err());
    }

    #[test]
    fn load_staged_attachments_rejects_metadata_id_mismatch() {
        let temp = tempdir().unwrap();
        let paths = ConfigPaths::discover(temp.path());
        let store = SessionStore::from_paths(&paths).unwrap();
        let session = store.create_session(temp.path().to_path_buf()).unwrap();
        let mut attachment = store
            .stage_attachment(
                session.id,
                StageAttachmentInput {
                    name: "pixel.png".to_string(),
                    mime_type: "image/png".to_string(),
                    extension: "PNG".to_string(),
                    kind: StoredAttachmentKind::Image,
                    bytes: vec![1, 2, 3],
                },
            )
            .unwrap();
        let requested_id = attachment.id.clone();
        let metadata_path = store
            .attachment_original_path(session.id, &attachment)
            .with_file_name("metadata.json");

        attachment.id = Uuid::new_v4().to_string();
        std::fs::write(metadata_path, serde_json::to_vec(&attachment).unwrap()).unwrap();

        assert!(store
            .load_staged_attachments(session.id, &[requested_id])
            .is_err());
    }

    #[test]
    fn delete_session_removes_attachment_sidecar_directory() {
        let temp = tempdir().unwrap();
        let paths = ConfigPaths::discover(temp.path());
        let store = SessionStore::from_paths(&paths).unwrap();
        let session = store.create_session(temp.path().to_path_buf()).unwrap();
        let attachment = store
            .stage_attachment(
                session.id,
                StageAttachmentInput {
                    name: "pixel.png".to_string(),
                    mime_type: "image/png".to_string(),
                    extension: "PNG".to_string(),
                    kind: StoredAttachmentKind::Image,
                    bytes: vec![1, 2, 3],
                },
            )
            .unwrap();
        let attachment_dir = store
            .attachment_original_path(session.id, &attachment)
            .parent()
            .unwrap()
            .to_path_buf();

        assert!(attachment_dir.exists());
        store.delete_session(session.id).unwrap();
        assert!(!attachment_dir.exists());
    }
}

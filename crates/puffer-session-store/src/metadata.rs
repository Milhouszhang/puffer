use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Stores persisted metadata for one session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub id: Uuid,
    pub display_name: Option<String>,
    #[serde(default)]
    pub generated_title: Option<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub parent_session_id: Option<Uuid>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Represents a loaded session and its transcript events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub metadata: SessionMetadata,
    pub events: Vec<crate::TranscriptEvent>,
}

/// Summarizes a session for picker or listing UIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: Uuid,
    pub display_name: Option<String>,
    #[serde(default)]
    pub generated_title: Option<String>,
    pub cwd: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub event_count: usize,
    pub parent_session_id: Option<Uuid>,
    pub slug: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
}

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeScope {
    Shared,
    GhostPrivate,
    GhostProjects,
    GhostDiary,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeContext {
    pub ghost_name: String,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub query: String,
    pub scope: MemoryScope,
    #[serde(default)]
    pub options: SearchOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceQuery {
    pub topic: String,
    pub question: String,
    #[serde(default)]
    pub options: SearchOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum MemoryScope {
    #[default]
    All,
    SharedOnly,
    GhostOnly,
    GhostPrivate,
    GhostProjects,
    GhostDiary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchOptions {
    pub max_results: Option<usize>,
    pub graph_depth: Option<u8>,
    pub graph_max: Option<usize>,
    pub bm25_limit: Option<usize>,
    pub dense_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub note_type: String,
    pub path: PathBuf,
    pub scope: KnowledgeScope,
    pub trust_score: i64,
    pub score: f32,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub summary: NoteSummary,
    pub parents: Vec<NoteSummary>,
    pub links_out: Vec<NoteSummary>,
    pub links_in: Vec<NoteSummary>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteDocument {
    pub id: String,
    pub title: String,
    pub note_type: String,
    pub path: PathBuf,
    pub scope: KnowledgeScope,
    pub trust_score: i64,
    pub created_at: DateTime<Utc>,
    pub created_by_ghost: String,
    pub created_by_model: String,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_validated_by_ghost: Option<String>,
    pub last_validated_by_model: Option<String>,
    pub version: Option<i64>,
    pub parent_id: Option<String>,
    pub comments_json: Option<String>,
    pub body: String,
}

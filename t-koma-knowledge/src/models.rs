use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeScope {
    Shared,
    GhostPrivate,
    GhostProjects,
    GhostDiary,
    Reference,
}

impl KnowledgeScope {
    /// Database string representation of this scope.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::GhostPrivate => "ghost_private",
            Self::GhostProjects => "ghost_projects",
            Self::GhostDiary => "ghost_diary",
            Self::Reference => "reference",
        }
    }

    /// Whether this scope is shared (owner_ghost IS NULL in DB).
    pub fn is_shared(&self) -> bool {
        matches!(self, Self::Shared | Self::Reference)
    }
}

impl FromStr for KnowledgeScope {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "shared" => Ok(Self::Shared),
            "ghost_private" => Ok(Self::GhostPrivate),
            "ghost_projects" => Ok(Self::GhostProjects),
            "ghost_diary" => Ok(Self::GhostDiary),
            "reference" => Ok(Self::Reference),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for KnowledgeScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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

/// Scope for write operations (create, capture).
///
/// Unlike `MemoryScope` (which is for reads/queries and includes `All` / `GhostOnly`),
/// `WriteScope` forces callers to pick a concrete destination.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WriteScope {
    #[default]
    Private,
    Projects,
    Diary,
    Shared,
}

/// Input for creating a new note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteCreateRequest {
    pub title: String,
    pub note_type: String,
    pub scope: WriteScope,
    pub body: String,
    pub parent: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<Vec<String>>,
    pub trust_score: Option<i64>,
}

/// Input for updating an existing note.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NoteUpdateRequest {
    pub note_id: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    pub trust_score: Option<i64>,
    pub parent: Option<String>,
}

/// Result of a note create/update operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteWriteResult {
    pub note_id: String,
    pub path: PathBuf,
}

// ── Reference topic models ──────────────────────────────────────────

/// Status of a reference topic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TopicStatus {
    #[default]
    Active,
    Stale,
    Obsolete,
}

impl TopicStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Obsolete => "obsolete",
        }
    }
}

impl std::fmt::Display for TopicStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TopicStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "stale" => Ok(Self::Stale),
            "obsolete" => Ok(Self::Obsolete),
            other => Err(format!("unknown topic status: {}", other)),
        }
    }
}

/// Source descriptor for a reference topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSourceInput {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub ref_name: Option<String>,
    pub paths: Option<Vec<String>>,
}

/// Input for creating a new reference topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicCreateRequest {
    pub title: String,
    pub body: String,
    pub sources: Vec<TopicSourceInput>,
    pub tags: Option<Vec<String>>,
    pub max_age_days: Option<i64>,
    pub trust_score: Option<i64>,
}

/// Result of a successful topic creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicCreateResult {
    pub topic_id: String,
    pub source_count: usize,
    pub file_count: usize,
    pub chunk_count: usize,
}

/// Entry in a topic listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicListEntry {
    pub topic_id: String,
    pub title: String,
    pub status: String,
    pub is_stale: bool,
    pub fetched_at: Option<DateTime<Utc>>,
    pub max_age_days: i64,
    pub created_by_ghost: String,
    pub source_count: usize,
    pub file_count: usize,
    pub tags: Vec<String>,
}

/// Result of a topic search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSearchResult {
    pub topic_id: String,
    pub title: String,
    pub status: String,
    pub is_stale: bool,
    pub fetched_at: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub score: f32,
    pub snippet: String,
}

/// Input for updating an existing reference topic's metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopicUpdateRequest {
    pub topic_id: String,
    pub status: Option<String>,
    pub max_age_days: Option<i64>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Generate a stable note ID.
pub fn generate_note_id() -> String {
    Uuid::new_v4().to_string()
}

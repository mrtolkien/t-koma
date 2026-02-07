use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Storage scope for knowledge artifacts.
///
/// The scope determines both the filesystem location and the DB ownership
/// constraint (owner_ghost IS NULL for shared scopes, NOT NULL for ghost scopes).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeScope {
    SharedNote,
    SharedReference,
    GhostNote,
    GhostReference,
    GhostDiary,
}

impl KnowledgeScope {
    /// Database string representation of this scope.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SharedNote => "shared_note",
            Self::SharedReference => "shared_reference",
            Self::GhostNote => "ghost_note",
            Self::GhostReference => "ghost_reference",
            Self::GhostDiary => "ghost_diary",
        }
    }

    /// Whether this scope is shared (owner_ghost IS NULL in DB).
    pub fn is_shared(&self) -> bool {
        matches!(self, Self::SharedNote | Self::SharedReference)
    }

    /// Whether this scope holds reference topics/files.
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::SharedReference | Self::GhostReference)
    }

    /// Whether this scope holds structured notes (with front matter).
    pub fn is_note(&self) -> bool {
        matches!(self, Self::SharedNote | Self::GhostNote)
    }
}

impl FromStr for KnowledgeScope {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "shared_note" => Ok(Self::SharedNote),
            "shared_reference" => Ok(Self::SharedReference),
            "ghost_note" => Ok(Self::GhostNote),
            "ghost_reference" => Ok(Self::GhostReference),
            "ghost_diary" => Ok(Self::GhostDiary),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for KnowledgeScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Ownership scope for knowledge queries.
///
/// Controls whether a query targets shared, private (ghost-owned), or both.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OwnershipScope {
    /// Search shared + private.
    #[default]
    All,
    /// Only shared artifacts.
    Shared,
    /// Only ghost-owned artifacts.
    Private,
}

/// Query for searching notes (shared + ghost).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteQuery {
    pub query: String,
    pub scope: OwnershipScope,
    #[serde(default)]
    pub options: SearchOptions,
}

/// Query for searching within a reference topic's files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceQuery {
    pub topic: String,
    pub question: String,
    #[serde(default)]
    pub options: SearchOptions,
}

/// Query for searching diary entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryQuery {
    pub query: String,
    #[serde(default)]
    pub options: SearchOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchOptions {
    pub max_results: Option<usize>,
    pub graph_depth: Option<u8>,
    pub graph_max: Option<usize>,
    pub bm25_limit: Option<usize>,
    pub dense_limit: Option<usize>,
    /// Boost multiplier for documentation files in reference search. Overrides config default.
    pub doc_boost: Option<f32>,
}

// ── Unified knowledge query models ─────────────────────────────────

/// Category of knowledge to search.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SearchCategory {
    Notes,
    Diary,
    References,
    Topics,
}

impl SearchCategory {
    /// All categories in default search order.
    pub fn all() -> Vec<Self> {
        vec![Self::Notes, Self::Diary, Self::References, Self::Topics]
    }
}

/// Unified query for searching across all knowledge subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSearchQuery {
    pub query: String,
    /// Categories to search. `None` = all.
    pub categories: Option<Vec<SearchCategory>>,
    #[serde(default)]
    pub scope: OwnershipScope,
    /// Narrow reference search to a specific topic.
    pub topic: Option<String>,
    #[serde(default)]
    pub options: SearchOptions,
}

/// Unified search result grouped by category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSearchResult {
    pub notes: Vec<NoteResult>,
    pub diary: Vec<DiarySearchResult>,
    pub references: ReferenceSearchOutput,
    pub topics: Vec<TopicSearchResult>,
}

/// Reference search output with optional matched topic context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSearchOutput {
    /// Present when a `topic` parameter was used and matched.
    pub matched_topic: Option<MatchedTopic>,
    pub results: Vec<NoteResult>,
}

/// Matched topic metadata for contextual enrichment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedTopic {
    pub topic_id: String,
    pub title: String,
    pub body: String,
}

/// Unified retrieval query — fetch by ID or by topic + path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGetQuery {
    pub id: Option<String>,
    pub topic: Option<String>,
    pub path: Option<String>,
    pub max_chars: Option<usize>,
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
pub struct NoteResult {
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

/// Scope for write operations (create note).
///
/// Forces callers to pick a concrete destination.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WriteScope {
    /// Ghost-owned note (default).
    #[default]
    GhostNote,
    /// Shared note visible to all ghosts.
    SharedNote,
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

/// Role of a reference source: documentation or code.
///
/// Determines the `note_type` stored in the DB (`ReferenceDocs` vs `ReferenceCode`)
/// and influences search ranking via `doc_boost`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceRole {
    #[default]
    Code,
    Docs,
}

impl SourceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Docs => "docs",
        }
    }

    /// Infer role from source type if not explicitly set.
    pub fn infer(source_type: &str) -> Self {
        match source_type {
            "web" => Self::Docs,
            _ => Self::Code,
        }
    }

    /// Map to the reference note_type for DB storage.
    pub fn to_note_type(&self) -> &'static str {
        match self {
            Self::Docs => "ReferenceDocs",
            Self::Code => "ReferenceCode",
        }
    }
}

impl std::fmt::Display for SourceRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SourceRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "code" => Ok(Self::Code),
            "docs" => Ok(Self::Docs),
            other => Err(format!("unknown source role: {}", other)),
        }
    }
}

/// Status of an individual reference file within a topic.
///
/// Controls search ranking and filtering:
/// - **Active**: normal ranking
/// - **Problematic**: included but penalized (0.5x score)
/// - **Obsolete**: excluded from search entirely
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceFileStatus {
    #[default]
    Active,
    /// Partially wrong — check topic notes for caveats.
    Problematic,
    /// Completely wrong / outdated — excluded from search.
    Obsolete,
}

impl ReferenceFileStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Problematic => "problematic",
            Self::Obsolete => "obsolete",
        }
    }
}

impl std::fmt::Display for ReferenceFileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ReferenceFileStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "problematic" => Ok(Self::Problematic),
            "obsolete" => Ok(Self::Obsolete),
            other => Err(format!("unknown reference file status: {}", other)),
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
    /// Role of the source content (docs vs code). Inferred from source_type if not set.
    pub role: Option<SourceRole>,
    /// Max link-hop depth for crawl sources (default 1, max 3).
    pub max_depth: Option<u8>,
    /// Max pages to fetch for crawl sources (default 20, max 100).
    pub max_pages: Option<usize>,
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

/// Summary of a collection within a topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub title: String,
    pub path: String,
    pub file_count: usize,
}

/// Entry in a topic listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicListEntry {
    pub topic_id: String,
    pub title: String,
    pub created_by_ghost: String,
    pub file_count: usize,
    pub collections: Vec<CollectionSummary>,
    pub tags: Vec<String>,
}

/// Result of a topic search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSearchResult {
    pub topic_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub score: f32,
    pub snippet: String,
}

/// Input for updating an existing reference topic's metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopicUpdateRequest {
    pub topic_id: String,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
}

// ── Reference save models ──────────────────────────────────────────

/// Input for saving content to a reference topic.
///
/// Creates topic and collection implicitly if they don't exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSaveRequest {
    /// Topic name (fuzzy-matched against existing topics).
    pub topic: String,
    /// Relative path within the topic directory (e.g. "bambulab-a1/specs.md").
    pub path: String,
    /// Content to write.
    pub content: String,
    /// Source URL for provenance tracking.
    pub source_url: Option<String>,
    /// Role of the content (docs vs code). Default: docs.
    pub role: Option<SourceRole>,
    /// Title for the file note.
    pub title: Option<String>,
    /// Title for auto-created collection.
    pub collection_title: Option<String>,
    /// Description for auto-created collection.
    pub collection_description: Option<String>,
    /// Tags for auto-created collection.
    pub collection_tags: Option<Vec<String>>,
    /// Tags for auto-created topic.
    pub tags: Option<Vec<String>>,
    /// Description for auto-created topic.
    pub topic_description: Option<String>,
}

/// Result of a reference_save operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSaveResult {
    pub topic_id: String,
    pub note_id: String,
    pub path: String,
    pub created_topic: bool,
    pub created_collection: bool,
}

/// Result of a `reference_search` query, including full topic context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSearchResult {
    /// Full topic.md body — context for the LLM.
    pub topic_body: String,
    pub topic_title: String,
    pub topic_id: String,
    /// Ranked file chunks.
    pub results: Vec<NoteResult>,
}

/// Result of a diary search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarySearchResult {
    pub date: String,
    pub score: f32,
    pub snippet: String,
    pub note_id: String,
}

/// Generate a stable note ID.
pub fn generate_note_id() -> String {
    Uuid::new_v4().to_string()
}

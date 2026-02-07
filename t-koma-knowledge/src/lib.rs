//! Knowledge & memory subsystem for T-KOMA.

pub mod chunker;
pub mod embeddings;
pub mod engine;
pub mod errors;
pub mod graph;
pub mod index;
pub mod ingest;
pub mod models;
pub mod parser;
pub mod paths;
pub mod sources;
pub mod storage;
pub mod watcher;

pub use embeddings::EmbeddingClient;
pub use engine::KnowledgeEngine;
pub use errors::KnowledgeError;
pub use models::{
    CollectionSummary, DiaryQuery, DiarySearchResult, KnowledgeGetQuery, KnowledgeScope,
    KnowledgeSearchQuery, KnowledgeSearchResult, MatchedTopic, NoteCreateRequest, NoteDocument,
    NoteQuery, NoteResult, NoteSummary, NoteUpdateRequest, NoteWriteResult, OwnershipScope,
    ReferenceFileStatus, ReferenceQuery, ReferenceSaveRequest, ReferenceSaveResult,
    ReferenceSearchOutput, ReferenceSearchResult, SearchCategory, SourceRole, TopicCreateRequest,
    TopicCreateResult, TopicListEntry, TopicSearchResult, TopicSourceInput, TopicUpdateRequest,
    WriteScope,
};
pub use t_koma_core::config::{KnowledgeSettings, SearchDefaults};

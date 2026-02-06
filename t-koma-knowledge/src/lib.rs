//! Knowledge & memory subsystem for T-KOMA.

pub mod embeddings;
pub mod errors;
pub mod graph;
pub mod index;
pub mod ingest;
pub mod models;
pub mod parser;
pub mod paths;
pub mod engine;
pub mod sources;
pub mod storage;
pub mod chunker;
pub mod watcher;

pub use t_koma_core::config::{KnowledgeSettings, SearchDefaults};
pub use errors::KnowledgeError;
pub use models::{
    DiaryQuery, DiarySearchResult, KnowledgeScope, NoteCreateRequest, NoteDocument, NoteQuery,
    NoteResult, NoteSearchScope, NoteSummary, NoteUpdateRequest, NoteWriteResult,
    ReferenceFileStatus, ReferenceQuery, ReferenceSearchResult, SourceRole, TopicCreateRequest,
    TopicCreateResult, TopicListEntry, TopicSearchResult, TopicSourceInput, TopicStatus,
    TopicUpdateRequest, WriteScope,
};
pub use engine::KnowledgeEngine;
pub use embeddings::EmbeddingClient;

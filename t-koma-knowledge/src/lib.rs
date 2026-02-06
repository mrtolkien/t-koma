//! Knowledge & memory subsystem for T-KOMA.

pub mod embeddings;
pub mod errors;
pub mod graph;
pub mod index;
pub mod ingest;
pub mod models;
pub mod parser;
pub mod paths;
pub mod search;
pub mod storage;
pub mod chunker;
pub mod watcher;

pub use t_koma_core::config::{KnowledgeSettings, SearchDefaults};
pub use errors::KnowledgeError;
pub use models::{
    KnowledgeScope, MemoryQuery, MemoryResult, NoteCreateRequest, NoteDocument, NoteSummary,
    NoteUpdateRequest, NoteWriteResult, ReferenceQuery, WriteScope,
};
pub use search::KnowledgeEngine;
pub use embeddings::EmbeddingClient;

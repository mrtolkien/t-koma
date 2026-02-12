//! t-koma-db: Database layer with sqlite-vec support for T-KOMA/GHOST storage.
//!
//! This crate provides database operations for:
//! - Operator approval/denial workflows
//! - Ghost registry and session/message storage
//! - Platform-specific handling (Discord, API, CLI)
//! - Audit trail via event logging

pub mod error;
pub mod ghosts;
pub mod interfaces;
pub mod job_logs;
pub mod koma_db;
pub mod operators;
pub mod prompt_cache;
pub mod sessions;
mod sqlite_runtime;
pub mod usage_log;

// Re-export commonly used types
pub use error::{DbError, DbResult};
pub use ghosts::{Ghost, GhostRepository};
pub use interfaces::{Interface, InterfaceRepository};
pub use job_logs::{
    JobKind, JobLog, JobLogRepository, JobLogSummary, TodoItem, TodoStatus, TranscriptEntry,
};
pub use koma_db::KomaDbPool;
pub use operators::{
    DEFAULT_RATE_LIMIT_1H_MAX, DEFAULT_RATE_LIMIT_5M_MAX, Operator, OperatorAccessLevel,
    OperatorRepository, OperatorStatus, Platform,
};
pub use prompt_cache::{PromptCacheEntry, PromptCacheRepository};
pub use sessions::{ContentBlock, Message, MessageRole, Session, SessionInfo, SessionRepository};
pub use usage_log::{TokenUsage, UsageLog, UsageLogRepository, UsageTotals};

// Re-export test helpers when running tests or when test-helpers feature is enabled
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

#[cfg(test)]
pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

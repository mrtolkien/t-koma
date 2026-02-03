//! t-koma-db: Database layer with sqlite-vec support for T-KOMA/GHOST storage.
//!
//! This crate provides database operations for:
//! - Operator approval/denial workflows
//! - Ghost registry and per-ghost session/message storage
//! - Platform-specific handling (Discord, API, CLI)
//! - Audit trail via event logging

pub mod error;
pub mod ghost_db;
pub mod ghosts;
pub mod interfaces;
pub mod koma_db;
pub mod operators;
pub mod sessions;

// Re-export commonly used types
pub use error::{DbError, DbResult};
pub use ghost_db::GhostDbPool;
pub use ghosts::{Ghost, GhostRepository};
pub use interfaces::{Interface, InterfaceRepository};
pub use koma_db::KomaDbPool;
pub use operators::{Operator, OperatorRepository, OperatorStatus, Platform};
pub use sessions::{ContentBlock, Message, MessageRole, Session, SessionInfo, SessionRepository};

// Re-export test helpers when running tests or when test-helpers feature is enabled
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

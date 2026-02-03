//! t-koma-db: Database layer with sqlite-vec support for user management.
//!
//! This crate provides database operations for user management including:
//! - User approval/denial workflows
//! - Pending user tracking with auto-pruning
//! - Platform-specific handling (Discord, API, CLI)
//! - Audit trail via event logging

pub mod db;
pub mod error;
pub mod sessions;
pub mod users;

// Re-export commonly used types
pub use db::DbPool;
pub use error::{DbError, DbResult};
pub use sessions::{ContentBlock, Message, MessageRole, Session, SessionInfo, SessionRepository};
pub use users::{Platform, User, UserRepository, UserStatus};

// Re-export test helpers when running tests
#[cfg(test)]
pub use db::test_helpers;

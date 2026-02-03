//! Conversation tests using the full gateway stack.
//!
//! These tests exercise the complete flow through AppState, including:
//! - Database persistence
//! - Session management
//! - Tool use with state
//! - Multi-turn conversations
//!
//! Run with: cargo test --features live-tests conversation

pub mod file_operations;
pub mod multi_turn;
pub mod skill_usage;

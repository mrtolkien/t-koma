//! Client tests for direct default-provider API interaction.
//!
//! These tests exercise the configured provider client directly without the full gateway
//! stack (no database, no AppState).
//!
//! Run with: cargo test --features live-tests client

pub mod basic_queries;
pub mod tool_use;

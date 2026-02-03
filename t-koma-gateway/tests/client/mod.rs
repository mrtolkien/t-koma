//! Client tests for direct Anthropic API interaction.
//!
//! These tests exercise the AnthropicClient directly without the full gateway
//! stack (no database, no AppState).
//!
//! Run with: cargo test --features live-tests client

pub mod basic_queries;
pub mod tool_use;

//! Snapshot tests for the t-koma gateway.
//!
//! Run with: cargo test --features live-tests
//!
//! These tests capture real API responses (with insta redactions to handle
//! dynamic fields like `id`). Review the `.snap` files to see actual API output.
//!
//! **IMPORTANT**: These tests should only be run by human developers, not AI
//! agents, as they require snapshot review and API access.

// Client tests - direct Anthropic API interaction
mod client;

// Conversation tests - full gateway stack with database
mod conversation;

# t-koma AGENTS.md

This document provides essential information for AI coding agents working on the
t-koma project.

## CRUCIAL

You should ALWAYS EDIT THIS FILE if:

- Any changes you make change the structure or features defined here
- You see outdated information in this file
- You make assumptions about a library that we use that turns out to be wrong

Default to editing this file more often than not. This is your memory. This is
how you improve.

## Development flow

- Always start by creating a markdown spec in `./vibe/specs` for validation by
  the user.
- After validation, create an append-only tracking file in `./vibe/ongoing`
  (e.g., `./vibe/ongoing/feature_name.md`). Never continue without the user's
  validation.
- Update the ongoing file at each step after thinking and while editing files.
- Iterate until all the steps and features outlined in the spec are developed
  and, if realistic, tested.
- cargo clippy and cargo test should pass
- Finally, rename the spec file to have a leading underscore (e.g.,
  `_feature_name.md`) to indicate completion.

## Project Overview

...

### t-koma-core

Core library with shared types and configuration:

- `src/config.rs`: Environment configuration (`Config` struct)
- `src/message.rs`: WebSocket message types (`WsMessage`, `WsResponse`)
- `src/persistent_config.rs`: Legacy TOML-based config (deprecated in favor of
  DB)
- `src/pending_users.rs`: Legacy TOML-based pending users (deprecated in favor
  of DB)

### t-koma-db

Database layer using SQLite with sqlite-vec extension:

- `src/db.rs`: Database pool initialization and connection management
- `src/users.rs`: User management (`UserRepository`, `User`, `UserStatus`,
  `Platform`)
- `src/error.rs`: Database error types (`DbError`)
- `migrations/001_initial_schema.sql`: Database schema

**Key Types:**

- `DbPool`: Database connection pool, initialize with `DbPool::new().await`
- `UserRepository`: Static methods for user CRUD operations
- `UserStatus`: `Pending`, `Approved`, `Denied`
- `Platform`: `Discord`, `Api`, `Cli`

**Database Location:** Platform-specific data directory:

- Linux: `~/.local/share/t-koma/db.sqlite3`
- macOS: `~/Library/Application Support/t-koma/db.sqlite3`
- Windows: `%APPDATA%\t-koma\db.sqlite3`

### t-koma-gateway

Gateway server with both library and binary targets:

- `src/main.rs`: Entry point, initializes tracing, creates Anthropic client,
  initializes database, optionally starts Discord bot
- `src/server.rs`: HTTP routes (`/health`, `/chat`), WebSocket handlers (`/ws`,
  `/logs`). **All routes check user approval status via database**
- `src/models/`: Model provider implementations (e.g., `anthropic.rs`)
- `src/tools/`: Model-agnostic tool implementations (e.g., `shell.rs`)
- `src/state.rs`: `AppState` with broadcast channel for logs, `LogEntry` enum,
  and `DbPool` for database access
- `src/discord.rs`: Discord bot integration using serenity, checks user approval
  status before processing messages
- `tests/snapshot_tests.rs`: Live API snapshot tests (requires `live-tests`
  feature)

## API Endpoints

...

## Testing Strategy

### Snapshot Testing

We use `insta` for snapshot testing of API responses.

**CRITICAL RULE**: Coding agents (AI) MUST NEVER validate or automatically
update snapshots (e.g., using `INSTA_UPDATE=always` or `cargo insta accept`).
Snapshot validation and acceptance is EXCLUSIVELY the responsibility of the
human developer. If a snapshot test fails, the agent should report the failure
and wait for the human to review and update the snapshots.

### Unit Tests

Embedded in source files using `#[cfg(test)]` modules:

- `config.rs`: Tests for default values, custom values, missing API key
- `message.rs`: Tests for serialization, message role display
- `state.rs`: Tests for log entry formatting
- `anthropic.rs`: Tests for client creation, text extraction

### Live Integration Tests

Located in `t-koma-gateway/tests/snapshot_tests.rs`:

- Requires `ANTHROPIC_API_KEY` environment variable
- Run with `cargo test --features live-tests`
- Uses `insta` for snapshot testing with redactions for dynamic fields (message
  IDs)
- Tests actual API calls: simple greeting, factual query, list response

### Adding Tests

For unit tests, add to the `#[cfg(test)]` module in the relevant source file:

```rust
#[test]
fn test_my_feature() {
    // Test code
}
```

For live tests, add to `t-koma-gateway/tests/snapshot_tests.rs`:

```rust
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_my_live_test() {
    t_koma_core::load_dotenv();
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("...");
    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");
    // Test code
}
```

## Code Style Guidelines

### Rust Edition

- Use Rust 2024 edition features
- Maintain MSRV of 1.85+

### Error Handling

- Use `thiserror` for defining custom error types
- Propagate errors with `?` operator
- Use `Result<T, Box<dyn std::error::Error>>` for main functions

### Async Patterns

- Use `tokio` for async runtime
- Prefer `tokio::sync::mpsc` for channels
- Use `tokio::select!` for concurrent operations
- Spawn tasks with `tokio::spawn` for background work

### Logging

- Use `tracing` macros: `info!`, `warn!`, `error!`, `debug!`
- Initialize subscriber in main:
  `tracing_subscriber::fmt().with_env_filter(...).init()`
- Use structured logging with key-value pairs when appropriate

### Naming Conventions

- Modules: `snake_case`
- Types/Structs/Enums: `PascalCase`
- Functions/Variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

### Comments

- Use `///` for public API documentation
- Use `//` for inline comments
- Document all public types and functions

## Security Considerations

### API Keys

- **Never commit API keys to version control**
- Use `.env` file for local development (already in `.gitignore`)
- The `Config` struct loads from environment only
- Gateway stores API key in `AnthropicClient`, never exposes it via API

### Discord Bot Token

- Same rules as API keys
- Optional feature - leave empty to disable Discord integration

### WebSocket Connections

- Currently uses unencrypted WebSocket (`ws://`)
- For production deployment, consider TLS termination or `wss://`

### Input Validation

- Gateway validates JSON payloads via serde
- Anthropic API errors are propagated but sanitized
- Discord bot ignores messages from other bots (anti-loop protection)

## Common Development Tasks

### Adding a New Environment Variable

1. Add to `t-koma-core/src/config.rs` in `Config` struct
2. Add default value handling in `from_env_inner()`
3. Update `.env.example`
4. Update this AGENTS.md

### Adding a New HTTP Endpoint

1. Add route in `t-koma-gateway/src/server.rs` `create_router()`
2. Implement handler function
3. Add request/response types as needed
4. Log the request via `state.log()`

### Adding a New WebSocket Message Type

1. Add variant to `WsMessage` or `WsResponse` in `t-koma-core/src/message.rs`
2. Handle in gateway's `handle_websocket()` in `server.rs`
3. Handle in CLI's `handle_ws_message()` in `app.rs`

### Adding a New Crate

1. Create directory with `Cargo.toml`
2. Add to workspace `members` in root `Cargo.toml`
3. Add to workspace `dependencies` if needed
4. Follow existing crate structure

### Adding Database Operations

1. Add methods to `t-koma-db/src/table/name.rs`
2. Write tests in the `#[cfg(test)]` module
3. Update migration file if schema changes needed
4. Use `DbPool` from `AppState` in gateway/CLI

## Dependencies Management

Workspace-level dependencies are defined in root `Cargo.toml`. Crate-specific
dependencies are defined in each crate's `Cargo.toml`.

When adding new dependencies:

1. Check if it should be workspace-level (used by multiple crates) or
   crate-specific
2. Use workspace inheritance: `dep.workspace = true` for workspace deps
3. Keep versions consistent with existing patterns

## Troubleshooting

### Gateway won't start

- Check that `ANTHROPIC_API_KEY` is set
- Verify port 3000 is not in use (or change `GATEWAY_PORT`)
- Check logs with `RUST_LOG=debug cargo run --bin t-koma-gateway`

### CLI can't connect

- Verify gateway is running: `curl http://localhost:3000/health`
- Check `GATEWAY_WS_URL` matches gateway's actual address
- Try manual gateway start first to see error messages

### Discord bot not responding

- Verify `DISCORD_BOT_TOKEN` is set correctly
- Check bot has proper permissions in Discord
- Bot only responds to mentions or DMs by design

## Additional Documentation

- `README.md`: User-facing documentation
- `vibe/specs/`: Design specifications and PoC docs
- `.cursor/`: Cursor IDE configuration (if present)

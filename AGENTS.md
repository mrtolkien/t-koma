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

- Always start by creating a markdown spec in ./vibe/specs for validation by the
  user
- After validation, iterate until all the steps and features you outlined in the
  spec are developed and, if realistic, tested
- Then edit the spec to add a leading underscore to the file name, to show it's
  done

## Project Overview

t-koma is a Rust-based AI agent system powered by the Anthropic Claude API. It
consists of a gateway server that proxies requests to Anthropic and a terminal
UI client for user interaction.

**Repository**: https://github.com/tolki/t-koma

## Architecture

The project follows a workspace structure with three crates:

```
t-koma/
├── Cargo.toml              # Workspace definition
├── t-koma-core/            # Shared types and configuration
├── t-koma-gateway/         # HTTP/WebSocket gateway server + Discord bot
└── t-koma-cli/             # Terminal UI client
```

### Component Communication

```
┌─────────────┐     WebSocket      ┌─────────────┐     HTTP      ┌────────────┐
│  t-koma-cli │ ◄────────────────► │   Gateway   │ ◄───────────► │ Anthropic  │
│   (TUI)     │                    │             │               │    API     │
└─────────────┘                    └─────────────┘               └────────────┘
                                        │
                                        ▼
                              ┌───────────────────┐
                              │   Discord Bot     │
                              │  (optional)       │
                              └───────────────────┘
```

## Technology Stack

- **Language**: Rust 1.85+ (2024 edition)
- **Async Runtime**: tokio
- **Web Framework**: axum (HTTP server with WebSocket support)
- **Terminal UI**: ratatui + crossterm
- **HTTP Client**: reqwest (gateway), tokio-tungstenite (CLI WebSocket)
- **Serialization**: serde, serde_json
- **Error Handling**: thiserror
- **Logging**: tracing, tracing-subscriber
- **Discord Bot**: serenity
- **Testing**: insta (snapshot testing)

## Build and Development Commands

```bash
# Build the entire workspace
cargo build --release

# Run the gateway server
cargo run --release --bin t-koma-gateway

# Run the CLI client (auto-starts gateway if not running)
cargo run --release --bin t-koma-cli

# Run all unit tests (fast, no API calls)
cargo test

# Run live integration tests (requires ANTHROPIC_API_KEY)
cargo test --features live-tests

# Code quality
cargo clippy --all-targets --all-features
cargo fmt
```

## Configuration

Configuration is loaded from environment variables with `.env` file support via
`dotenvy`.

### Required Environment Variables

| Variable            | Description                       |
| ------------------- | --------------------------------- |
| `ANTHROPIC_API_KEY` | Your Anthropic API key (required) |

### Optional Environment Variables

| Variable            | Default                      | Description                                |
| ------------------- | ---------------------------- | ------------------------------------------ |
| `ANTHROPIC_MODEL`   | `claude-sonnet-4-5-20250929` | Claude model to use                        |
| `GATEWAY_HOST`      | `127.0.0.1`                  | Gateway bind address                       |
| `GATEWAY_PORT`      | `3000`                       | Gateway HTTP port                          |
| `GATEWAY_WS_URL`    | `ws://127.0.0.1:3000/ws`     | WebSocket URL for CLI                      |
| `DISCORD_BOT_TOKEN` | _(empty)_                    | Discord bot token (leave empty to disable) |

### Example .env

```bash
ANTHROPIC_API_KEY=sk-ant-api03-...
ANTHROPIC_MODEL=claude-sonnet-4-5-20250929
GATEWAY_PORT=3000
DISCORD_BOT_TOKEN=     # Optional
```

## Code Organization

### t-koma-core

Core types and configuration shared across crates:

- `src/config.rs`: `Config` struct with environment variable loading,
  `load_dotenv()` function
- `src/message.rs`: `ChatMessage`, `MessageRole`, `WsMessage`, `WsResponse`
  types

### t-koma-gateway

Gateway server with both library and binary targets:

- `src/main.rs`: Entry point, initializes tracing, creates Anthropic client,
  optionally starts Discord bot
- `src/server.rs`: HTTP routes (`/health`, `/chat`), WebSocket handlers (`/ws`,
  `/logs`)
- `src/anthropic.rs`: `AnthropicClient` for calling Anthropic API
- `src/state.rs`: `AppState` with broadcast channel for logs, `LogEntry` enum
- `src/discord.rs`: Discord bot integration using serenity
- `tests/snapshot_tests.rs`: Live API snapshot tests (requires `live-tests`
  feature)

### t-koma-cli

Terminal UI client:

- `src/main.rs`: Entry point, menu selection, mode dispatch
- `src/app.rs`: `App` struct with event loop, message handling, TUI state
- `src/ui.rs`: `Ui` struct for rendering chat history, input area, status bar
  with ratatui
- `src/client.rs`: `WsClient` for WebSocket connection to gateway
- `src/gateway_spawner.rs`: Auto-detection and spawning of gateway process
- `src/log_follower.rs`: `LogFollower` for real-time log streaming mode

## API Endpoints

The gateway exposes the following endpoints:

### HTTP Endpoints

- `GET /health` - Health check returning
  `{"status": "ok", "version": "0.1.0", "gateway": "running"}`
- `POST /chat` - Send message to Claude, returns `ChatResponse` with content,
  model, usage

### WebSocket Endpoints

- `WS /ws` - Chat WebSocket
  - Client → Server: `{"type": "chat", "content": "..."}` or `{"type": "ping"}`
  - Server → Client:
    `{"type": "response", "id": "...", "content": "...", "done": true}` or
    `{"type": "error", "message": "..."}`

- `WS /logs` - Log streaming WebSocket
  - Server broadcasts `LogEntry` formatted strings with timestamps

## Testing Strategy

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

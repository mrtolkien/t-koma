# t-koma

A Rust-based AI system with multi-provider model support, featuring the T-KOMA
(ティーコマ) server and a terminal UI client.

## Status

This project is in an extremely early stage and is experimental.
Expect breaking changes and rough edges.

Only one runtime path is currently supported: run the compiled binaries directly.

## Overview

t-koma is an AI system consisting of:

- **t-koma-gateway**: The T-KOMA (ティーコマ) server that proxies requests to the
  configured model provider and manages operators/ghosts
- **t-koma-cli**: A terminal UI client (interface) for interacting with the system
- **t-koma-core**: Shared types and configuration

## Features

- 🤖 **Multi-provider Models**: Supports Anthropic and OpenRouter via a unified provider
  interface
- 🌐 **WebSocket Communication**: Real-time bidirectional messaging between CLI and
  T-KOMA
- 🖥️ **Terminal UI**: Built with ratatui for a rich terminal experience
- 🚀 **Auto-start for Chat**: CLI can start T-KOMA for chat sessions
- 👤 **Operator + Ghost Model**: Operators own ghosts, with per-ghost sessions
- 🗃️ **Per-Ghost Storage**: Each ghost has its own database and workspace
- 🔧 **Configurable**: Environment-based configuration with `.env` file support
- ✅ **Tested**: Unit tests and live API integration tests

## Quick Start

### Prerequisites

- Rust 1.85+ (2024 edition)
- API key for your chosen provider (Anthropic and/or OpenRouter)

### Installation

```bash
# Clone the repository
git clone https://github.com/tolki/t-koma
cd t-koma

# Build release binaries
cargo build --release

# Set up environment
cp .env.example .env
# Edit .env and add your provider API keys
```

### Usage

Run both components from compiled binaries:

```bash
# Terminal 1: Start T-KOMA
./target/release/t-koma-gateway

# Terminal 2: Start the TUI
./target/release/t-koma-cli
```

### Operator/Ghost Flow

1. First message on an interface (Discord or TUI) prompts whether the interface belongs
   to a NEW or EXISTING operator.
2. NEW operator creation is supported; EXISTING operator linking is not yet implemented.
3. Operators must be approved via the management CLI before interacting.
4. GHOST creation happens in Discord. The bot prompts for a name, then boots the GHOST
   with `prompts/system/bootstrap.md` as the first user message.

### TUI Controls

| Key               | Action           |
| ----------------- | ---------------- |
| `Enter`           | Send message     |
| `Esc` or `Ctrl+C` | Quit             |
| `Backspace`       | Delete character |
| `←` `→`           | Move cursor      |

## Configuration

Configuration is loaded from a TOML file and environment variables:

- Settings: `~/.config/t-koma/config.toml` (Linux/macOS) or
  `%APPDATA%/t-koma/config.toml` (Windows)
- Secrets: `.env` or environment variables for API keys

### Example `config.toml`

```toml
default_model = "primary"
heartbeat_model = "fallback"

[models]
[models.primary]
provider = "openrouter"
model = "your-openrouter-model-id"
routing = ["anthropic"] # optional OpenRouter provider order

[models.fallback]
provider = "openai_compatible"
model = "your-local-model-id"
base_url = "http://127.0.0.1:8080"

[gateway]
host = "127.0.0.1"
port = 3000
```

### Example `.env`

```bash
ANTHROPIC_API_KEY=sk-ant-...
OPENROUTER_API_KEY=sk-or-...
OPENAI_API_KEY=sk-openai-...
```

## API Endpoints

T-KOMA exposes the following HTTP endpoints:

### `GET /health`

Health check endpoint.

**Response:**

```json
{
  "status": "ok",
  "version": "0.1.0",
  "koma": "running"
}
```

### `POST /chat`

Send a message to the configured provider.

**Request:**

```json
{
  "content": "Hello!"
}
```

**Response:**

```json
{
  "id": "msg_01ABC123",
  "content": "Hello! How can I help?",
  "model": "your-model-id",
  "usage": {
    "input_tokens": 10,
    "output_tokens": 15
  }
}
```

### `WS /ws`

WebSocket endpoint for real-time communication.

**Client → Server:**

```json
{ "type": "chat", "content": "Hello!" }
```

**Server → Client:**

```json
{
  "type": "response",
  "id": "msg_01ABC123",
  "content": "Hello! How can I help?",
  "done": true
}
```

## Development

### Running Tests

```bash
# Run all unit tests (fast, no API calls)
cargo test

# Run live integration tests (requires ANTHROPIC_API_KEY)
cargo test --features live-tests
```

### Code Quality

```bash
# Run clippy
cargo clippy --all-targets --all-features

# Format all code (Rust, Markdown, SQL)
just fmt                     # Format everything

# Or format individually:
just fmt-rust                # Rust code only
just fmt-md                  # Markdown only
just fmt-sql                 # SQL only

# Check formatting without modifying files
just check-fmt

# Full CI pipeline (format check + clippy + tests)
just ci
```

See [FORMATTING.md](./FORMATTING.md) for detailed formatting documentation.

## Architecture

```
┌─────────────┐     WebSocket      ┌─────────────┐     HTTP      ┌────────────┐
│  t-koma-cli │ ◄────────────────► │  T-KOMA     │ ◄───────────► │ Provider   │
│   (TUI)     │                    │             │               │    APIs    │
└─────────────┘                    └─────────────┘               └────────────┘
```

- **t-koma-cli**: ratatui-based TUI, connects via WebSocket
- **t-koma-gateway**: axum HTTP server (T-KOMA), proxies to provider APIs
- **t-koma-core**: Shared types (messages, config)

### Resetting the Database

For /testing, delete the database file:

```bash
# Linux
rm ~/.local/share/t-koma/db.sqlite3

# macOS
rm ~/Library/Application\ Support/t-koma/db.sqlite3
```

The database will be recreated with migrations on next startup.

## Acknowledgments

- Built with [axum](https://github.com/tokio-rs/axum),
  [ratatui](https://github.com/ratatui/ratatui), and
  [tokio](https://github.com/tokio-rs/tokio)
- Powered by external LLM provider APIs

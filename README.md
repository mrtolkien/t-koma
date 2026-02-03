# t-koma

A Rust-based AI agent with multi-provider model support, featuring a gateway
server and a terminal UI client.

## Overview

t-koma is an AI agent system consisting of:

- **t-koma-gateway**: An async HTTP/WebSocket server that proxies requests to
  the configured model provider
- **t-koma-cli**: A terminal UI client for interacting with the agent
- **t-koma-core**: Shared types and configuration

## Features

- 🤖 **Multi-provider Models**: Supports Anthropic and OpenRouter via a unified
  provider interface
- 🌐 **WebSocket Communication**: Real-time bidirectional messaging between CLI
  and gateway
- 🖥️ **Terminal UI**: Built with ratatui for a rich terminal experience
- 🚀 **Auto-start for Chat**: CLI can start the gateway for chat sessions
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

#### Option 1: Run both components manually

```bash
# Terminal 1: Start the gateway
cargo run --release --bin t-koma-gateway

# Terminal 2: Start the TUI
cargo run --release --bin t-koma-cli
```

#### Option 2: Run just the TUI (auto-starts gateway)

```bash
cargo run --release --bin t-koma-cli
```

The TUI will automatically detect if the gateway is running and start it if
needed.

### TUI Controls

| Key               | Action           |
| ----------------- | ---------------- |
| `Enter`           | Send message     |
| `Esc` or `Ctrl+C` | Quit             |
| `Backspace`       | Delete character |
| `←` `→`           | Move cursor      |

## Configuration

Configuration is loaded from a TOML file and environment variables:

- Settings: `~/.config/t-koma/config.toml` (Linux/macOS) or `%APPDATA%/t-koma/config.toml` (Windows)
- Secrets: `.env` or environment variables for API keys

### Example `config.toml`

```toml
default_model = "primary"

[models]
[models.primary]
provider = "openrouter"
model = "your-openrouter-model-id"

[models.fallback]
provider = "anthropic"
model = "your-anthropic-model-id"

[gateway]
host = "127.0.0.1"
port = 3000
```

### Example `.env`

```bash
ANTHROPIC_API_KEY=sk-ant-...
OPENROUTER_API_KEY=sk-or-...
```

## API Endpoints

The gateway exposes the following HTTP endpoints:

### `GET /health`

Health check endpoint.

**Response:**

```json
{
  "status": "ok",
  "version": "0.1.0",
  "gateway": "running"
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

# Format code
cargo fmt
```

## Architecture

```
┌─────────────┐     WebSocket      ┌─────────────┐     HTTP      ┌────────────┐
│  t-koma-cli │ ◄────────────────► │   Gateway   │ ◄───────────► │ Provider   │
│   (TUI)     │                    │             │               │    APIs    │
└─────────────┘                    └─────────────┘               └────────────┘
```

- **t-koma-cli**: ratatui-based TUI, connects via WebSocket
- **t-koma-gateway**: axum HTTP server, proxies to provider APIs
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

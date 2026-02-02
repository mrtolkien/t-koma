# t-koma

A Rust-based AI agent powered by the Anthropic Claude API, featuring a gateway server and a terminal UI client.

## Overview

t-koma is an AI agent system consisting of:

- **t-koma-gateway**: An async HTTP/WebSocket server that proxies requests to the Anthropic API
- **t-koma-cli**: A terminal UI client for interacting with the agent
- **t-koma-core**: Shared types and configuration

## Features

- 🤖 **Claude 4.5 Sonnet Integration**: Direct API access to Anthropic's latest models
- 🌐 **WebSocket Communication**: Real-time bidirectional messaging between CLI and gateway
- 🖥️ **Terminal UI**: Built with ratatui for a rich terminal experience
- 🚀 **Auto-start**: CLI automatically starts the gateway if not already running
- 🔧 **Configurable**: Environment-based configuration with `.env` file support
- ✅ **Tested**: Unit tests and live API integration tests

## Quick Start

### Prerequisites

- Rust 1.85+ (2024 edition)
- Anthropic API key

### Installation

```bash
# Clone the repository
git clone https://github.com/tolki/t-koma
cd t-koma

# Build release binaries
cargo build --release

# Set up environment
cp .env.example .env
# Edit .env and add your ANTHROPIC_API_KEY
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

The TUI will automatically detect if the gateway is running and start it if needed.

### TUI Controls

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Esc` or `Ctrl+C` | Quit |
| `Backspace` | Delete character |
| `←` `→` | Move cursor |

## Configuration

Configuration is loaded from environment variables (with `.env` file support):

| Variable | Default | Description |
|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | *required* | Your Anthropic API key |
| `ANTHROPIC_MODEL` | `claude-sonnet-4-5-20250929` | Claude model to use |
| `GATEWAY_HOST` | `127.0.0.1` | Gateway bind address |
| `GATEWAY_PORT` | `3000` | Gateway HTTP port |
| `GATEWAY_WS_URL` | `ws://127.0.0.1:3000/ws` | WebSocket URL for CLI |

### Example `.env`

```bash
ANTHROPIC_API_KEY=sk-ant-api03-...
ANTHROPIC_MODEL=claude-sonnet-4-5-20250929
GATEWAY_PORT=3000
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
Send a message to Claude.

**Request:**
```json
{
  "content": "Hello, Claude!"
}
```

**Response:**
```json
{
  "id": "msg_01ABC123",
  "content": "Hello! How can I help?",
  "model": "claude-sonnet-4-5-20250929",
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
{"type": "chat", "content": "Hello!"}
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
│  t-koma-cli │ ◄────────────────► │   Gateway   │ ◄───────────► │ Anthropic  │
│   (TUI)     │                    │             │               │    API     │
└─────────────┘                    └─────────────┘               └────────────┘
```

- **t-koma-cli**: ratatui-based TUI, connects via WebSocket
- **t-koma-gateway**: axum HTTP server, proxies to Anthropic API
- **t-koma-core**: Shared types (messages, config)

## License

MIT License - see LICENSE file for details.

## Acknowledgments

- Built with [axum](https://github.com/tokio-rs/axum), [ratatui](https://github.com/ratatui/ratatui), and [tokio](https://github.com/tokio-rs/tokio)
- Powered by [Anthropic's Claude API](https://www.anthropic.com/claude)

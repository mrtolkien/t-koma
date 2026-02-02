# t-koma PoC Specification

## Date: 2026-02-02

## Overview

AI Agent built on Anthropic API with a gateway server and TUI client.

## Research Summary

### Key Technologies

1. **Anthropic API** - Raw HTTP implementation with `reqwest`
   - Endpoint: `https://api.anthropic.com/v1/messages`
   - Authentication: `x-api-key` header + `anthropic-version: 2023-06-01`
   - Default Model: `claude-sonnet-4-5-20250929` (Claude 4.5 Sonnet)
   - Context Window: 200K tokens
   - Response format: JSON with `content` array containing text blocks

2. **axum** - Web framework for the gateway
   - WebSocket support via `axum::extract::ws`
   - State management with `State` extractor

3. **ratatui** - Terminal UI framework
   - Widget-based rendering with crossterm backend

## Architecture

### Crate Structure

```
t-koma/
├── Cargo.toml                 # Workspace definition
├── t-koma-core/               # Shared types and config
│   ├── src/
│   │   ├── lib.rs
│   │   ├── config.rs
│   │   └── message.rs
│   └── Cargo.toml
├── t-koma-gateway/            # Gateway server
│   ├── src/
│   │   ├── main.rs
│   │   ├── server.rs          # HTTP/WebSocket server
│   │   ├── anthropic.rs       # Raw Anthropic API client
│   │   └── state.rs           # Shared app state
│   └── Cargo.toml
└── t-koma-cli/                # TUI client
    ├── src/
    │   ├── main.rs
    │   ├── app.rs             # Application state/logic
    │   ├── ui.rs              # UI rendering
    │   └── client.rs          # WebSocket client
    └── Cargo.toml
```

## Implementation Plan

### Phase 1: Core Types & Config

- Workspace setup with 3 crates
- Message types (Request/Response/WS protocol)
- Config with env var support

### Phase 2: Gateway Server

- HTTP API: POST /chat, GET /health
- WebSocket endpoint /ws for TUI connections
- Anthropic client using raw reqwest

### Phase 3: TUI Client

- WebSocket client (tokio-tungstenite)
- ratatui UI: chat history, input area, status bar
- Event loop for keyboard + WS messages

### Phase 4: Testing

- Unit tests for core types
- Gateway API integration tests

## Environment Variables

```bash
# Required
ANTHROPIC_API_KEY=sk-ant-api03-...

# Optional (defaults shown)
ANTHROPIC_MODEL=claude-sonnet-4-5-20250929
GATEWAY_HOST=127.0.0.1
GATEWAY_PORT=3000
GATEWAY_WS_URL=ws://127.0.0.1:3000/ws
```

## Anthropic API Specification

### POST https://api.anthropic.com/v1/messages

**Headers:**

```
x-api-key: <ANTHROPIC_API_KEY>
anthropic-version: 2023-06-01
Content-Type: application/json
```

**Request Body:**

```json
{
  "model": "claude-sonnet-4-5-20250929",
  "max_tokens": 4096,
  "messages": [{ "role": "user", "content": "Hello!" }]
}
```

**Response:**

```json
{
  "id": "msg_01ABC123",
  "type": "message",
  "role": "assistant",
  "model": "claude-sonnet-4-5-20250929",
  "content": [{ "type": "text", "text": "Hello! How can I help?" }],
  "usage": {
    "input_tokens": 10,
    "output_tokens": 15
  }
}
```

## WebSocket Protocol

### Client → Gateway

```json
{ "type": "chat", "content": "Hello!" }
```

### Gateway → Client

```json
{
  "type": "response",
  "id": "msg_01ABC123",
  "content": "Hello! How can I help?",
  "done": true
}
```

## Dependencies

### t-koma-core

- `serde`, `thiserror`, `chrono`

### t-koma-gateway

- `tokio`, `axum`, `tower-http`, `reqwest`
- `serde`, `serde_json`, `tracing`
- `t-koma-core`

### t-koma-cli

- `tokio`, `ratatui`, `crossterm`
- `tokio-tungstenite`, `futures`
- `serde`, `serde_json`, `t-koma-core`

## Success Criteria

- [x] Gateway starts with HTTP and WebSocket
- [x] Raw Anthropic API integration works
- [x] TUI connects via WebSocket
- [x] Full message flow: TUI → Gateway → Anthropic → TUI
- [x] Tests pass

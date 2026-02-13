# Usage

The only supported runtime path right now is running compiled binaries directly.

## Starting t-koma

Run both components manually:

```bash
# Terminal 1: Start the gateway
./target/release/t-koma-gateway

# Terminal 2: Start the TUI
./target/release/t-koma-cli
```

## Operator and Ghost Flow

1. Your first message on an interface (Discord or TUI) prompts you to register as a
   **new** or **existing** operator (existing-operator linking is not fully implemented
   yet).
2. New operators must be **approved** via the management CLI before they can chat.
3. Once approved, you can create a **ghost** — your personal AI agent.
4. The ghost is bootstrapped with an initial system prompt and is ready to chat.

## TUI Controls

| Key               | Action           |
| ----------------- | ---------------- |
| `Enter`           | Send message     |
| `Esc` or `Ctrl+C` | Quit             |
| `Backspace`       | Delete character |
| `←` `→`           | Move cursor      |

## API Endpoints

The gateway exposes these HTTP endpoints:

### `GET /health`

Health check endpoint.

```json
{
  "status": "ok",
  "version": "0.1.0",
  "koma": "running"
}
```

### `WS /ws`

WebSocket endpoint for real-time communication (used by the TUI).

### `WS /logs`

WebSocket endpoint for streaming gateway logs.

## Resetting the Database

Delete the database file to start fresh:

```bash
# Linux
rm ~/.local/share/t-koma/koma.sqlite3

# macOS
rm ~/Library/Application\ Support/t-koma/koma.sqlite3
```

The database is recreated with migrations on next startup.

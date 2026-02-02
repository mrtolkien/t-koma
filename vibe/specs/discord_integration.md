# Discord Integration Specification

## Overview

Add Discord bot functionality to t-koma-gateway, allowing the AI agent to
respond to Discord messages via the Anthropic API.

## Goals

- Run a Discord bot as part of the gateway (using serenity)
- Bridge Discord messages to gateway logs
- Respond to Discord messages using Anthropic API
- CLI option to follow gateway logs (including Discord activity)

## Architecture

### Gateway Changes

```
┌─────────────────┐
│  Discord Guild  │
└────────┬────────┘
         │ Gateway Intents
         ▼
┌─────────────────┐     HTTP      ┌──────────────┐
│ serenity Client │ ◄───────────► │  Anthropic   │
│  (in gateway)   │               │     API      │
└────────┬────────┘               └──────────────┘
         │
         │ WebSocket
         ▼
┌─────────────────┐
│   CLI Client    │
│  (log follow)   │
└─────────────────┘
```

### New Components

#### 1. Discord Module (`t-koma-gateway/src/discord.rs`)

- Initialize serenity client with bot token
- Handle `MessageCreate` events
- Filter: only respond to messages mentioning the bot or in DM
- Bridge: send Discord events to log broadcast

#### 2. Log Broadcasting

- Add `tokio::sync::broadcast` channel to `AppState`
- Discord events → broadcast channel
- HTTP/WebSocket events → broadcast channel
- CLI can subscribe to follow logs

#### 3. Discord Message Handler

- On message: extract content
- Call Anthropic API with just the message content (v1)
- Send response back to Discord channel
- Log the interaction

### CLI Changes

#### Menu on Startup

```
┌─────────────────────────────┐
│       t-koma CLI            │
├─────────────────────────────┤
│  1. Chat with t-koma        │
│  2. Follow gateway logs     │
│                             │
│  Select [1-2]: _            │
└─────────────────────────────┘
```

#### Log Follow Mode

- Connect to WebSocket
- Display formatted log messages:

  ```
  [DISCORD] #general @user: Hello bot!
  [AI] -> @user: Hello! How can I help?

  [HTTP] POST /chat - 200 OK
  [WS] Client connected: client_abc123
  ```

## Configuration

### New Environment Variables

```bash
# Discord (optional - gateway works without it)
DISCORD_BOT_TOKEN=your_discord_bot_token_here
DISCORD_BOT_PREFIX=!ai  # Optional prefix for commands
```

### .env.example Update

```bash
# ... existing vars ...

# Discord Bot (optional)
DISCORD_BOT_TOKEN=
```

## Implementation Plan

### Phase 1: Discord Bot in Gateway

1. Add `serenity` dependency to `t-koma-gateway`
2. Create `discord.rs` module with:
   - `start_discord_bot()` function
   - Event handler for `MessageCreate`
   - Filter logic (mentions bot or DM)
3. Integrate into main: spawn Discord bot as separate task
4. Make Discord optional (gateway starts even without token)

### Phase 2: Log Broadcasting

1. Add `tokio::sync::broadcast::Sender<LogEntry>` to `AppState`
2. Create `LogEntry` enum:
   ```rust
   enum LogEntry {
       Discord { channel: String, user: String, content: String },
       DiscordResponse { user: String, content: String },
       HttpRequest { method: String, path: String, status: u16 },
       WebSocket { event: String, client_id: String },
   }
   ```
3. Broadcast from Discord handler
4. Broadcast from HTTP handlers

### Phase 3: CLI Menu & Log Follow

1. On startup: show menu (chat vs logs)
2. Extract existing TUI into `chat_mode()`
3. Create new `log_mode()`:
   - Simple scrolling view (no user input)
   - WebSocket connection for live logs
   - Color-coded by source (Discord=blue, HTTP=green, WS=yellow)

### Phase 4: Anthropic Response

1. In Discord message handler:
   - Extract message content
   - Call `state.anthropic.send_message(content).await`
   - Send response to Discord channel
   - Broadcast to log channel

## Message Flow

### Discord → AI → Discord

1. User sends message in Discord (mentioning bot or DM)
2. serenity triggers `MessageCreate` event
3. Handler extracts content, calls Anthropic API
4. Response sent back to Discord channel
5. Both message and response logged

### Log Follow Flow

1. User selects "Follow logs" in CLI
2. CLI connects to gateway WebSocket
3. Gateway broadcasts all events to subscribed clients
4. CLI displays formatted log entries

## UI/UX

### Gateway Logs Format

```
[2024-02-02 15:30:45] [DISCORD] #general @alice: Hey bot, what's Rust?
[2024-02-02 15:30:47] [AI] @alice: Rust is a systems programming language...
[2024-02-02 15:31:02] [HTTP] 127.0.0.1 POST /chat 200 145ms
[2024-02-02 15:31:15] [WS] client_abc123 connected
```

### CLI Menu

- Simple text menu (not TUI - keep it simple)
- Clear on selection
- Log mode: Ctrl+C to exit

## Dependencies

### t-koma-gateway

```toml
serenity = { version = "0.12", default-features = false, features = ["client", "gateway", "rustls_backend", "model"] }
# Already have: tokio, tracing, etc.
```

### t-koma-cli

```toml
# No new dependencies needed
```

## Notes

- Discord bot is optional - gateway starts regardless
- Log follow mode is read-only (no input)
- For v1: simple message passing (no conversation history)
- Consider rate limiting for Discord (Anthropic API costs)

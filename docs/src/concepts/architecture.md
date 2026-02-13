# Architecture

t-koma follows a layered architecture with clear separation between transport, chat
orchestration, and provider communication.

## Crate Hierarchy

```text
t-koma-cli          t-koma-gateway
   │                     │
   │  WebSocket          ├── providers/     (LLM API adapters)
   └─────────────────►   ├── tools/         (ghost tool system)
                         ├── discord/       (Discord transport)
                         ├── session.rs     (chat orchestration)
                         └── state.rs       (app state + fallback)
                              │
                    ┌─────────┼─────────┐
                    ▼         ▼         ▼
               t-koma-db  t-koma-core  t-koma-knowledge
```

## Key Principles

### Transport Isolation

Transport layers (Discord, WebSocket server) handle message delivery only. They never
manage tools, build chat history, or talk to providers directly. All interactive
conversations go through `SessionChat` in `t-koma-gateway/src/session.rs`.

### Provider Neutrality

Chat history and message types are provider-neutral, defined in
`t-koma-gateway/src/chat/history.rs`. Provider adapters in
`t-koma-gateway/src/providers/` convert to/from provider-specific wire formats
internally.

### Semantic Messages

Gateway outbound responses use semantic `GatewayMessage` payloads defined in
`t-koma-core::message`. Every interactive message includes a `text_fallback` path so
non-rich interfaces (plain text) can still operate.

## Core Components

### AppState (`state.rs`)

Holds shared application state: provider clients, circuit breaker, database pools, and
the chat fallback loop. All chat calls flow through `AppState::try_chat_with_chain()`.

### SessionChat (`session.rs`)

The high-level chat interface. Manages:

- System prompt construction
- Chat history with compaction
- Tool execution loops
- Tool approval flow
- Provider communication via the model chain

### ToolManager (`tools/manager.rs`)

Central tool registry with different constructors for different contexts:

- `new_chat()` — full tool set for interactive sessions
- `new_reflection()` — knowledge curation tools for background reflection

### Scheduler (`scheduler.rs`)

Centralized scheduler for background jobs (heartbeat, reflection). No ad-hoc per-module
timers — all scheduling goes through the scheduler.

## Database Architecture

t-koma uses a **unified SQLite database**:

- **Main DB** (`koma.sqlite3`): operators, ghosts, interfaces, sessions, messages, usage
  logs, job logs, prompt cache
- Ghost data remains isolated by `ghost_id` in ghost-scoped tables

Knowledge indexing uses sqlite-vec for vector operations (embeddings search).

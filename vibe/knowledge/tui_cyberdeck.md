# TUI Cyberdeck Notes

## Purpose

Operational notes for the ratatui cyberdeck in `t-koma-cli`.

## Entry flow

- CLI now boots directly into the cyberdeck (`t-koma-cli/src/main.rs`).
- There is no mode-selection menu in the current flow.

## Puppet Master identity

- TUI user is treated as Puppet Master (admin/operator context).
- Header includes a Japanese marquee welcome message for Puppet Master.

## Logs pipeline

- Gateway `/logs` WebSocket emits structured events:
  - `{ "type": "log_entry", "entry": { "kind": "...", ... } }`
- Runtime tracing is bridged into structured logs via:
  - `t-koma-gateway/src/log_bridge.rs`

### Duplicate prevention strategy

- Chat I/O trace logs are tagged with `event_kind = "chat_io"`.
- Log bridge suppresses only trace events with that field.
- This avoids brittle message-text filtering.

### Routing observability

- Routing decisions are logged as `LogEntry::Routing` with:
  - `platform`, `operator_id`, `ghost_name`, `session_id`
- Emitted from both WS and Discord paths.

## Chat log ownership

- Structured `OperatorMessage` / `GhostMessage` events are emitted from the
  shared `AppState::chat*` boundary so WS and Discord behave consistently.

## Gate viewer UX conventions

- Gate metadata line format:
  - `time level source core-context`
- Message body is rendered below metadata, with lightweight markdown styling.
- Filters:
  - `1` all
  - `2` gateway
  - `3` ghost
  - `4` operator
  - `5` transport
  - `6` warn/error

## Stability guards

- Markdown rendering is UTF-8 safe.
- Long message/core fields are truncated to avoid UI crashes on oversized logs.

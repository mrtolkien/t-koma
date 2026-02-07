# Heartbeat (Session-Idle)

## Goal
Add a session-scoped heartbeat runner to T-KOMA that triggers when a session has been idle for 15 minutes and the last message is not `HEARTBEAT_OK`. Heartbeats run inside the session (no channel routing). They use the standard tool loop and can optionally use a `heartbeat_model` alias from config.

## Scope
- Add `heartbeat_model` (optional) to config, parallel to `default_model`.
- Implement a background heartbeat runner in `t-koma-gateway` that:
  - Wakes on a short interval (e.g., 60s).
  - Iterates all ghosts and their active sessions.
  - Selects sessions where `updated_at <= now - 15min`.
  - Skips if the last message is effectively `HEARTBEAT_OK`.
  - Skips if the session is currently in-flight.
  - Sends a heartbeat prompt as an operator message and saves the response.
- Heartbeat prompt should instruct:
  - Read `HEARTBEAT.md` if it exists and follow it.
  - Do not rehash old tasks if nothing needs attention.
  - If nothing needs attention, reply `HEARTBEAT_OK`.
  - (Project-specific) review inbox and promote items into notes.

## Non-Goals
- Active hours support.
- Channel routing / visibility controls (we have no channel concept).
- Per-ghost schedules, per-session overrides, or manual wake commands.

## Configuration
Add at the top-level of `config.toml`:
- `heartbeat_model` (string, optional): model alias used for heartbeat runs. Falls back to `default_model` when absent.

## Implementation Notes
- Add `t-koma-gateway/src/heartbeat.rs` with:
  - `DEFAULT_HEARTBEAT_PROMPT` constant
  - `is_heartbeat_content_effectively_empty` (OpenClaw semantics)
  - `strip_heartbeat_token` helper (OpenClaw semantics)
  - `last_message_is_heartbeat_ok`
  - `run_heartbeat_tick` used by a background task
- Add `SessionRepository::get_last_message` and `SessionRepository::list_inactive_active_sessions` in `t-koma-db`.
- Start the heartbeat runner in `t-koma-gateway/src/main.rs` after AppState creation.
- Add log entries for heartbeat skips/runs to the log WebSocket stream.

## Files
- `t-koma-core/src/config/settings.rs` (add `heartbeat_model`)
- `t-koma-core/src/config/mod.rs` (validate `heartbeat_model`)
- `t-koma-db/src/sessions.rs` (new queries)
- `t-koma-gateway/src/heartbeat.rs` (new)
- `t-koma-gateway/src/state.rs` (runner + log entries)
- `t-koma-gateway/src/main.rs` (start runner)
- `AGENTS.md` (document heartbeat behavior)

## Tests
- Unit tests for heartbeat token stripping + empty HEARTBEAT.md detection.

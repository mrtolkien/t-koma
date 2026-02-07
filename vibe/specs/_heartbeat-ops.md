# Heartbeat Ops + TUI Indicator

## Goal
Add a default `HEARTBEAT.md` bootstrap file for each ghost workspace, and surface heartbeat scheduling state in the TUI (e.g. “HEARTBEAT IN Xm”). Also prepare a minimal scheduler hook that makes future cron-like jobs easy to integrate without refactoring.

## Scope
1) **Workspace bootstrap**
- Create `HEARTBEAT.md` automatically for new ghosts (and for existing ghosts when first opened if missing).
- The file should be a minimal template with instructions for the heartbeat loop.

2) **Scheduling state exposure**
- Track per-session next heartbeat due time in gateway memory.
- Expose it via the WebSocket API in the ghost/session list payload used by the TUI.
- Render in the TUI ghost pane as “HEARTBEAT IN Xm” (or “HEARTBEAT DUE” if overdue).

3) **Scheduler hook**
- Introduce a minimal `SchedulerState` in `t-koma-gateway` that holds future job metadata (heartbeat now, cron later).
- Heartbeat scheduling should write into this state so later cron jobs can share the same infra.

## Non-Goals
- Persistent scheduling state (DB). In-memory is fine.
- Full cron implementation.
- Changes to heartbeat prompt text.

## Design Details
### HEARTBEAT.md bootstrap
- Location: ghost workspace root (`.../ghosts/<name>/HEARTBEAT.md`).
- Creation timing:
  - On ghost creation and on `get_or_init_ghost_db` if file missing.
- Template content:
  - Short header + a checklist with one placeholder item.

### Scheduling state
- Add `SchedulerEntry` to gateway state:
  - `job_type: "heartbeat"`
  - `session_key` or `(ghost, session_id)`
  - `next_due_ts` (unix seconds)
- `heartbeat.rs` writes:
  - default schedule derived from session `updated_at + 15m`
  - override schedule for `HEARTBEAT_CONTINUE` (`now + 30m`)
- Expose via WS:
  - Extend `WsResponse::SessionList` (or the TUI’s session list payload) with `next_heartbeat_due` per session.

### TUI
- In the ghost pane, show:
  - “HEARTBEAT IN Xm” if due in the future
  - “HEARTBEAT DUE” if overdue
  - nothing if heartbeat disabled or unknown

## Files
- `t-koma-db/src/ghost_db.rs` or `t-koma-db/src/ghosts.rs` (bootstrap helper)
- `t-koma-gateway/src/state.rs` (scheduler state + WS payload)
- `t-koma-gateway/src/heartbeat.rs` (write schedule state)
- `t-koma-gateway/src/server.rs` (WS payload updates)
- `t-koma-cli/src/tui/...` (render indicator)
- `AGENTS.md` (document HEARTBEAT.md bootstrap + indicator)

## Tests
- Unit test for bootstrap creation if missing.
- Unit test for schedule string rendering ("IN Xm" vs "DUE").

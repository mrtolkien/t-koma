# Background Jobs: Heartbeat and Reflection

Background jobs are orchestrated by scheduler state in
`t-koma-gateway/src/scheduler.rs`. Do not create ad-hoc per-module timers.

## Heartbeat (Session Health Check)

- Trigger condition: session idle for configured time
  (`[heartbeat_timing].idle_minutes`, default 4).
- Skip guard: if a successful heartbeat already happened since last activity (checked
  via `job_logs`).
- Prompt source: `HEARTBEAT.md` in GHOST workspace (auto-created on first use).
- Special response handling:
  - `HEARTBEAT_CONTINUE` suppresses session output and reschedules after
    `continue_minutes` (default 30).
- Persistence:
  - Full transcript stored in `job_logs` (not session messages).
  - Only meaningful runs (`status = "ran"`) post summary into session.

## Reflection (Knowledge Curation)

- Checked after each heartbeat tick (including skipped heartbeat ticks).
- Runs when:
  - new session messages exist since last successful reflection
  - and session is idle for configured reflection idle time
    (`[reflection].idle_minutes`, default 4)
- No cooldown: one run per idle window, then waits for new messages.

## Reflection Inputs and Outputs

- Input transcript is filtered:
  - text blocks preserved
  - tool-use blocks summarized as one-liners
  - tool-result blocks stripped
- Reflection toolset:
  - uses `ToolManager::new_reflection()`
  - includes `reflection_todo` for structured planning/status updates
- Continuity:
  - previous `handoff_note` from `job_logs` is injected
  - final model response becomes next `handoff_note`
- Persistence:
  - full transcript + TODO list + handoff note written to `job_logs`
  - reflection transcript does not appear in session messages

## Web Cache Interaction

- `web_fetch` results (2xx only) and `web_search` results are auto-saved as plain files
  to `.web-cache/` in the GHOST workspace during chat. Files have YAML front matter
  (`source_url`, `fetched_at`). No DB records or embeddings are created.
- Reflection sees the file list via the `web_cache_files` template variable in its
  system prompt.
- Reflection curates files into proper reference topics using
  `reference_manage(action="move", cache_file=".web-cache/<file>")` or deletes noise
  with `reference_manage(action="delete", cache_file=...)`.
- `.web-cache/` is auto-cleared after successful reflection.

## Key Files

- `t-koma-gateway/src/heartbeat.rs`
- `t-koma-gateway/src/reflection.rs`
- `t-koma-gateway/src/scheduler.rs`
- `t-koma-db/src/job_logs.rs`

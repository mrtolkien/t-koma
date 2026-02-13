# t-koma AGENTS.md

This file is the high-signal baseline for all agent runs. Feature-specific
implementation details live in `docs/dev/`.

`CLAUDE.md` is symlinked to this file.

## CRUCIAL

Always update this file when:

- project-wide core rules change
- information here is stale
- an important assumption documented here is proven wrong

## Workspace and Flow

- Work in the currently opened workspace/worktree only.
- Do not jump to repo roots outside the opened workspace.
- Prefer small, atomic commits with conventional commit messages. Commit your changes
  step by step.

## Core Concepts

- T-KOMA: deterministic gateway service
- Operator: approved end user
- Interface: messaging endpoint for an operator (Discord, TUI/API)
- Ghost: agent with its own DB/workspace
- Session: chat thread between an operator and a ghost
- Heartbeat: background session health check; transcripts go to `job_logs`
- Reflection: background knowledge curation run after heartbeat ticks; writes to
  knowledge stores and `job_logs`

## MCP Usage (Core)

- Use MCPs heavily when available:
  - `rust-analyzer-mcp` for refactors and code actions
  - `context7` for up-to-date library documentation
  - `gh` MCP for GitHub/PR workflows
- Prefer MCP-backed answers over assumptions for library/framework behavior.

## Code Quality Rules

### Code quality

- If you have to use over 4 levels of indentation, you should likely break it down into
  functions
- Avoid excess comments: code should be expressive and readable by itself. If it
  requires comments, it likely needs a refactor.
- Break down complex systems in clear function or traits, and if required, in multiple
  files with clear names. A file over 500 LoC (excluding tests) likely means a design
  issue. Humans search code through filenames.
- Do not make code live in mod.rs files, they should be mostly barrel files

### MCPs

Make extensive use of MCPs available to you:

- context7 for up-to-date library doc
- rust-analyzer-mcp for refactors and code actions (file name changes, ...)
- gh for interacting with github, including reading files

### Development Flow

- Iterate until all spec items are built and, if realistic, tested.
  - At each step of the integration:
    - Run `just check`.
    - Run `just clippy`.
    - Run `just test` (no live-tests).
    - Run `just fmt`
  - Once an atomic feature is added, make an atomic commit in the conventional-commit
    style (`feat:`, `fix:`, ...)
- Offer the user to create a pull request with the gh mcp

### Config Organization

- All resolved config types belong in `t-koma-core/src/config/`. The TOML `Settings`
  structs (with `Option<T>` fields) live in `settings.rs`; resolved types (with concrete
  defaults) live in dedicated files like `knowledge.rs`.
- New config should be editable via the TUI (already supported via "Edit in Editor").
- Heartbeat uses the optional `heartbeat_model` alias in config; when unset it falls
  back to `default_model`.
- Per-provider configuration docs live in `docs/providers/`.

### Locality of Concern

- Files that describe or support a feature should live near the feature's crate or
  module.
  - For example knowledge-system prompts live inside `prompts/system`.

### Project Layout (Short)

- `t-koma-core`: Shared types, config, WebSocket message schema.
- `t-koma-db`: SQLite layer for operators/ghosts/interfaces/sessions/job_logs.
- `t-koma-knowledge`: Knowledge and memory indexing/search crate (tools-only gateway
  surface).
- `t-koma-gateway`: T-KOMA server, providers, chat/session orchestration, tools, and
  transport handlers.
- `t-koma-cli`: TUI + management CLI.

## Database Notes

- SQLite storage for operators, ghosts, interfaces, sessions, and messages lives under
  the platform data dir and ghost workspaces.
- Schema defined in SQLx migrations: `t-koma-db/migrations/`.
- SQLite runtime bootstrap lives in `t-koma-db/src/sqlite_runtime.rs` (sqlite-vec init,
  pool options, PRAGMAs).

Key types:

- `KomaDbPool`, `GhostDbPool`
- `OperatorRepository`, `GhostRepository`, `InterfaceRepository`, `SessionRepository`,
  `JobLogRepository`, `UsageLogRepository`
- `OperatorStatus`, `Platform`, `ContentBlock`
- `JobLog`, `JobKind`, `TranscriptEntry`, `UsageLog`

Ghost DB tables (beyond messages/sessions):

- `usage_log`: per-request token usage (input, output, cache_read, cache_creation).
  Linked to session_id.
- `prompt_cache`: cached system prompt blocks per session (survives restarts).
- `sessions`: session identity is `id` + timestamps; there is no session title field.
- `sessions.compaction_summary` / `sessions.compaction_cursor_id`: persisted compaction
  state. Original messages are never deleted.

### Job Logs

Background jobs (heartbeat, reflection) use `SessionChat::chat_job()` instead of
`chat()`. This keeps their full transcript (prompt, tool calls, tool results, final
response) out of the session `messages` table. Instead, each run is stored as a single
row in the `job_logs` table (ghost DB) with the transcript as JSON.

Job lifecycle: INSERT at start → UPDATE todos mid-run → UPDATE finish at end.

- `JobLog::start(kind, session_id)` creates an in-progress log.
- `JobLogRepository::insert_started()` persists at job start (TUI sees "in progress").
- `JobLogRepository::update_todos()` updates the `todo_list` column mid-run.
- `JobLogRepository::finish()` sets `finished_at`, `status`, `transcript`, and
  `handoff_note`.
- `JobLogRepository::latest_ok_since()` checks for recent successful runs (used by
  heartbeat skip logic).
- `JobLogRepository::latest_ok()` finds last successful run of a given kind (no time
  bound, used by reflection to find "since" timestamp and handoff note).

Columns: `todo_list` (JSON array of `TodoItem`), `handoff_note` (plain text carried to
next reflection prompt).

Path override knobs for testing:

- `T_KOMA_CONFIG_DIR`: overrides config root dir used by `Settings` (expects
  `config.toml` inside this dir).
- `T_KOMA_DATA_DIR`: overrides data root dir used by `KomaDbPool` and ghost workspace/DB
  paths.

Ghost tool state persistence:

- `ghosts.cwd` stores the current working directory for tools.

Test helpers:

- `t_koma_db::test_helpers::create_test_koma_pool()`
- `t_koma_db::test_helpers::create_test_ghost_pool(ghost_name)`

## Architecture Rule

Transport layers (Discord, WebSocket) do NOT manage tools. They call
`SessionChat.chat()` via `AppState`. Keep tool logic in `t-koma-gateway/src/session.rs`.
Provider-neutral chat history is defined in `t-koma-gateway/src/chat/history.rs`;
provider adapters convert from that neutral model internally.

CLI admin actions that need cross-interface side effects (for example operator approval
that must trigger a Discord follow-up prompt) must go through a gateway WebSocket
command handled in `t-koma-gateway`, not direct CLI DB writes.

Gateway outbound responses use semantic `GatewayMessage` payloads in
`t-koma-core::message` and are rendered per interface. Every interactive gateway message
must include a plaintext `text_fallback` path so non-rich interfaces can still operate
with plain replies.

## Architecture Guardrails

- `default_model` / `heartbeat_model` accept a single alias or an ordered list for
  multi-model fallback. Chain resolution and circuit breaker live in
  `t-koma-gateway/src/state.rs` and `t-koma-gateway/src/circuit_breaker.rs`. See
  `docs/dev/multi-model-fallback.md`.
- Transport layers (`discord`, WebSocket server) must not implement chat/tool
  orchestration.
- Interactive conversations must go through `SessionChat` in
  `t-koma-gateway/src/session.rs`.
- Keep provider-neutral history types in `t-koma-gateway/src/chat/history.rs`.
- Keep provider adapters in `t-koma-gateway/src/providers/`.
- Keep tool implementations and tool manager wiring in `t-koma-gateway/src/tools/`.
- Keep transport-agnostic operator flow in `t-koma-gateway/src/operator_flow.rs`.
- Keep scheduler state centralized in `t-koma-gateway/src/scheduler.rs`.
- No endpoint may bypass normal chat orchestration for interactive chat.
- Workspace boundary checks must remain canonicalization-aware (symlink-safe).

## Testing and Verification

Run these whenever realistic for your change:

1. `just check`
2. `just clippy`
3. `just test` (no live tests)
4. `just fmt`

Rules:

- Snapshot tests use `insta`; AI agents must not accept/update snapshots.
- Live tests (`--features live-tests`) are human-run only.

## Security

- Never commit API keys or tokens.
- Use env vars / `.env` for secrets.

## Developer Specs

Use these detailed guides for feature-specific work:

- `docs/dev/README.md`
- `docs/dev/add-provider.md`
- `docs/dev/add-interface.md`
- `docs/dev/add-tool.md`
- `docs/dev/mcp-usage.md`
- `docs/dev/prompts-and-messages.md`
- `docs/dev/background-jobs.md`
- `docs/dev/knowledge-system.md`
- `docs/dev/multi-model-fallback.md`
- `docs/dev/assistant-first-tooling-migration.md`

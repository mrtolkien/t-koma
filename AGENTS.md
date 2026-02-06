# t-koma AGENTS.md

This document provides essential information for AI coding agents working on the
T-KOMA (ティーコマ) project.

The `CLAUDE.md` file is symlinked to this file too, to help Claude Code use it.

## CRUCIAL

You should ALWAYS EDIT THIS FILE if:

- Any changes you make change the structure or features defined here
- You see outdated information in this file
- You make assumptions about a library that we use that turns out to be wrong

If you implement a complex feature that will need to be referenced later, save
important information in vibe/knowledge.

## Core Concepts

- T-KOMA (ティーコマ): The gateway service. Deterministic logic only.
- OPERATOR (オペレータ): End user. Approved via management CLI.
- INTERFACE: A messaging endpoint for an operator (Discord, TUI). An operator
  can have multiple interfaces.
- GHOST (ゴースト): Agent with its own DB and workspace (same folder as ghost
  DB).
- SESSION: A chat thread between an operator and a ghost (stored in ghost DB).
- Puppet Master: The name used for WebSocket clients.
- In TUI context, the user is the Puppet Master (admin/operator context for
  management UX and messaging labels).

Relationship summary:

- An operator owns multiple ghosts.
- A ghost can have multiple sessions with its owner.
- Ghost names are unique per T-KOMA.

## Code organization and style

### MCPs

Make extensive use of MCPs available to you:

- context7 for up-to-date library doc
- rust-analyzer-mcp for refactors and code actions (file name changes, ...)
- gh for interacting with github, including reading files

### Development Flow

- Always start by creating a markdown spec in `vibe/specs/` for validation by
  the user.
- Make sure you are running in a git worktree to isolate your changes
- Iterate until all spec items are built and, if realistic, tested.
  - At each step of the integration:
    - Run `cargo check --all-features --all-targets`.
    - Run `cargo clippy --all-features --all-targets`.
    - Run `cargo test` (no live-tests).
  - Once an atomic feature is added, make an atomic commit in the
    conventional-commit style (`feat:`, `fix:`, ...)
- Rename the spec file to start with `_` when complete.
- Create a pull request with the gh mcp

### Locality of Concern

- Files that describe or support a feature should live near the feature's crate
  or module.
  - For example knowledge-system prompts live inside
    `t-koma-knowledge/knowledge/prompts`.

### Project Layout (Short)

- `t-koma-core`: Shared types, config, WebSocket message schema.
- `t-koma-db`: SQLite layer for operators/ghosts/interfaces/sessions.
- `t-koma-knowledge`: Knowledge and memory indexing/search crate (tools-only
  gateway surface).
- `t-koma-gateway`: T-KOMA server, providers, chat/session orchestration, tools,
  and transport handlers.
- `t-koma-cli`: TUI + management CLI.

## Database Notes

- SQLite storage for operators, ghosts, interfaces, sessions, and messages lives
  under the platform data dir and ghost workspaces.
- Reference schemas: `t-koma-db/schema.sql`, `t-koma-db/ghost_schema.sql`.
- SQLite runtime bootstrap lives in `t-koma-db/src/sqlite_runtime.rs`
  (sqlite-vec init, pool options, PRAGMAs).

Key types:

- `KomaDbPool`, `GhostDbPool`
- `OperatorRepository`, `GhostRepository`, `InterfaceRepository`,
  `SessionRepository`
- `OperatorStatus`, `Platform`, `ContentBlock`

Path override knobs for testing:

- `T_KOMA_CONFIG_DIR`: overrides config root dir used by `Settings` (expects
  `config.toml` inside this dir).
- `T_KOMA_DATA_DIR`: overrides data root dir used by `KomaDbPool` and ghost
  workspace/DB paths.

Ghost tool state persistence:

- `ghosts.cwd` stores the current working directory for tools.

Test helpers:

- `t_koma_db::test_helpers::create_test_koma_pool()`
- `t_koma_db::test_helpers::create_test_ghost_pool(ghost_name)`

## Architecture Rule

Transport layers (Discord, WebSocket) do NOT manage tools. They call
`SessionChat.chat()` via `AppState`. Keep tool logic in
`t-koma-gateway/src/session.rs`. Provider-neutral chat history is defined in
`t-koma-gateway/src/chat/history.rs`; provider adapters convert from that
neutral model internally.

## Architecture Guardrails

These are hard rules to preserve code quality and discoverability.

### Gateway module ownership

- `t-koma-gateway/src/chat/`: conversation domain only (history, orchestration,
  tool loop state).
- `t-koma-gateway/src/providers/`: provider adapters only (Anthropic,
  OpenRouter). No transport logic here.
- `t-koma-gateway/src/prompt/`: prompt composition/rendering only.
- `t-koma-gateway/src/tools/`: tool implementations and tool manager only.
- `t-koma-gateway/src/server.rs` and `t-koma-gateway/src/discord.rs`: transport
  adapters only.
- `t-koma-gateway/src/web/`: reusable web services used by tools.

### Layering rules

- Transport -> `AppState`/`SessionChat` -> providers/tools/db.
- Providers must not depend on transport or DB modules.
- Prompt modules must stay provider-neutral; provider-specific prompt encoding
  belongs in provider adapters.
- Shared chat history types must stay in `chat/history.rs`. Do not reintroduce
  provider-specific history types in shared traits.

### Naming rules

- Use `providers` (not `models`) for external LLM integrations.
- Use `chat` for session/history orchestration.
- Use explicit names for boundaries (`render`, `history`, `manager`,
  `orchestrator`) instead of generic names like `utils`.

### Data boundary rules

- Keep `t-koma-core` focused on shared protocol/config types.
- Keep `t-koma-db` focused on persistence and schema concerns.
- Convert between DB records and provider payloads at gateway boundaries; do not
  leak provider wire types into DB/core.

### Safety rules

- No endpoint may bypass chat orchestration for interactive conversations.
- Fail fast on critical persistence failures (do not log-and-continue for core
  chat writes).
- Workspace boundary checks must be canonicalization-aware (symlink-safe).

### Pre-alpha policy

- Prefer clean architecture over backward compatibility shims.
- Remove temporary aliases/adapters once migrations are complete.
- If a compatibility shim is added temporarily, include a TODO with removal
  condition in the same PR.

### Refactor checklist (before merge)

- `cargo check --all-features --all-targets`
- `cargo clippy --all-features --all-targets`
- `cargo test` (no `live-tests`)
- Verify touched files still follow module ownership and layering rules above.

## Testing Rules (Short)

- Snapshot tests use `insta`.
- AI agents must NEVER accept or update snapshots.
- Live tests (`--features live-tests`) are human-only.

Full examples live in:

- `vibe/knowledge/testing.md`

## Code Style (Short)

- Rust 2024 edition, MSRV 1.85+
- Error handling via `thiserror` and `?`
- Async runtime: `tokio`
- Logging: `tracing`
- Public APIs must have `///` docs

## Security Reminders

- Never commit API keys. Use `.env` and config.
- Discord bot token follows same rule.
- WebSocket is `ws://` only; use TLS termination for production.

## Web Tools

- `web_search` and `web_fetch` are available when enabled in
  `~/.config/t-koma/config.toml` under `[tools.web]`.
- `web_search` uses the Brave Search API and requires `BRAVE_API_KEY`.
- `web_fetch` performs HTTP fetch + HTML-to-text conversion (no JavaScript).
- Rate limits for Brave are enforced at ~1 query/second.
- Reference: `vibe/knowledge/web_tools.md`.

## Knowledge & Memory Tools

The knowledge system lives in `t-koma-knowledge` with gateway tools in
`t-koma-gateway/src/tools/memory_*.rs` and `reference_search.rs`.

### Scopes

- **SHARED**: Visible to all ghosts. Stored in `xdg_data/knowledge/`.
- **PRIVATE** (ghost_private, ghost_projects, ghost_diary): Owned by a single
  ghost. Stored in ghost workspace subdirs.
- **REFERENCE**: System-maintained read-only corpus. Indexed from topic files.

Cross-scope rule: ghost notes can link to shared notes, but shared notes never
see private data.

### Tools

- `memory_search`: Hybrid BM25 + dense search across scopes.
- `memory_get`: Retrieve a note by ID or title.
- `memory_capture`: Write raw text to ghost or shared inbox.
- `reference_search`: Search reference corpus only.
- `memory_note_create`: Create a structured note with front matter.
- `memory_note_update`: Patch an existing note (title, body, tags, etc.).
- `memory_note_validate`: Mark a note as validated, optionally adjust trust.
- `memory_note_comment`: Append a timestamped comment to a note.

### Testing

- Unit tests: `cargo test -p t-koma-knowledge`
- Integration tests (requires Ollama with `qwen3-embedding:8b`):
  `cargo test -p t-koma-knowledge --features slow-tests`
- **Run slow-tests after any change to the knowledge system.** Snapshot
  mismatches are expected — the user will validate.
- Prompts: `t-koma-knowledge/knowledge/prompts/knowledge_system.md`

## Gateway Content (Brief)

- Messages: add to `t-koma-gateway/messages/en/*.toml` as `[message-id]` with
  `body` and optional `vars`/`title`. Use `{{var}}`.
- Prompts: add `t-koma-gateway/prompts/<id>.md` with TOML front matter (`+++`)
  and a `# loaded:` comment to know where they are used.
- Update `t-koma-gateway/src/content/ids.rs` after changes.

## Common Tasks (Pointer Only)

Detailed how-tos are in `vibe/knowledge/`:

- Adding providers: `vibe/knowledge/providers.md`
- Adding tools: `vibe/knowledge/tools.md`
- Testing patterns: `vibe/knowledge/testing.md`
- Skills system: `vibe/knowledge/skills.md`
- TUI cyberdeck notes: `vibe/knowledge/tui_cyberdeck.md`
- Anthropic/OpenRouter specifics: `vibe/knowledge/anthropic_claude_api.md`,
  `vibe/knowledge/openrouter.md`
- sqlite-vec notes: `vibe/knowledge/sqlite-vec.md`
- Knowledge system: `t-koma-knowledge/knowledge/prompts/knowledge_system.md`

ALWAYS read relevant files in knowledge.md before implementing a feature. If you
see outdated information, update it. If you learn something new during the task
that will be useful for future tasks, create a new knowledge file and list it
here.

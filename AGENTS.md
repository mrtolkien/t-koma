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
- HEARTBEAT: Background session check triggered after 15 minutes of inactivity
  when the last message is not `HEARTBEAT_OK`. Uses `HEARTBEAT.md` in the ghost
  workspace as instructions (auto-created on first use). `HEARTBEAT_CONTINUE`
  suppresses output and reschedules in ~30 minutes.
- REFLECTION: Background job triggered after heartbeat. Processes unread inbox
  captures into structured knowledge (notes, diary, identity files) using the
  `reflection-prompt.md` template (which includes `note-guidelines.md`).
  30-minute cooldown between runs.
- Puppet Master: The name used for WebSocket clients.
- In TUI context, the user is the Puppet Master (admin/operator context for
  management UX and messaging labels).

Relationship summary:

- An operator owns multiple ghosts.
- A ghost can have multiple sessions with its owner.
- Ghost names are unique per T-KOMA.

## Code organization and style

### Code quality

- If you have to use over 4 levels of indentation, you should likely break it
  down into functions
- Avoid excess comments: code should be expressive and readable by itself. If it
  requires comments, it likely needs a refactor.
- Break down complex systems in clear function or traits, and if required, in
  multiple files with clear names. A file over 500 LoC (excluding tests) likely
  means a design issue. Humans search code through filenames.
- Do not make code live in mod.rs files, they should be mostly barrel files

### MCPs

Make extensive use of MCPs available to you:

- context7 for up-to-date library doc
- rust-analyzer-mcp for refactors and code actions (file name changes, ...)
- gh for interacting with github, including reading files

### Development Flow

- Iterate until all spec items are built and, if realistic, tested.
  - At each step of the integration:
    - Run `cargo check --all-features --all-targets`.
    - Run `cargo clippy --all-features --all-targets`.
    - Run `cargo test` (no live-tests).
    - Run `cargo fmt --all`
  - Once an atomic feature is added, make an atomic commit in the
    conventional-commit style (`feat:`, `fix:`, ...)
- Create a pull request with the gh mcp

### Config Organization

- All resolved config types belong in `t-koma-core/src/config/`. The TOML
  `Settings` structs (with `Option<T>` fields) live in `settings.rs`; resolved
  types (with concrete defaults) live in dedicated files like `knowledge.rs`.
- New config should be editable via the TUI (already supported via "Edit in
  Editor").
- Heartbeat uses the optional `heartbeat_model` alias in config; when unset it
  falls back to `default_model`.

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

CLI admin actions that need cross-interface side effects (for example operator
approval that must trigger a Discord follow-up prompt) must go through a gateway
WebSocket command handled in `t-koma-gateway`, not direct CLI DB writes.

Gateway outbound responses use semantic `GatewayMessage` payloads in
`t-koma-core::message` and are rendered per interface. Every interactive gateway
message must include a plaintext `text_fallback` path so non-rich interfaces can
still operate with plain replies.

## Architecture Guardrails

These are hard rules to preserve code quality and discoverability.

### Scheduler (Background Jobs)

Background jobs use `t-koma-gateway/src/scheduler.rs` as the single scheduling
state. Job kinds: `JobKind::Heartbeat` and `JobKind::Reflection`. Future cron
jobs must reuse this scheduler instead of creating bespoke timers or per-module
maps. The only valid place to persist "next due" times in memory is the
scheduler state owned by `AppState`.

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

### Tool design rules

- **Create tools with extreme caution.** Too many tools confuses models — they
  struggle to select the right tool at the right time. Prefer fewer,
  well-designed tools over many granular ones.
- Administrative operations (delete, refresh, bulk management) belong in the
  CLI/TUI, not as ghost-facing tools. Only expose tools that the ghost needs
  during a conversation.
- Each tool must have a clear, non-overlapping purpose. If two tools could
  plausibly handle the same request, merge them or sharpen their descriptions.

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
`t-koma-gateway/src/tools/` (`knowledge_*.rs`, `memory_*.rs`, `reference_*.rs`).

### Folder Layout

```
$DATA_DIR/shared/notes/              → SharedNote scope
$DATA_DIR/shared/references/         → SharedReference scope

$DATA_DIR/ghosts/$slug/inbox/        → NOT indexed, NOT embedded
$DATA_DIR/ghosts/$slug/notes/        → GhostNote scope (tag-based subfolders)
$DATA_DIR/ghosts/$slug/references/   → GhostReference scope
$DATA_DIR/ghosts/$slug/diary/        → GhostDiary scope
$DATA_DIR/ghosts/$slug/skills/       → Ghost-local skills (highest priority)
```

Notes are organized into tag-based subfolders derived from the first tag at
creation time (e.g., `rust/library/` for tag `rust/library`). Files don't move
on tag changes.

### Scopes

Five-variant `KnowledgeScope` enum:

- **SharedNote**: Visible to all ghosts. `owner_ghost = NULL`.
- **SharedReference**: Shared reference topics. `owner_ghost = NULL`.
- **GhostNote**: Private to one ghost. `owner_ghost` required.
- **GhostReference**: Ghost-owned reference topics. `owner_ghost` required.
- **GhostDiary**: Ghost diary entries (date-based markdown). `owner_ghost`
  required.

Helpers: `is_shared()` → SharedNote | SharedReference. `is_reference()` →
SharedReference | GhostReference. `is_note()` → SharedNote | GhostNote.

Cross-scope rule: ghost notes can link to shared notes and reference topics via
`[[Title]]` wiki links, but shared notes never see private data.

### Tools

All knowledge tools are always visible to the ghost. Basic reference usage
guidance is in the system prompt (`reference_system.md`); the
`reference-researcher` skill covers advanced import strategies.

Query tools:

- `knowledge_search`: Unified search across notes, diary, references, and
  topics. Supports `categories` filter, `scope` (all/shared/private), and
  `topic` for narrowing reference searches. Min-1-per-category budget algorithm
  ensures diverse results. Tags are indexed in FTS and embeddings.
- `knowledge_get`: Retrieve full content by ID (searches all scopes) or by
  `topic` + `path` for reference files. Supports `max_chars` truncation.

Memory write tools:

- `memory_capture`: Write raw text to ghost inbox (private only). NOT embedded,
  NOT indexed. Requires `source` field for provenance tracking. Inbox items are
  processed during the reflection job.
- `note_write`: Consolidated tool for note operations. Actions: `create`,
  `update`, `validate`, `comment`, `delete`. (skill: `note-writer`)

Reference write tools:

- `reference_write`: Consolidated tool for reference operations. Actions: `save`
  (file content or topic upsert), `update` (file status or topic metadata),
  `delete` (file only). The `path` field determines scope: present = file
  operation, absent = topic operation. No approval needed.
- `reference_import`: Bulk import from git repos, web pages, and crawled doc
  sites into a reference topic. Source types: `git`, `web`, `crawl` (BFS from
  seed URL). Sources can have a `role` (docs/code) to control search boost.
  Requires operator approval. (skill: `reference-researcher`)

Other:

- `load_skill`: Load a skill for detailed guidance on a workflow. Searches
  ghost-local skills first (`$WORKSPACE/skills/`), then user config, then
  project defaults.

Administrative operations (refresh) are CLI/TUI-only — not ghost tools. Note
deletion is available via `note_write` action `delete`.

### Reflection Job

After each heartbeat tick completes, the reflection runner
(`t-koma-gateway/src/reflection.rs`) checks whether the ghost has unprocessed
inbox files. If so, and the last reflection was > 30 minutes ago, it renders
`prompts/reflection-prompt.md` (which includes `note-guidelines.md` via
`{{ include }}`) with the inbox items, then sends it through the normal chat
pipeline. The ghost curates inbox captures into structured notes, diary entries,
or identity file updates.

Reflection uses `JobKind::Reflection` in the scheduler with a 30-minute
cooldown.

### Skills

Skills are discovered from three locations (highest priority first):

1. Ghost-local: `$WORKSPACE/skills/{name}/SKILL.md`
2. User config: `~/.config/t-koma/skills/{name}/SKILL.md`
3. Project defaults: `./default-prompts/skills/{name}/SKILL.md`

Available skills are listed in the system prompt under "Available Skills".
Ghosts can create their own skills by adding `SKILL.md` files with YAML
frontmatter (`name`, `description`) to their workspace `skills/` directory.

Default skills: `note-writer`, `reference-researcher`, `skill-creator`.

### Topic Discovery

- Use `knowledge_search` with `categories: ["topics"]` to find reference topics.
- The `reference-researcher` default skill teaches ghosts how to research and
  create reference topics effectively.
- The `note-writer` skill's `references/system-internals.md` explains the
  physical file layout and indexing pipeline for agents that need lower-level
  understanding.

### Three-Tier Reference Hierarchy

References use a **Topic > Collection > File** structure:

- **Topic**: Broad knowledge container (e.g., "3d-printers", "dioxus"). Stored
  in `topic.md` with `type = "ReferenceTopic"`.
- **Collection**: Sub-grouping within a topic (e.g., `bambulab-a1/`). Stored in
  `_index.md` with `type = "ReferenceCollection"`. Indexed for search.
- **Reference file**: Individual content unit. Raw content, no front matter.
  Per-file metadata (source_url, fetched_at, status, role) in DB.

### Approval System

Tools that need operator confirmation use `ApprovalReason` in
`tools/context.rs`. Current variants:

- `WorkspaceEscape(path)`: Tool wants to access files outside the workspace.
- `ReferenceImport { title, summary }`: Ghost wants to import external sources
  into a reference topic.

The two-phase pattern: Phase 1 returns `APPROVAL_REQUIRED:` error with metadata.
On approval, Phase 2 re-executes with `has_approval()` returning true. See
`reference_import.rs` for the canonical example.

### Testing

- Unit tests: `cargo test -p t-koma-knowledge`
- Integration tests (requires Ollama with `qwen3-embedding:8b`):
  `cargo test -p t-koma-knowledge --features slow-tests`
- **Run slow-tests after any change to the knowledge system.** Snapshot
  mismatches are expected — the user will validate.
- Prompts: `t-koma-knowledge/knowledge/prompts/knowledge_system.md`

## Gateway Content (Brief)

- Messages: add to `t-koma-gateway/messages/en/*.toml` as `[message-id]` with
  `body` and optional `vars`/`title`/`kind`/`actions`. Use `{{var}}`.
- Prompts: add `t-koma-gateway/prompts/<id>.md` with TOML front matter (`+++`)
  and a `# loaded:` comment to know where they are used.
- **Tool usage guidance** lives in `prompts/tool-guidelines.md`, included in the
  system prompt via `{{ include "tool-guidelines.md" }}`. Prompt text must never
  be inline Rust strings — always use `.md` files in the content system.
- `ghost-context.md` uses Jinja template variables (`{{ ghost_identity }}`,
  `{{ ghost_diary }}`, etc.) rendered per-session with ghost-specific data.
  Template vars must be declared in front matter `vars = [...]`.
- Update `t-koma-gateway/src/content/ids.rs` after changes.
- Debug logging: set `dump_queries = true` in `[logging]` config to write raw
  LLM request/response JSON to `./logs/queries/`.

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
- Operator access levels: `vibe/knowledge/operator_access_levels.md`

ALWAYS read relevant files in knowledge.md before implementing a feature. If you
see outdated information, update it. If you learn something new during the task
that will be useful for future tasks, create a new knowledge file and list it
here.

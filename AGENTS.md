# t-koma AGENTS.md

This document provides essential information for AI coding agents working on the T-KOMA
(ティーコマ) project.

The `CLAUDE.md` file is symlinked to this file too, to help Claude Code use it.

## CRUCIAL

You should ALWAYS EDIT THIS FILE if:

- Any changes you make change the structure or features defined here
- You see outdated information in this file
- You make assumptions about a library that we use that turns out to be wrong

If you implement a complex feature that will need to be referenced later, save important
information in this file.

## Frequent mistakes

I often make you work in git worktrees under feat/$feat_name and you try to access the
repo root, creating permissions issues. Do not do this. Stay in the workspace I open you
in.

## Core Concepts

- T-KOMA (ティーコマ): The gateway service. Deterministic logic only.
- OPERATOR (オペレータ): End user. Approved via management CLI.
- INTERFACE: A messaging endpoint for an operator (Discord, TUI). An operator can have
  multiple interfaces.
- GHOST (ゴースト): Agent with its own DB and workspace (same folder as ghost DB).
- SESSION: A chat thread between an operator and a ghost (stored in ghost DB). Operator
  `NEW` command creates a fresh active session, seeds it with a synthetic first user
  message (`hello`) to get an initial ghost reply, and triggers immediate reflection on
  the previously active session.
- HEARTBEAT: Background session check triggered after configurable idle time (default 4
  minutes) when no successful heartbeat has run since the last session activity (checked
  via `job_logs` table). Uses `HEARTBEAT.md` in the ghost workspace as instructions
  (auto-created on first use). `HEARTBEAT_CONTINUE` suppresses output and reschedules
  after configurable continue interval (default 30 minutes). Heartbeat transcripts are
  stored in `job_logs`, not in session messages. Only meaningful content (status "ran")
  posts a summary to the session.
- REFLECTION: Background knowledge curation job checked after each heartbeat tick
  (including when heartbeat is skipped). Uses a **filtered transcript** (text preserved,
  tool results stripped) and a dedicated reflection tool set
  (`ToolManager::new_reflection()`). Creates a structured TODO plan via
  `reflection_todo`, then curates conversation insights into notes, references, diary,
  and identity files. Carries a **handoff note** between runs for continuity. Web
  results are auto-saved to `_web-cache` during chat; reflection curates them into
  proper topics. Runs when new messages exist since last reflection AND the session has
  been idle for the configured idle time (default 4 minutes). No cooldown — runs once
  per idle window, then waits for new messages. Reflection transcripts are stored in
  `job_logs` and do NOT appear in session messages.
- Puppet Master: The name used for WebSocket clients.
- In TUI context, the user is the Puppet Master (admin/operator context for management
  UX and messaging labels).

Relationship summary:

- An operator owns multiple ghosts.
- A ghost can have multiple sessions with its owner.
- Ghost names are unique per T-KOMA.

## Code organization and style

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
- OpenRouter upstream routing is configured per model with `routing = ["provider-slug"]`
  on `[models.<alias>]`.
- OpenAI-compatible providers use per-model fields on `[models.<alias>]`:
  `provider = "openai_compatible"` and `base_url = "http://host:port[/v1]"`. Optional
  auth uses `OPENAI_API_KEY` by default or model-level `api_key_env`.

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

These are hard rules to preserve code quality and discoverability.

### Scheduler (Background Jobs)

Background jobs use `t-koma-gateway/src/scheduler.rs` as the single scheduling state.
Job kinds: `JobKind::Heartbeat` and `JobKind::Reflection`. Future cron jobs must reuse
this scheduler instead of creating bespoke timers or per-module maps. The only valid
place to persist "next due" times in memory is the scheduler state owned by `AppState`.

### Gateway module ownership

- `t-koma-gateway/src/chat/`: conversation domain only (history, orchestration, token
  budget, prompt caching, compaction).
- `t-koma-gateway/src/operator_flow.rs`: transport-agnostic operator command
  orchestration (chat dispatch, approval/continuation flow, session lifecycle side
  effects).
- `t-koma-gateway/src/providers/`: provider adapters only (Anthropic, OpenRouter,
  OpenAI-compatible). No transport logic here.
- `t-koma-gateway/src/prompt/`: prompt composition/rendering only.
- `t-koma-gateway/src/tools/`: tool implementations and tool manager only.
- `t-koma-gateway/src/server.rs` and `t-koma-gateway/src/discord.rs`: transport adapters
  only.
- `t-koma-gateway/src/web/`: reusable web services used by tools.

### Layering rules

- Transport -> `AppState`/`SessionChat` -> providers/tools/db.
- Providers must not depend on transport or DB modules.
- Prompt modules must stay provider-neutral; provider-specific prompt encoding belongs
  in provider adapters.
- Shared chat history types must stay in `chat/history.rs`. Do not reintroduce
  provider-specific history types in shared traits.

### Naming rules

- Use `providers` (not `models`) for external LLM integrations.
- Use `chat` for session/history orchestration.
- Use explicit names for boundaries (`render`, `history`, `manager`, `orchestrator`)
  instead of generic names like `utils`.

### Data boundary rules

- Keep `t-koma-core` focused on shared protocol/config types.
- Keep `t-koma-db` focused on persistence and schema concerns.
- Convert between DB records and provider payloads at gateway boundaries; do not leak
  provider wire types into DB/core.

### Tool design rules

- **Create tools with extreme caution.** Too many tools confuses models — they struggle
  to select the right tool at the right time. Prefer fewer, well-designed tools over
  many granular ones.
- Administrative operations (delete, refresh, bulk management) belong in the CLI/TUI,
  not as ghost-facing tools. Only expose tools that the ghost needs during a
  conversation.
- Each tool must have a clear, non-overlapping purpose. If two tools could plausibly
  handle the same request, merge them or sharpen their descriptions.

### Safety rules

- No endpoint may bypass chat orchestration for interactive conversations.
- Fail fast on critical persistence failures (do not log-and-continue for core chat
  writes).
- Workspace boundary checks must be canonicalization-aware (symlink-safe).

### Pre-alpha policy

- Prefer clean architecture over backward compatibility shims.
- Remove temporary aliases/adapters once migrations are complete.
- If a compatibility shim is added temporarily, include a TODO with removal condition in
  the same PR.

### Refactor checklist (before merge)

- `just check`
- `just clippy`
- `just test`
- Verify touched files still follow module ownership and layering rules above.

## Testing Rules (Short)

- Snapshot tests use `insta`.
- AI agents must NEVER accept or update snapshots.
- Live tests (`--features live-tests`) are human-only.

Follow existing tests in `t-koma-knowledge/tests/` and crate-level test modules as
canonical examples.

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
- **Auto-save**: `web_fetch` results (2xx only) and `web_search` results are
  automatically saved to the `_web-cache` reference topic via `auto_save_web_result()`
  in `ToolContext`. Search results are saved as JSON. The ghost does NOT need to
  manually call `reference_write` — reflection curates the cache later.
- Keep web tool guidance in this file and prompt/tool docs close to code.

## Knowledge & Memory Tools

The knowledge system lives in `t-koma-knowledge` with gateway tools in
`t-koma-gateway/src/tools/` (`knowledge_*.rs`, `reference_*.rs`).

### Folder Layout

```
$DATA_DIR/shared/notes/              → SharedNote scope (includes topic notes)
$DATA_DIR/shared/references/         → Reference file storage (linked to topic notes)

$DATA_DIR/ghosts/$slug/notes/        → GhostNote scope (tag-based subfolders)
$DATA_DIR/ghosts/$slug/references/   → GhostReference scope
$DATA_DIR/ghosts/$slug/diary/        → GhostDiary scope
$DATA_DIR/ghosts/$slug/skills/       → Ghost-local skills (highest priority)
```

Notes are organized into tag-based subfolders derived from the first tag at creation
time (e.g., `rust/library/` for tag `rust/library`). Files don't move on tag changes.

### Scopes

Five-variant `KnowledgeScope` enum:

- **SharedNote**: Visible to all ghosts. `owner_ghost = NULL`.
- **SharedReference**: Shared reference topics. `owner_ghost = NULL`.
- **GhostNote**: Private to one ghost. `owner_ghost` required.
- **GhostReference**: Ghost-owned reference topics. `owner_ghost` required.
- **GhostDiary**: Ghost diary entries (date-based markdown). `owner_ghost` required.

Helpers: `is_shared()` → SharedNote | SharedReference. `is_reference()` →
SharedReference | GhostReference. `is_note()` → SharedNote | GhostNote.

Cross-scope rule: ghost notes can link to shared notes and reference topics via
`[[Title]]` wiki links, but shared notes never see private data.

### Note Structure

Notes have two classification axes:

- **`entry_type`** (structural): `Note`, `ReferenceDocs`, `ReferenceCode`, `Diary`.
  Used in WHERE clauses for scope discrimination. Set automatically by the ingest
  pipeline. Topics are regular `Note` entries in `shared_note` scope — discovered via
  `reference_files` table joins.
- **`archetype`** (semantic, optional): `person`, `concept`, `decision`, `event`,
  `place`, `project`, `organization`, `procedure`, `media`, `quote`. Optional
  classification for notes — omit when no archetype fits. Filterable via
  `knowledge_search`. Templates in `prompts/skills/note-writer/archetypes/`.

### Tools

Tools are split across two `ToolManager` constructors — ghost sessions get conversation
tools only, reflection gets knowledge-writing tools only.

- `ToolManager::new_chat(skill_paths)` — interactive ghost sessions (14 tools): shell +
  filesystem (8), web (2), knowledge query (2), reference_import, load_skill.
- `ToolManager::new_reflection(skill_paths)` — autonomous reflection jobs (~13 tools):
  knowledge query (2), note_write, reference_write, reference_manage, identity_edit,
  diary_write, reflection_todo, web (2), read_file, find_files, load_skill.

Both expose `get_tools()` for tool listing. `chat_job()` accepts an optional
`tool_manager_override` param to inject the reflection tool manager.

Ghost chat tools (query only):

- `knowledge_search`: Unified search across notes, diary, references, and topics.
  Supports `categories` filter, `scope` (all/shared/private), `topic` for narrowing
  reference searches, and `archetype` for filtering notes by semantic type (e.g. person,
  concept, decision). Min-1-per-category budget algorithm ensures diverse results. Tags
  are indexed in FTS and embeddings.
- `knowledge_get`: Retrieve full content by ID (searches all scopes) or by `topic` +
  `path` for reference files. Supports `max_chars` truncation.

Reflection tools (knowledge writing):

- `note_write`: Consolidated tool for note operations. Actions: `create`, `update`,
  `validate`, `comment`, `delete`. (skill: `note-writer`)
- `reference_write`: Save-only tool for reference files. Fields: `topic` (required —
  must exist as a shared note, except `_web-cache` which auto-creates), `filename`
  (required), `content` or `content_ref` (one required), `collection` (optional),
  `source_url` (optional). No approval needed.
- `reference_manage`: Curation tool for reference files. Actions: `update` (change file
  status), `delete` (remove file), `move` (relocate file between topics server-side —
  content is never exposed to the caller). Files can be identified by `note_id` alone
  (globally unique) or `topic` + `path`. To update topic metadata (description, tags),
  use `note_write(action="update")` instead.
- `identity_edit`: Read/update ghost identity files (BOOT.md, SOUL.md, USER.md).
- `diary_write`: Create or append to diary entries (YYYY-MM-DD.md format).
- `reflection_todo`: Structured TODO list for reflection planning. Actions: `plan`
  (create list), `update` (change item status), `batch_update` (change multiple item
  statuses at once), `add` (append item). Persisted to `job_logs.todo_list` for TUI
  observability.

Other (both):

- `load_skill`: Load a skill for detailed guidance on a workflow. Searches ghost-local
  skills first (`$WORKSPACE/skills/`), then user config, then project defaults.
- `reference_import`: Bulk import tool for documentation sites, code repos, and web
  pages. Registered in `new_chat()` — requires operator approval (two-phase).

Administrative operations (refresh) are CLI/TUI-only — not ghost tools.

### Reflection Job

After each heartbeat tick completes, the reflection runner
(`t-koma-gateway/src/reflection.rs`) checks whether new messages exist since the last
successful reflection (via `JobLogRepository::latest_ok()` +
`SessionRepository::get_messages_since()`). If new messages exist and the session has
been idle for the configured idle time (default 4 minutes), it builds a **filtered
transcript** and sends it through `chat_job()` with `ToolManager::new_reflection()` and
a `JobHandle` for real-time TODO persistence.

Filtered transcript format: text blocks from both roles are preserved verbatim. Tool use
blocks become concise one-liners (`→ Used web_fetch(url)`). Tool result blocks are
stripped entirely — reflection uses `knowledge_search`/`knowledge_get` to access saved
content instead.

The reflection agent receives the previous run's **handoff note** (stored in
`job_logs.handoff_note`) and **today's diary** (read from the ghost workspace) as
context. Its final message becomes the handoff note for the next run, creating
continuity across reflection sessions.

Auto-save: `web_fetch` results (2xx only) and `web_search` results are automatically
saved to the `_web-cache` reference topic during the ghost session. Search results are
saved as JSON. Reflection curates these into proper reference topics or deletes them.

Job lifecycle: the job log is INSERT-ed at start (TUI sees "in progress"), TODO list
updates are persisted mid-run, and finish writes status + transcript + handoff note.

No cooldown — reflection runs once per idle window, then waits for new messages to
appear before triggering again.

### Skills

Skills are discovered from three locations (highest priority first):

1. Ghost-local: `$WORKSPACE/skills/{name}/SKILL.md`
2. User config: `~/.config/t-koma/skills/{name}/SKILL.md`
3. Project defaults: `./prompts/skills/{name}/SKILL.md`

Available skills are listed in the system prompt under "Available Skills". Ghosts can
create their own skills by adding `SKILL.md` files with YAML frontmatter (`name`,
`description`) to their workspace `skills/` directory.

Default skills: `note-writer`, `reference-researcher`, `skill-creator`.

### Ghost Initialization

- When a ghost is created via operator-facing gateway flows, the workspace `SOUL.md` is
  initialized/updated with `I am called <ghost-name>.`

### Topic Discovery

- Use `knowledge_search` with `categories: ["topics"]` to find reference topics.
- The `reference-researcher` default skill teaches ghosts how to research and create
  reference topics effectively.
- The `note-writer` skill's `references/system-internals.md` explains the physical file
  layout and indexing pipeline for agents that need lower-level understanding.

### Reference Structure

References use a **Topic Note > Directory > File** structure:

- **Topic note**: A shared note created via `note_write` that describes the reference
  topic. Lives in `shared/notes/`. Discovered by the system via `reference_files` table
  joins (shared notes that have reference files linked to them).
- **Directory**: Optional sub-grouping within a topic (e.g., `bambulab-a1/`). Just a
  plain filesystem directory — no special metadata file.
- **Reference file**: Individual content unit. Raw content, no front matter. Per-file
  metadata (source_url, fetched_at, status, role) in DB via `reference_files` table.

`reference_save` errors if the topic note doesn't exist, except `_web-cache` which
auto-creates. `reference_import` creates the topic note automatically.

### Approval System

Tools that need operator confirmation use `ApprovalReason` in `tools/context.rs`.
Current variants:

- `WorkspaceEscape(path)`: Tool wants to access files outside the workspace.
- `ReferenceImport { title, summary }`: Ghost wants to import external sources into a
  reference topic.

The two-phase pattern: Phase 1 returns `APPROVAL_REQUIRED:` error with metadata. On
approval, Phase 2 re-executes with `has_approval()` returning true. See
`reference_import.rs` for the canonical example.

### Testing

- Unit tests: `cargo test -p t-koma-knowledge`
- Integration tests (requires Ollama with `qwen3-embedding:8b`):
  `cargo test -p t-koma-knowledge --features slow-tests`
- **Run slow-tests after any change to the knowledge system.** Snapshot mismatches are
  expected — the user will validate.

## Gateway Content (Brief)

- Messages: add to `t-koma-gateway/messages/en/*.toml` as `[message-id]` with `body` and
  optional `vars`/`title`/`kind`/`actions`. Use `{{var}}`.
- Prompts: add `prompts/system/<id>.md` with TOML front matter (`+++`) and a `# loaded:`
  comment to know where they are used.
- Keep the operator chat system prompt in a single file:
  `prompts/system/system-prompt.md` (no prompt-fragment includes).
- Keep the reflection system prompt in a single file:
  `prompts/system/reflection-prompt.md` (self-contained; includes note-writing
  guidance).
- Session context variables (`{{ ghost_identity }}`, `{{ ghost_diary }}`, etc.) are
  rendered directly in `prompts/system/system-prompt.md`. Template vars must be declared
  in front matter `vars = [...]`.
- Update `t-koma-gateway/src/content/ids.rs` after changes.
- Debug logging: set `dump_queries = true` in `[logging]` config to write raw LLM
  request/response JSON to `./logs/queries/`.

## Context Management (Token Budget, Caching, Compaction)

The chat pipeline manages context window usage through three subsystems in
`t-koma-gateway/src/chat/`:

### Token Budget (`token_budget.rs`)

Pure functions for estimating token usage without a tokenizer. Uses a
`ceil(chars / 3.5)` heuristic (~20% margin, works across providers).

- `estimate_tokens(text) -> u32`: core heuristic
- `estimate_system_tokens(blocks)`, `estimate_history_tokens(messages)`,
  `estimate_tool_tokens(tools)`: component estimates
- `context_window_for_model(model) -> u32`: lookup table for known models (Claude 200K,
  Gemini 1M, GPT-4 128K, etc.), fallback 200K
- `compute_budget(model, override, system, tools, history, threshold) -> TokenBudget`:
  computes total usage and `needs_compaction` flag

### Prompt Cache (`prompt_cache.rs`)

In-memory + DB-backed cache for rendered system prompt blocks. Guarantees identical
system prompt bytes within a 5-minute window, enabling prefix-based caching by providers
that support it (e.g. Anthropic prompt caching).

- `PromptCacheManager::get_or_build()`: returns cached blocks if context hash matches
  and age < 5 min; otherwise renders fresh, caches, and returns
- `hash_context(vars)`: deterministic hash of ghost context variables
- DB table: `prompt_cache` (session_id, system_blocks_json, context_hash, cached_at) —
  survives gateway restarts within the 5-min window

### Compaction (`compaction.rs`)

Two-phase context compaction triggered when the token budget exceeds threshold.

**Phase 1 — Observation masking** (free, no LLM call): Replaces verbose `ToolResult`
blocks outside the "keep window" with compact placeholders:
`[tool_result: {tool_name} — {preview}... (truncated)]`. Preserves the action/reasoning
skeleton while removing verbose output.

**Phase 2 — LLM summarization** (one LLM call): When masking alone is insufficient,
summarizes the oldest messages into a single summary block. The summary is persisted to
the session (`compaction_summary`, `compaction_cursor_id` columns) so subsequent
requests start from a smaller history.

Key functions:

- `mask_tool_results(messages, config)`: Phase 1 pure function
- `summarize_and_compact(messages, keep_window, provider)`: Phase 2 async
- `compact_if_needed(...)`: main entry point, tries Phase 1 first

Wiring in `session.rs`:

- `load_compacted_history()`: compaction-aware history loading. If the session has a
  previous summary, loads only messages after the cursor and prepends the summary. Then
  runs `compact_if_needed()` and persists new state if Phase 2 ran.
- `apply_masking_if_needed()`: lightweight Phase 1 only, used mid-tool-loop
- Original `Some(50)` message limit is removed — context budget is token-based

Prompt: `prompts/system/compaction-prompt.md` (summarization instructions for Phase 2).

### Usage Logging

API token usage (input, output, cache_read, cache_creation) is logged to the ghost DB
`usage_log` table after every `send_conversation()` call. Fire-and-forget pattern —
failures are warned but never fail the chat request.

- `UsageLog::new(session_id, message_id, model, ...)`: creates a log entry
- `UsageLogRepository::insert()`: persists to DB
- `UsageLogRepository::session_totals()`: SUM aggregation per session

### Configuration

```toml
# Per-model context window override (optional)
[models.my-model]
provider = "anthropic"
model = "claude-sonnet-4-5-20250929"
context_window = 200000 # overrides built-in lookup

# Compaction settings (all optional, shown with defaults)
[compaction]
threshold = 0.85 # fraction of context window triggering compaction
keep_window = 20 # recent messages kept verbatim
mask_preview_chars = 100 # chars retained from masked tool results

# Heartbeat timing (all optional, shown with defaults)
[heartbeat_timing]
idle_minutes = 4 # session idle time before heartbeat triggers
check_seconds = 60 # polling interval for heartbeat loop
continue_minutes = 30 # reschedule interval after HEARTBEAT_CONTINUE

# Reflection timing (all optional, shown with defaults)
[reflection]
idle_minutes = 4 # session idle time before reflection triggers
# No cooldown — reflection runs once after idle, then waits for new messages
```

## Common Tasks (Pointer Only)

Use crate-local docs, prompts, and tests as the source of truth:

- Gateway prompts: `prompts/system/`
- Default skills: `prompts/skills/`
- Knowledge tests: `t-koma-knowledge/tests/`
- Provider/tool implementation references in their owning crates

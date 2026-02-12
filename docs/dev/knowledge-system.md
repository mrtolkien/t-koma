# Knowledge System Overview

The knowledge system is implemented in `t-koma-knowledge` and exposed to the gateway via
tools in `t-koma-gateway/src/tools/`.

## Scopes

`KnowledgeScope` variants:

- `SharedNote`
- `GhostNote`
- `GhostReference`
- `GhostDiary`

Rule: shared notes must not contain private ghost data. Ghost notes can link shared
notes/reference topics.

## Storage Layout

Shared:

- `$DATA_DIR/shared/notes/`
- `$DATA_DIR/shared/references/`

Per ghost:

- `$DATA_DIR/ghosts/$slug/notes/`
- `$DATA_DIR/ghosts/$slug/references/`
- `$DATA_DIR/ghosts/$slug/diary/`
- `$DATA_DIR/ghosts/$slug/skills/`

## Tool Surface

Chat (`ToolManager::new_chat`): query-oriented tools (search/get, web, filesystem/shell,
reference import, skill load).

Reflection (`ToolManager::new_reflection`): knowledge-writing tools
(note/reference/diary/identity writes, reference manage, reflection_todo, plus
query/web/read helpers).

## Note and Reference Model

- Notes are canonical entities with semantic metadata (`archetype`) and structural type
  (`entry_type`).
- Reference content uses topic structure:
  - topic note (shared note)
  - directory (optional)
  - reference files (content + metadata in DB)
- `reference_write` requires topic note existence except `_web-cache`.

## Search and Retrieval

- `knowledge_search`: hybrid retrieval across notes/diary/references/topics with
  filtering by scope/category/topic/archetype.
- `knowledge_get`: full content by ID or by topic+path.

## Reflection Integration

- Reflection is the curation layer:
  - reads filtered transcript
  - curates insights into notes/references/diary/identity files
  - processes `.web-cache`
- This keeps chat focused on operator response while background runs maintain memory
  quality.

## Testing

Core:

- `cargo test -p t-koma-knowledge`

Knowledge slow tests (when touching knowledge internals):

- `cargo test -p t-koma-knowledge --features slow-tests`

Snapshot and live-test policy:

- AI agents do not accept/update snapshots.
- Live tests are human-run.

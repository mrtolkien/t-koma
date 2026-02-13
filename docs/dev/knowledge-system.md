# Knowledge System Overview

The knowledge system is implemented in `t-koma-knowledge` and exposed to the gateway via
tools in `t-koma-gateway/src/tools/`.

## Scopes

Five-variant `KnowledgeScope` enum:

- `SharedNote`: visible to all GHOSTS (`owner_ghost = NULL`)
- `SharedReference`: shared reference topics (`owner_ghost = NULL`)
- `GhostNote`: private to one GHOST (`owner_ghost` required)
- `GhostReference`: GHOST-owned reference topics (`owner_ghost` required)
- `GhostDiary`: GHOST diary entries, date-based markdown (`owner_ghost` required)

Helpers: `is_shared()` = SharedNote | SharedReference, `is_reference()` =
SharedReference | GhostReference, `is_note()` = SharedNote | GhostNote.

Rule: shared notes must not contain private GHOST data. GHOST notes can link shared
notes/reference topics via `[[Title]]` wiki links.

## Storage Layout

Shared:

- `$DATA_DIR/shared/notes/`
- `$DATA_DIR/shared/references/`

Per GHOST:

- `$DATA_DIR/ghosts/$slug/notes/`
- `$DATA_DIR/ghosts/$slug/references/`
- `$DATA_DIR/ghosts/$slug/diary/`
- `$DATA_DIR/ghosts/$slug/skills/`
- `$DATA_DIR/ghosts/$slug/.web-cache/` (transient, plain files, auto-cleared)

Notes are organized into tag-based subfolders derived from the first tag at creation
time (e.g., `rust/library/` for tag `rust/library`). Files don't move on tag changes.

## Note Classification

Notes have two classification axes:

- **`entry_type`** (structural): `Note`, `ReferenceDocs`, `ReferenceCode`, `Diary`. Set
  automatically by the ingest pipeline. Topics are regular `Note` entries in
  `shared_note` scope, discovered via `reference_files` table joins.
- **`archetype`** (semantic, optional): `person`, `concept`, `decision`, `event`,
  `place`, `project`, `organization`, `procedure`, `media`, `quote`, `topic`. Filterable
  via `knowledge_search`. Templates in `prompts/skills/note-writer/archetypes/`.

## Reference Model

References use a **Topic Note > Directory > File** structure:

- **Topic note**: shared note created via `note_write` that describes the reference
  topic. Discovered by the system via `reference_files` table joins.
- **Directory**: optional sub-grouping (just a plain filesystem directory).
- **Reference file**: individual content unit. Raw content, no front matter. Per-file
  metadata (source_url, fetched_at, status, role) in DB via `reference_files` table.

`reference_write` requires the topic note to exist. `reference_import` creates the topic
note automatically.

## Tool Surface

Chat (`ToolManager::new_chat`): query-oriented tools (search/get, web, filesystem/shell,
reference import, skill load).

Reflection (`ToolManager::new_reflection`): knowledge-writing tools
(note/reference/diary/identity writes, reference manage, reflection_todo, plus
query/web/read helpers).

Key reflection tools:

- `reference_manage`: curation tool with actions `update`, `delete`, `move`. Files can
  be identified by `note_id`, `topic` + `path`, or `cache_file` (for `.web-cache/` files
  not yet in the DB).
- `reference_write`: save-only tool. Requires topic note to exist.
- `note_write`: consolidated note operations (create/update/validate/comment/delete).

## Web Cache (Filesystem Staging)

Web results are staged as plain files in the GHOST's `.web-cache/` directory:

- `web_fetch` and `web_search` results are auto-saved during chat via
  `auto_save_web_result()` in `ToolContext`.
- Files have YAML front matter (`source_url`, `fetched_at`). Search results are saved as
  JSON.
- No DB records or embeddings are created for cached files.
- Reflection sees the file list via the `web_cache_files` template variable and curates
  content into proper reference topics using
  `reference_manage(action="move", cache_file=".web-cache/<file>")`.
- `.web-cache/` is auto-cleared after successful reflection.

## Search and Retrieval

- `knowledge_search`: hybrid retrieval across notes/diary/references/topics with
  filtering by scope/category/topic/archetype.
- `knowledge_get`: full content by ID or by topic+path.

## Reflection Integration

- Reflection is the curation layer:
  - reads filtered transcript
  - curates insights into notes/references/diary/identity files
  - processes `.web-cache/` into reference topics
- This keeps chat focused on OPERATOR response while background runs maintain memory
  quality.

## Testing

Core:

- `cargo test -p t-koma-knowledge`

Knowledge slow tests (when touching knowledge internals):

- `cargo test -p t-koma-knowledge --features slow-tests`

Snapshot and live-test policy:

- AI agents do not accept/update snapshots.
- Live tests are human-run.

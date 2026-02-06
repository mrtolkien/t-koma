# Knowledge v2 Specification

## Status: In Progress

## Overview

Clean rewrite of the knowledge system with:
- Cleaner folder hierarchy (shared/notes, shared/references, ghosts/$slug/{notes,references,diary,inbox})
- Private references (ghost-owned reference topics)
- Fewer always-visible tools; creation tools unlocked via skills
- Inbox not embedded; diary embedded but simple markdown (no front matter)
- No backwards compatibility or migrations needed

## Folder Layout

```
$DATA_DIR/shared/notes/              → SharedNote scope
$DATA_DIR/shared/references/         → SharedReference scope

$DATA_DIR/ghosts/$slug/inbox/        → NOT indexed, NOT embedded
$DATA_DIR/ghosts/$slug/notes/        → GhostNote scope
$DATA_DIR/ghosts/$slug/references/   → GhostReference scope
$DATA_DIR/ghosts/$slug/diary/        → GhostDiary scope
```

## Scopes

```rust
pub enum KnowledgeScope {
    SharedNote,       // owner_ghost = NULL
    SharedReference,  // owner_ghost = NULL
    GhostNote,        // owner_ghost required
    GhostReference,   // owner_ghost required
    GhostDiary,       // owner_ghost required
}
```

Helpers: `is_shared()`, `is_reference()`, `is_note()`

## Tool Surface

### Always-Available (6 tools)
- `capture_thought` — write to inbox (not embedded)
- `search_notes` — hybrid BM25+dense on notes
- `search_references` — search within a reference topic
- `search_diary` — semantic search on diary entries
- `reference_topic_search` — find topics by semantic query
- `reference_topic_list` — list all topics

### Skill-Unlocked
- **note-writer**: create_note, update_note, validate_note, comment_note, get_note
- **reference-researcher**: create_reference_topic, update_reference_topic, get_reference, update_reference_file

## Diary Format

Simple markdown. Filename = `YYYY-MM-DD.md`. No TOML front matter. Embedded for search.

## Inbox Format

Raw markdown. Filename = `{timestamp}-{slug}.md`. Optional source comment at top.
NOT embedded, NOT indexed.

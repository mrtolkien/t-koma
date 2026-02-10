# Knowledge System Internals

Physical file layout, formats, and indexing pipeline for the knowledge system.
Use this reference when you need to understand how content is stored on disk,
debug indexing issues, or work with knowledge files directly.

## Folder Hierarchy

All paths are relative to the platform data directory (`$DATA_DIR`, typically
`~/.local/share/t-koma/`).

```
$DATA_DIR/
  shared/
    notes/                          # SharedNote scope
      *.md                          # Notes with TOML front matter
    references/                     # SharedReference scope
      <topic-slug>/
        topic.md                    # Topic description with front matter
        <fetched-files>             # Source files (code, docs)

$GHOST_WORKSPACE/                   # Per-ghost workspace root
  notes/                            # GhostNote scope (tag-based subfolders)
    <tag-subfolder>/
      *.md                          # Private notes with front matter
  references/                       # GhostReference scope
    <topic-slug>/
      topic.md
      <fetched-files>
  diary/                            # GhostDiary scope
    YYYY-MM-DD.md                   # Plain markdown, no front matter
  inbox/                            # NOT indexed, NOT embedded
    {unix-timestamp}-{slug}.md      # Raw captures from memory_capture
  skills/                           # Ghost-local skills (highest priority)
    {skill-name}/SKILL.md
```

## Note Format

Notes use TOML front matter delimited by `+++`:

```markdown
+++
id = "uuid-here"
title = "Note Title"
archetype = "concept"            # optional
created_at = "2025-01-15T10:00:00Z"
trust_score = 8
tags = ["rust", "async"]
parent = "parent-note-id"        # optional

[created_by]
ghost = "ghost-name"
model = "claude-sonnet-4-5-20250929"
+++

Note body in markdown.

Link to other notes: [[Note Title]] or [[Note Title|alias]].
Links can target any shared note or reference topic by title.
```

### Front Matter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | yes | Unique note ID (UUID) |
| `title` | yes | Human-readable title |
| `archetype` | no | Optional semantic classification (lowercase). One of: `person`, `concept`, `decision`, `event`, `place`, `project`, `organization`, `procedure`, `media`, `quote` |
| `created_at` | yes | ISO 8601 timestamp |
| `trust_score` | yes | 0-10, higher = more trusted |
| `created_by.ghost` | yes | Ghost that created the note |
| `created_by.model` | yes | Model used |
| `tags` | no | String array for categorization |
| `parent` | no | ID of parent note (for hierarchy) |
| `version` | no | Integer version counter |
| `source` | no | Provenance array (URLs, "user stated", etc.) |

## Diary Format

Plain markdown files, **no front matter**. Filename is the date.

Each file is one day's entry. The system generates a deterministic ID
(`diary:{ghost}:{date}`) so re-indexing produces upserts.

## Inbox Format

Raw markdown captured via `memory_capture`. **Not indexed** — these are staging
areas for later curation into structured notes during reflection.

Format: `{unix-timestamp}-{slug}.md`. Optional provenance comment at top:

```markdown
<!-- source: https://example.com/docs -->
Raw captured content here...
```

## Reference Topic Structure

Each reference topic is a directory containing `topic.md` (with front matter)
plus fetched source files:

```
reference/
  dioxus-framework/
    topic.md                    # Front matter + topic body
    README.md                   # Fetched from git source
    src/lib.rs                  # Fetched from git source
    docs-getting-started.md     # Fetched from web source
```

### topic.md Special Fields

In addition to standard note fields, topic.md has:

```toml
files = ["README.md", "src/lib.rs", "docs-getting-started.md"]
max_age_days = 30
fetched_at = "2025-01-15T10:00:00Z"

[[sources]]
type = "git"
url = "https://github.com/example/repo"
ref = "main"
paths = ["README.md", "src/"]
role = "code"

[[sources]]
type = "web"
url = "https://example.com/docs/getting-started"
role = "docs"
```

## Storage Scopes

| Scope | `owner_ghost` | Visibility |
|-------|---------------|------------|
| SharedNote | `NULL` | All ghosts |
| SharedReference | `NULL` | All ghosts |
| GhostNote | ghost name | Owner only |
| GhostReference | ghost name | Owner only |
| GhostDiary | ghost name | Owner only |

Cross-scope rule: ghost notes can link to shared notes and reference topics via
`[[Title]]` wiki links. Shared notes never see private data.

## Index Database

Single SQLite database (`knowledge/index.sqlite3`) with:

- **notes**: All note metadata across all scopes
- **chunks**: Text chunks for each note (heading-based for markdown,
  tree-sitter-based for code)
- **chunk_fts**: FTS5 full-text search index on chunks
- **chunk_vec**: sqlite-vec embedding index for dense retrieval
- **links**: Wiki-link edges between notes
- **tags**: Tag assignments per note
- **reference_topics**: Topic metadata (files list)
- **reference_files**: Per-file metadata (role, status)
- **meta**: Key-value store (reconciliation timestamps)

## Indexing Flow

1. **File watcher** (`notify` crate) monitors all scope directories
2. **Debounce** (2 second window) coalesces rapid changes
3. **Reconcile** walks directory trees:
   - Notes: `index_markdown_tree()` with `parse_note()` front matter
   - Diary entries: `index_diary_tree()` with `ingest_diary_entry()` (no front matter)
   - Reference topics: `index_reference_topics()` parses `topic.md` + all listed files
4. **Content hash dedup**: Skip files whose hash hasn't changed since last index
5. **Chunking**: Markdown by headings (`chunk_markdown`), code by tree-sitter
   (`chunk_code`)
6. **Embedding**: Batched embedding via configured model (default: `qwen3-embedding:8b`)
7. **Search**: Hybrid BM25 (FTS5) + dense (sqlite-vec KNN) with RRF fusion

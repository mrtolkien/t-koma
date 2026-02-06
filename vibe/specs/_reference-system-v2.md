# Reference System v2: Three-Tier Hierarchy

## Status: In Progress

## Context

The current reference system treats topics as one-shot "fetch events" — you
create a topic, it clones a git repo and/or fetches web pages, and that's it.
This works for library docs ("dioxus", "sqlite-vec") but fails for incremental
knowledge accumulation where a ghost saves useful web pages, articles, and data
over time.

Problems with v1:

- Creating a reference requires operator approval **every time**
- Topic-level staleness conflates the container with the content
- No way to incrementally add content to existing topics
- No sub-grouping within a topic

## Design

### Three-Tier Hierarchy

```
references/
  $topic/
    topic.md                  ← ReferenceTopic (indexed note)
    file-a.md                 ← root-level file
    $collection/
      _index.md               ← ReferenceCollection (indexed note)
      file-b.md               ← collection file
      file-c.rs               ← collection file
```

- **Topic**: Broad knowledge category/container (e.g., "3d-printers",
  "slay-the-spire"). Stored in `topic.md` with `type = "ReferenceTopic"`. No
  staleness — topics are long-lived.
- **Collection**: Sub-grouping within a topic (e.g., `bambulab-a1/` inside
  `3d-printers`). Stored in `_index.md` with `type = "ReferenceCollection"`.
  Indexed and embedded for search. Provides context enrichment for child file
  chunks.
- **Reference file**: Individual content unit (web page dump, data file, etc.).
  Raw content, no front matter. Per-file metadata (source_url, fetched_at,
  status, role, max_age_days) in DB `reference_files` table.

### Data Model Changes

#### DB migration (0002)

```sql
ALTER TABLE reference_files ADD COLUMN source_url TEXT;
ALTER TABLE reference_files ADD COLUMN source_type TEXT NOT NULL DEFAULT 'git';
ALTER TABLE reference_files ADD COLUMN fetched_at TEXT;
ALTER TABLE reference_files ADD COLUMN max_age_days INTEGER NOT NULL DEFAULT 0;
```

#### Model changes

- **Remove topic-level staleness**: `TopicStatus` enum removed. Topics are
  either active or removed by operator.
- **New `ReferenceSaveRequest`**: topic, path, content, source_url, role,
  title, collection_title, collection_description, collection_tags, tags,
  topic_description.
- **New `ReferenceSaveResult`**: topic_id, note_id, path, created_topic,
  created_collection.
- **New `CollectionSummary`**: title, path, file_count.
- **Simplified `TopicListEntry`**: remove fetched_at, max_age_days,
  source_count. Add `collections: Vec<CollectionSummary>`.
- **Simplified `TopicSearchResult`**: remove fetched_at, is_stale.
- **Simplified `TopicUpdateRequest`**: remove status, max_age_days.

### Tool Surface

- `reference_save`: Primary write tool. Creates topic/collection implicitly.
  No approval for inline/web content. Must search topics first.
- `reference_import`: Bulk git repo import (replaces old
  `reference_topic_create` for repos). Requires approval.
- Read tools unchanged: `reference_search`, `reference_topic_search`,
  `reference_topic_list`, `reference_get`.
- `reference_topic_update`, `reference_file_update` for metadata management.

### Key Decisions

- Staleness is per-file, NOT per-topic
- `_index.md` is an indexed note (searchable) — not just metadata
- Chunk context enrichment: collection title+description prepended to file
  chunks before embedding
- Topic creation is free but ghost must search first (prompt-enforced)
- Approval only for git cloning, not for saving inline/web content
- Fuzzy matching on topic names when saving

### Chunk Context Enrichment

When indexing file chunks, prepend collection/topic context:
- For collection files: `[{collection_title}: {collection_description}]\n\n{chunk}`
- For root-level files: `[{topic_title}]\n\n{chunk}`

This ensures search queries about the collection subject find file chunks even
if the raw file content doesn't mention the collection name.

## Implementation Steps

1. DB migration + simplified models
2. Collection type + `_index.md` indexing
3. Chunk context enrichment
4. `reference_save` engine method + gateway tool
5. Rename `reference_topic_create` → `reference_import`
6. Read tool updates (directory listing in reference_get)
7. Docs, skills, AGENTS.md updates, tests

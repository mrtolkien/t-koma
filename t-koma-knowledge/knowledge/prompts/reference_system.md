## Reference System

References are your persistent external knowledge. Web content disappears —
pages change, go offline, or get paywalled. If you fetched it, save it.

### When to save references

**Every time** you call `web_fetch` or get useful results from `web_search`. Do
it in the same turn as your response — don't plan to "save it later".

### Quick save (minimum viable reference)

```
reference_write(action="save", topic="rust-async",
  path="tokio-select-guide.md",
  content="<the fetched content>",
  source_url="https://tokio.rs/tokio/tutorial/select")
```

That's it. Four fields: `topic`, `path`, `content`, `source_url`. The topic and
collection are auto-created if they don't exist.

### What to Save

- Fetched web pages (articles, docs, blog posts, forum answers)
- Web search result summaries with source URLs
- API specs, protocol docs, configuration references
- Product pages, spec sheets, data tables
- JSONs with valuable data (searchable via filesystem tools)
- Any external content worth preserving for future conversations or that you
  want to be able to query through embeddings

Focus on **primary sources**. If a page cites a source, fetch the source too. If
a primary source is inaccessible, try the WayBack machine:
`http://archive.org/wayback/available?url=example.com`

### Topic > Collection > File Hierarchy

- **Topic**: Broad container (e.g., "dioxus", "3d-printers"). Auto-created on
  first save.
- **Collection**: Optional sub-grouping. Created when your path has a directory
  (e.g., `guide/state-management.md` creates a "guide" collection).
- **File**: Individual content unit.

Before creating a new topic, search first:
`knowledge_search(query="topic name", categories=["topics"])`

### `reference_write` — Full Reference

| Action   | With `path`           | Without `path`                       |
| -------- | --------------------- | ------------------------------------ |
| `save`   | Save file content     | Create/update topic                  |
| `update` | Change file status    | Update topic metadata (body, tags)   |
| `delete` | Delete reference file | Error (topic deletion is admin-only) |

Optional fields for richer saves: `role` (`docs`/`code`/`data`, default `docs`),
`title`, `collection_title`, `collection_description`, `tags`.

**File status** (for managing quality): `active` (default), `problematic` (0.5x
search penalty, provide reason), `obsolete` (excluded from search, provide
reason).

### `reference_import` Tool

Bulk import from git repos, web pages, and crawled doc sites. Requires operator
approval. Load the `reference-researcher` skill for advanced import strategies.

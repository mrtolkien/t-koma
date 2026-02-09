## Reference System

References are your persistent external knowledge. Web content disappears —
pages change, go offline, or get paywalled. If you fetched it, save it.

### When to save references

**Every time** you call `web_fetch` or get useful results from `web_search`. Do
it in the same turn as your response — don't plan to "save it later".

### Quick save (minimum viable reference)

```
reference_write(topic="rust-async", filename="tokio-select-guide.md",
  content_ref=1, source_url="https://tokio.rs/tokio/tutorial/select")
```

That's it. Fields: `topic`, `filename`, `content_ref` (or `content`),
`source_url`. The topic and collection are auto-created if they don't exist.

### Using `content_ref`

Web tool results (`web_fetch`, `web_search`) are automatically cached with a
result ID shown as `[Result #N]` in the output. Instead of copying large content
into the `content` field, use `content_ref=N` to reference the cached result.
This avoids duplicating content and reduces token usage.

If you need to save content that didn't come from a web tool, use the `content`
field directly. Exactly one of `content` or `content_ref` must be provided.

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
- **Collection**: Optional sub-grouping via the `collection` field (e.g.,
  `collection="guide"` with `filename="state-management.md"` creates
  `guide/state-management.md`).
- **File**: Individual content unit.

Before creating a new topic, search first:
`knowledge_search(query="topic name", categories=["topics"])`

### `reference_write` — Save Only

| Field         | Required | Description                                  |
| ------------- | -------- | -------------------------------------------- |
| `topic`       | yes      | Topic slug (e.g., "rust-async")              |
| `filename`    | yes      | Filename (e.g., "guide.md")                  |
| `content`     | one of   | Raw content to save                          |
| `content_ref` | one of   | ID of cached web tool result                 |
| `collection`  | no       | Sub-grouping within topic                    |
| `source_url`  | no       | Original URL source                          |

Reference curation (status changes, topic metadata updates, deletion) is handled
automatically during reflection.

### `reference_import` Tool

Bulk import from git repos, web pages, and crawled doc sites. Requires operator
approval. Load the `reference-researcher` skill for advanced import strategies.

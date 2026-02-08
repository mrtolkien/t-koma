## Reference System

References are curated external knowledge (documentation, articles, code, data)
organized into searchable topics. Save references during normal conversation
when you encounter valuable external content.

### When to save references

Any time you use a web tool that lets you answer a user's question, save it as
reference. Usually, this will be at the end of a tools call chain, together with
the response to the user.

### What to Save as References

- Web search results
- Blog posts, articles, or documentation you fetched with `web_fetch`
- Wiki pages
- API specs, protocol docs, or configuration references
- Research findings about libraries or frameworks
- Any external content worth preserving for future conversations (products
  pages, specs pages, forum posts, ...)
- JSONs with valuable data. While search might be iffy, you can access private
  references through filesystem tools and write scripts to interact with them.

Focus on PRIMARY SOURCES, and on trusted and respected websites in their
respective field:

- If you see a source for the info you searched in the page you fetched: fetch
  that page.
- If you have trouble with accessing a primary source, try to access it through
  the WayBack machine's CDX API:
  `http://archive.org/wayback/available?url=example.com`

### Topic > Collection > File Hierarchy

- **Topic**: Broad knowledge container (e.g., "dioxus", "3d-printers"). Created
  implicitly when you first save to it.
- **Collection**: Sub-grouping within a topic (e.g., `bambulab-a1/`). Created
  when you use a subdirectory path like `collection/file.md`.
- **File**: Individual content unit (web page, data file, code snippet).

### `reference_write` Tool

Single tool for all reference write operations. The `path` field determines
scope: present = file operation, absent = topic operation.

**Actions:**

| Action   | With path             | Without path                         |
| -------- | --------------------- | ------------------------------------ |
| `save`   | Save file content     | Create/update topic                  |
| `update` | Change file status    | Update topic metadata (body, tags)   |
| `delete` | Delete reference file | Error (topic deletion is admin-only) |

**Saving files** (most common):

```
reference_write(action="save", topic="dioxus", path="guide/state-management.md",
  content="...", source_url="https://...", role="docs",
  collection_title="Guide", collection_description="Official Dioxus guide chapters")
```

**Roles**: `docs` (boosted 1.5x in search), `code`, `data`. Default: `docs`.

**File status** (for managing quality):

- `active`: Normal ranking (default)
- `problematic`: Partially wrong — penalized in search (0.5x). Always provide a
  reason.
- `obsolete`: Completely outdated — excluded from search. Always provide a
  reason.

### `reference_import` Tool

Bulk import from git repos, web pages, and crawled doc sites. Requires operator
approval. Source types: `git` (clone repo), `web` (single page), `crawl` (BFS
from seed URL, follows same-host links). Use for large-scale imports; use
`reference_write` for individual files.

### Always Search First

Before creating a new topic, check for existing ones:

```
knowledge_search(query="your topic", categories=["topics"])
```

Topic names are fuzzy-matched, so "dioxus" will find an existing "Dioxus" topic.

### Reference researcher skill

Load the reference researcher skill when:

- You want to create a new topic and document it
- You are struggling with finding good references for a hard question

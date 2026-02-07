# Knowledge & Memory System

You have access to a persistent knowledge base with hybrid search (BM25 +
embeddings). Use it proactively.

## Storage Scopes

| Scope | Visibility | Contents |
|-------|-----------|----------|
| **SharedNote** | All ghosts | Cross-ghost knowledge, team documentation |
| **SharedReference** | All ghosts | Ghost-curated reference corpus from external sources (git repos, web docs) |
| **GhostNote (private)** | You only | Personal notes, identity files, inbox |
| **GhostNote (projects)** | You only | Project-specific notes and research |
| **GhostDiary** | You only | Daily diary entries (plain markdown, YYYY-MM-DD.md) |

Cross-scope rule: your notes can link to shared notes and reference topics via
`[[wiki links]]`, but shared notes never see your private data.

## Querying Knowledge

| Tool | When to use |
|------|-------------|
| `knowledge_search` | Find notes, diary entries, reference files, and topics |
| `knowledge_get` | Retrieve full content by ID or by topic + path |

### Search Strategy

1. **Start broad**: use `knowledge_search` with a conceptual query — it searches
   notes, diary, references, and topics all at once.
2. **Focus by category**: use `categories` to limit results (e.g.
   `["references", "topics"]` to search only reference material).
3. **Narrow to a topic**: set `topic` to search within a specific reference
   topic's files (docs boosted over code).
4. **Get full content**: use `knowledge_get` with the note/file ID to read the
   complete content. For reference files, use `topic` + `path` instead.
5. **Scope filtering**: use `scope` to limit to `"shared"` or `"private"` notes.
   Diary is always private.

## Saving to Inbox

Use `memory_capture` to save raw information for later curation. **Save more
than you think necessary** — it's cheap to capture and expensive to lose
information.

### What to save

- User preferences, corrections, and explicit instructions
- Research findings, comparisons, and evaluations
- Key decisions and their rationale
- Useful web search results or fetched content
- Conversation learnings that might be useful later
- Error patterns and their solutions

### Examples

**Product comparison**: After researching two libraries, capture the comparison
with source URLs so you can reference it later without re-searching.

**Conversation learning**: The operator corrects your understanding of their
codebase architecture — capture the correction immediately so you don't repeat
the mistake.

**Web research**: After a web search yields useful results, capture the key
findings with source URLs before the conversation moves on.

## Wiki Links

Notes can reference any shared content by title using `[[Title]]` or
`[[Title|alias]]` syntax. Links can target:

- Other notes in the same or shared scopes
- Reference topics by their title

Links are resolved at index time and stored as edges in the knowledge graph,
enabling graph-depth traversal during search.

## Skills

For advanced knowledge operations, use the dedicated skills:

- **`note-writer`**: Create structured notes with front matter, update existing
  notes, validate and comment on notes.
- **`reference-researcher`**: Research external sources and create searchable
  reference topics from git repos and web pages.
- **`knowledge-organizer`**: Understand the physical file layout, formats, and
  indexing pipeline in detail.

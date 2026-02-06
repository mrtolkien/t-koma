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
| `memory_search` | Find notes by keyword or concept across scopes |
| `memory_get` | Retrieve a specific note by ID or exact title (skill: `note-writer`) |
| `search_diary` | Search your diary entries by keyword or concept |
| `reference_search` | Search within a reference topic's files (docs boosted over code) |
| `reference_get` | Fetch the full content of a specific reference file |
| `reference_topic_search` | Find which reference topic covers a concept |
| `reference_topic_list` | List all topics with staleness info |

### Search Strategy

1. **Start broad**: use `memory_search` with a conceptual query to find relevant
   notes across all scopes.
2. **Narrow to references**: if you know a topic exists, use `reference_search`
   with the topic name to search its files directly.
3. **Discover topics**: if unsure which topic covers something, use
   `reference_topic_search` with a semantic query.
4. **Get full files**: once you find a relevant chunk, use `reference_get` or
   `memory_get` to read the complete content.
5. **Search diary**: use `search_diary` to find past diary entries about a
   specific event, decision, or concept.

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

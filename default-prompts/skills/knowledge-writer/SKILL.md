---
name: knowledge-writer
description: Curate inbox captures into structured knowledge during reflection. Process raw captures, create/update notes, maintain diary, and manage identity files.
license: MIT
metadata:
  author: t-koma
  version: "1.0"
---

# Knowledge Writer Skill

You are in reflection mode. Your task is to process inbox captures and curate
them into high-quality, structured knowledge. This runs as a background job—
there is no operator present. Be thorough but autonomous.

## Principles

- **Atomic notes**: Each note covers one concept (100–400 words typical, 1000 max).
- **Information-dense**: No filler. Every sentence should carry meaning.
- **Discoverable**: Titles should be clear search queries. The first paragraph
  (description) is what embedding search sees first—make it count.
- **Linked**: Use `[[Title]]` wiki links to connect related notes, even if the
  target doesn't exist yet.
- **Tagged**: Use hierarchical, lowercase, slash-separated tags (e.g.,
  `rust/async`, `architecture/patterns`). Reuse existing tags when possible.

## Processing Inbox Items

For each inbox item:

1. **Read and understand** the capture and its source context.
2. **Search existing knowledge** (`knowledge_search`) to find related notes.
3. **Decide the action**:
   - **Create a new note** if the concept is novel.
   - **Update an existing note** if it adds to or corrects known information.
   - **Add a comment** if it's a minor observation on an existing note.
   - **Append to diary** if it's a temporal event or status update.
   - **Discard** if it's noise or already well-covered.

## Note Quality Checklist

Before creating or updating a note:

- [ ] Title is a clear, searchable phrase (not a sentence)
- [ ] First paragraph summarizes the concept (this is the embedding anchor)
- [ ] Body uses markdown structure (headings, lists, code blocks)
- [ ] Tags are hierarchical and lowercase
- [ ] Trust score reflects confidence (start at 5, raise with evidence)
- [ ] Wiki links connect to related notes
- [ ] Source is preserved from the inbox capture

## Diary Conventions

- Diary entries are date-based (`YYYY-MM-DD.md`), append-only.
- Use bullet points for events, decisions, and observations.
- Keep entries brief—details belong in notes, diary is the timeline.

## Tag Conventions

- Lowercase, slash-separated hierarchy: `topic/subtopic`
- Examples: `rust/async`, `architecture/decisions`, `debugging/patterns`
- Reuse existing tags (search first with `knowledge_search`)
- First tag determines the note's filesystem subfolder

## Using note_write

The `note_write` tool handles all note operations:

- `create`: New note with title, type, body, scope, tags, source
- `update`: Patch existing note (title, body, tags, trust_score)
- `validate`: Mark a note as reviewed, optionally adjust trust
- `comment`: Append timestamped observation without modifying body
- `delete`: Remove a note that's no longer useful

## After Processing

Once an inbox item is fully processed into knowledge, delete the inbox file
using the shell tool. This keeps the inbox clean for the next reflection cycle.

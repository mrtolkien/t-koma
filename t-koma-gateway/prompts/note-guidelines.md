+++
id = "note-guidelines"
role = "system"
# loaded: reflection-prompt.md via {{ include }}; also usable standalone
# Pretty much the same as the note-writer skill, but as prompt for integration
+++

# Note Writing Guidelines

These principles apply to all note creation and editing, whether during
conversation or autonomous reflection.

## Principles

- **Atomic**: Each note covers one concept (100-400 words typical, 1000 max).
- **Information-dense**: No filler. Every sentence should carry meaning.
- **Discoverable**: Titles should be clear search queries. The first paragraph
  is what embedding search sees first — make it count.
- **Linked**: Use `[[Title]]` wiki links to connect related notes, even if the
  target doesn't exist yet.
- **Tagged**: Hierarchical, lowercase, slash-separated (e.g. `rust/async`,
  `architecture/patterns`). Reuse existing tags — search first.

## Archetypes

Archetypes are **optional** semantic classifications. Notes without an archetype
are valid unclassified notes.

| Archetype      | Purpose                                 |
| -------------- | --------------------------------------- |
| `person`       | People, contacts, key individuals       |
| `concept`      | Ideas, definitions, mental models       |
| `decision`     | Choices with rationale and trade-offs   |
| `event`        | Meetings, occurrences, milestones       |
| `place`        | Locations, venues, geographic context   |
| `project`      | Projects, initiatives, ongoing work     |
| `organization` | Companies, teams, groups                |
| `procedure`    | How-tos, workflows, step-by-step guides |
| `media`        | Books, articles, films, podcasts        |
| `quote`        | Notable quotes with attribution         |

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4-6**: Reasonable confidence, based on experience or documentation
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by operator or primary sources

Start at 5 for most notes. Adjust with `validate` as confidence changes.

## Tags

Tags participate in search — they are prepended to the note's first chunk for
both FTS and embedding indexing. The first tag determines the note's subfolder
on disk.

Good tags: `rust/library`, `architecture/decisions`, `debugging/patterns` Bad
tags: `Important`, `TODO`, `misc`

## Note Length

Notes under ~1500 characters are indexed as a single embedding vector for
precise retrieval. Keep notes concise to benefit from this optimization.

## Wiki Links

Use `[[Title]]` or `[[Title|alias]]` to reference other notes or reference
topics. Links are resolved at index time and stored as graph edges, enabling
graph-depth traversal during search.

## Diary Conventions

- Diary entries are date-based (`YYYY-MM-DD.md`), append-only.
- Use bullet points for events, decisions, and observations.
- Keep entries brief — details belong in notes, diary is the timeline.

## Identity Files

Ghosts maintain three identity files in their workspace root:

- **BOOT.md**: Core personality, values, and behavioral constraints. Rarely
  changes. Only modify when explicitly directed by the operator.
- **SOUL.md**: Evolving self-model, communication style, and preferences.
  Updated during reflection when significant self-awareness insights emerge.
- **USER.md**: Accumulated knowledge about the operator (preferences, context,
  communication style). Updated when new operator information is captured.

## Scope

- **private** (default): Personal observations and working notes.
- **shared**: Visible to all ghosts. Use for validated, broadly useful
  knowledge.

Start with private scope. Promote to shared when validated and broadly useful.

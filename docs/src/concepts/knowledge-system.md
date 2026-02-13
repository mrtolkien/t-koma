# Knowledge System

The knowledge system gives ghosts persistent memory through notes, references, and diary
entries, backed by embeddings-based search.

## Scopes

Knowledge is organized into five scopes:

| Scope             | Visibility | Owner | Description              |
| ----------------- | ---------- | ----- | ------------------------ |
| `SharedNote`      | All ghosts | None  | Shared knowledge notes   |
| `SharedReference` | All ghosts | None  | Shared reference topics  |
| `GhostNote`       | One ghost  | Ghost | Private notes            |
| `GhostReference`  | One ghost  | Ghost | Private reference topics |
| `GhostDiary`      | One ghost  | Ghost | Date-based diary entries |

**Rule**: shared notes must not contain private ghost data. Ghost notes can link to
shared notes via `[[Title]]` wiki links.

## Storage Layout

```text
$DATA_DIR/
├── shared/
│   ├── notes/          # SharedNote files
│   └── references/     # SharedReference topics
└── ghosts/
    └── $slug/
        ├── notes/      # GhostNote files
        ├── references/ # GhostReference topics
        ├── diary/      # GhostDiary entries
        ├── skills/     # Skill files
        └── .web-cache/ # Transient web cache (auto-cleared)
```

Notes are organized into tag-based subfolders derived from the first tag at creation
time.

## Note Classification

Notes have two classification axes:

- **Entry type** (structural): `Note`, `ReferenceDocs`, `ReferenceCode`, `Diary`
- **Archetype** (semantic, optional): `person`, `concept`, `decision`, `event`, `place`,
  `project`, `organization`, `procedure`, `media`, `quote`, `topic`

## Reference Model

References use a **Topic Note → Directory → File** structure:

1. A **topic note** (shared note) describes the reference topic
2. A **directory** provides optional sub-grouping
3. **Reference files** hold individual content units with per-file metadata in the DB

## Tools

### Chat Tools (Interactive)

Query-oriented tools available during conversations: search, get, web fetch/search,
filesystem operations, reference import.

### Reflection Tools (Background)

Knowledge-writing tools used during autonomous reflection: note/reference/diary writes,
reference management, identity updates.

## Search and Retrieval

`knowledge_search` provides hybrid retrieval across all knowledge types with filtering
by scope, category, topic, and archetype. `knowledge_get` retrieves full content by ID
or topic path.

## Web Cache

Web results from `web_fetch` and `web_search` are auto-saved as plain files in
`.web-cache/` during chat. The reflection system curates these into proper reference
topics, then `.web-cache/` is auto-cleared.

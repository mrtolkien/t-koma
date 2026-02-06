# Ghost Reference Management

> Spec for making ghosts autonomous in gathering, curating, and maintaining
> reference knowledge.

## Problem

Ghosts currently cannot create reference content. When a ghost needs knowledge
about a cutting-edge library (e.g., Dioxus), it has no way to:

1. Fetch the library's source code and documentation
2. Index it with embeddings for precise retrieval
3. Track when the content was fetched and whether it's stale
4. Search for existing topics semantically (different models may use different
   names)

Reference content is not training data — it's **reliable information gathered by
the ghost for future runs**. The ghost should manage it like a researcher
manages their reference library.

## Design Principles

- **topic.md is a note** — created by the ghost with the same front matter
  system as any other note, but with `type = "ReferenceTopic"`.
- **Ghost curates** — the ghost reads source material, understands the topic,
  and writes the topic description. The system handles fetching and indexing.
- **Sources are tracked** — every topic records where its files came from (git
  URL, web URL) so the gateway can refresh them later.
- **Staleness is front-matter-driven** — `fetched_at` + `max_age_days`
  determines freshness. The ghost can mark topics as obsolete.
- **Synchronous indexing** — the tool blocks until all files are fetched,
  chunked, and embedded. The ghost can search immediately after creation.
- **Always shared** — references are factual content, visible to all ghosts.
- **Use `gh` CLI for git** — leverages existing GitHub auth, no separate
  SSH/token config.
- **Default to fetching everything** — source code + docs. The embedding system
  handles large codebases well.
- **Human validation before fetch** — before the heavy work starts, the tool
  gathers repo metadata (file count, size estimate) and requests operator
  approval. Uses the existing `APPROVAL_REQUIRED:` mechanism from
  `tools/context.rs`, extended with structured approval reasons.

## Topic Discovery

The ghost needs to find existing topics before creating duplicates. Two
mechanisms work together:

### 1. Recent Topics in System Prompt

The **10 most recently fetched** reference topics are injected into the ghost's
context during `add_ghost_prompt_context()` in `session.rs`. Format:

```markdown
## Available Reference Topics

- Dioxus - Rust UI Framework (`topic-dioxus-ui`) — rust, ui, framework
- SQLite-Vec KNN (`topic-sqlite-vec`) — sqlite, embeddings
```

This gives the ghost instant awareness of existing references without a tool
call. The query is cheap (single DB query on `reference_topics` table, ordered
by `fetched_at DESC LIMIT 10`). No need for staleness/fetch dates here —
these are the 10 most recent, so they should be fresh.

### 2. `reference_topic_search` Tool

For topics not in the prompt (older or many topics), the ghost uses semantic
search over topic titles and descriptions via embeddings:

```json
{
  "query": "Rust GUI framework with React-like syntax"
}
```

Returns matching topics ranked by embedding similarity, so different models
using different terminology ("Dioxus", "Rust UI", "RSX framework") all find the
same topic.

This searches the `ReferenceTopic` notes (not their files) using the existing
hybrid BM25 + dense search pipeline.

## Example: Dioxus Workflow

```
Ghost is asked to write Dioxus code
  │
  ├─ Ghost checks system prompt: no "dioxus" in Available Reference Topics
  │
  ├─ reference_topic_search(query="Rust UI framework dioxus") → empty
  │
  ├─ Ghost activates the "reference-researcher" skill
  │  (default skill that teaches research best practices)
  │
  ├─ Ghost researches:
  │  1. gh search repos "dioxus" --language=Rust → finds DioxusLabs/dioxus
  │  2. gh api repos/DioxusLabs/dioxus → reads description, stars, topics
  │  3. web_fetch("https://dioxuslabs.com") → reads landing page
  │  4. Ghost now understands what Dioxus is
  │
  ├─ reference_topic_create:
  │    title: "Dioxus - Rust UI Framework"
  │    body: "Dioxus is a portable, performant framework for building
  │           cross-platform UIs in Rust. Uses RSX syntax similar to JSX..."
  │    sources:
  │      - type: git, url: https://github.com/DioxusLabs/dioxus, ref: main
  │      - type: web, url: https://dioxuslabs.com/learn/0.6/
  │    tags: [rust, ui, framework, dioxus]
  │    max_age_days: 30
  │
  │  → PHASE 1 (metadata): Tool queries GitHub API for repo stats
  │    "DioxusLabs/dioxus: 1,247 files, ~98 MB, Rust"
  │
  │  → APPROVAL_REQUIRED: operator sees:
  │    "Create reference topic 'Dioxus - Rust UI Framework'?
  │     Sources: DioxusLabs/dioxus (1,247 files, ~98 MB) + 1 web page
  │     Approve / Deny"
  │
  │  → Operator approves
  │
  │  → PHASE 2 (fetch): gh repo clone, web fetch, store files
  │  → PHASE 3 (index): chunk with tree-sitter, compute embeddings
  │  → Returns { topic_id, file_count: 1248, chunk_count: 4521 }
  │
  ├─ reference_search(topic="dioxus", question="component lifecycle hooks")
  │  → Returns relevant code chunks from the indexed source
  │
  └─ Ghost writes code with precise, up-to-date knowledge
```

### What if the operator denies?

When the operator denies, the tool returns an error to the ghost. The ghost
should then suggest a smaller scope — the skill teaches this fallback:

```
  │  → Operator denies
  │
  │  → Ghost adjusts: "The full repo is large. Let me fetch just the
  │    docs and examples instead."
  │
  ├─ reference_topic_create (retry with paths filter):
  │    sources:
  │      - type: git, url: ..., paths: [README.md, docs/, examples/]
  │      - type: web, url: ...
  │
  │  → Metadata: "341 files, ~12 MB (filtered from DioxusLabs/dioxus)"
  │  → Operator approves
  │  → Proceeds with filtered fetch
```

## Front Matter: Extended Source Tracking

The existing `SourceEntry` (`path` + `checksum`) is too limited. We need richer
source descriptors that enable tracking and future automated refresh.

### New `TopicSource` (replaces `SourceEntry` for reference topics)

```toml
[[sources]]
type = "git"
url = "https://github.com/DioxusLabs/dioxus"
ref = "main"
commit = "a1b2c3d"     # populated after fetch, tracks exact version
paths = ["README.md", "docs/", "examples/"]

[[sources]]
type = "web"
url = "https://dioxuslabs.com/learn/0.6/"
```

### New Topic-Level Fields

```toml
status = "active"       # active | stale | obsolete
fetched_at = "2025-06-15T10:00:00Z"
max_age_days = 30       # 0 = never auto-stale
```

### Full topic.md Example

```markdown
+++
id = "topic-dioxus-ui"
title = "Dioxus - Rust UI Framework"
type = "ReferenceTopic"
created_at = "2025-06-15T10:00:00Z"
trust_score = 8
tags = ["rust", "ui", "framework", "dioxus"]
status = "active"
fetched_at = "2025-06-15T10:00:00Z"
max_age_days = 30
files = ["README.md", "docs/guide/index.md", "examples/hello_world.rs"]

[created_by]
ghost = "ghost-a"
model = "claude-sonnet-4-5-20250929"

[[sources]]
type = "git"
url = "https://github.com/DioxusLabs/dioxus"
ref = "main"
commit = "a1b2c3d4e5f6"
paths = ["README.md", "docs/", "examples/"]

[[sources]]
type = "web"
url = "https://dioxuslabs.com/learn/0.6/"
+++

# Dioxus - Rust UI Framework

Dioxus is a portable, performant, and ergonomic framework for building
cross-platform user interfaces in Rust. It uses a virtual DOM with RSX syntax
similar to React's JSX, but compiles to native code.

Key concepts:

- Components are functions that return `Element`
- State management via `use_signal` hooks
- ...
```

### Staleness Logic

```
is_stale(topic):
  if max_age_days == 0: return false
  if status == "obsolete": return true  (manually marked)
  return (now - fetched_at).days > max_age_days
```

- **active**: Fresh. Normal search behavior.
- **stale**: Auto-computed. Still appears in search, but results include a
  `stale_since` field so the ghost can decide to act.
- **obsolete**: Manually set by ghost. Excluded from `reference_search` results.
  Can still be retrieved by `reference_topic_list`.

## Tools

### 1. `reference_topic_create`

Create a new reference topic. Fetches content from sources, stores files, writes
topic.md, indexes everything with embeddings.

**Input:**

```json
{
  "title": "Dioxus - Rust UI Framework",
  "body": "Ghost-written description after reading the source material...",
  "sources": [
    {
      "type": "git",
      "url": "https://github.com/DioxusLabs/dioxus",
      "ref": "main",
      "paths": ["README.md", "docs/", "examples/"]
    },
    {
      "type": "web",
      "url": "https://dioxuslabs.com/learn/0.6/"
    }
  ],
  "tags": ["rust", "ui", "framework"],
  "max_age_days": 30,
  "trust_score": 8
}
```

**Behavior (two-phase with operator approval):**

**Phase 1 — Metadata gathering (lightweight, no cloning yet):**

1. For each git source: query repo metadata via `gh api repos/{owner}/{repo}` to
   get file count, repo size, default branch, and description. For non-GitHub
   git: use `git ls-remote` to verify accessibility.
2. For web sources: just count the URLs (no fetching yet).
3. Build a human-readable summary:
   `"DioxusLabs/dioxus: 1,247 files, ~98 MB, Rust + 1 web page"`
4. Return `APPROVAL_REQUIRED:REFERENCE_TOPIC_CREATE:{summary}` — this triggers
   the existing approval flow in `session.rs`.

**On operator approval — Phase 2 (fetch + index):**

5. Create topic directory under `reference_root/<sanitized-title>/`
6. For each source:
   - **git**: Use `gh repo clone` (or `git clone --depth 1 --sparse` for
     non-GitHub repos) into temp dir, sparse-checkout paths if specified, copy
     matching files to topic dir. Record `commit` SHA via `git rev-parse HEAD`.
   - **web**: HTTP fetch + HTML-to-markdown conversion. Save as `<url-slug>.md`
     in topic dir.
7. Write `topic.md` with front matter (sources, fetched_at, files list)
8. Index topic + all files (chunks, FTS5, embeddings)
9. Return `{ topic_id, file_count, chunk_count }`

**On operator denial:**

- Tool returns an error to the ghost explaining the denial.
- The skill instructs the ghost to retry with a `paths` filter for a smaller
  subset (e.g., just `docs/` and `examples/`).

**Errors:**

- Source fetch failures are non-fatal per source (log warning, continue with
  remaining sources). Fail only if ALL sources fail.
- Empty result (no files fetched) is an error.

### 2. `reference_topic_search`

Semantic search over existing reference topics. Uses the hybrid BM25 + dense
embedding pipeline on `ReferenceTopic` notes only.

**Input:**

```json
{
  "query": "Rust GUI framework with React-like syntax"
}
```

**Output:**

```json
[
  {
    "topic_id": "topic-dioxus-ui",
    "title": "Dioxus - Rust UI Framework",
    "status": "active",
    "is_stale": false,
    "fetched_at": "2025-06-15T10:00:00Z",
    "tags": ["rust", "ui", "framework", "dioxus"],
    "score": 0.87,
    "snippet": "Dioxus is a portable, performant framework for building..."
  }
]
```

This reuses the existing `search_reference_topics()` function from
`engine/reference.rs` but exposes it directly (currently it's internal to
`reference_search` which uses it to pick the top topic, then searches files).

### 3. `reference_topic_list`

List all reference topics with staleness information.

**Input:**

```json
{
  "include_obsolete": false
}
```

**Output:**

```json
[
  {
    "topic_id": "topic-dioxus-ui",
    "title": "Dioxus - Rust UI Framework",
    "status": "active",
    "is_stale": false,
    "fetched_at": "2025-06-15T10:00:00Z",
    "max_age_days": 30,
    "created_by_ghost": "ghost-a",
    "source_count": 2,
    "file_count": 12,
    "tags": ["rust", "ui", "framework"]
  }
]
```

### 4. `reference_topic_update`

Update topic metadata without re-fetching sources.

**Input:**

```json
{
  "topic_id": "topic-dioxus-ui",
  "status": "obsolete",
  "max_age_days": 60,
  "body": "Updated description...",
  "tags": ["rust", "ui"]
}
```

All fields except `topic_id` are optional patches.

### CLI-Only Operations (Not Ghost Tools)

The following operations are administrative and will be exposed through the
CLI/TUI, not as ghost-facing tools (see AGENTS.md tool design rules):

- **Refresh**: Re-fetch from tracked sources, re-index changed files. Will be
  handled by the gateway in a future iteration.
- **Delete**: Remove a topic, its files, and all indexed data. Destructive
  operation that belongs in operator-facing CLI.

## Source Types

### Git Sources

```
type: "git"
url: "https://github.com/org/repo"    # Required
ref: "main"                            # Optional, default: default branch
paths: ["README.md", "src/", "docs/"]  # Optional, default: ENTIRE REPO
commit: "abc123"                       # Set after fetch, tracked for refresh
```

**Default behavior: fetch the entire repo.** The embedding system handles large
codebases well (tree-sitter chunking, hybrid search). The operator approval step
gates the actual fetch, so the ghost should default to everything and only use
`paths` when the operator denies (too large) or the ghost knows only a subset is
relevant.

**Fetch strategy:**

For GitHub URLs (detected by `github.com` in URL):

```bash
# Use gh cli — leverages existing auth for private repos
gh repo clone org/repo "$tmp" -- --depth 1

# If paths specified (fallback after denial), use sparse-checkout
cd "$tmp"
git sparse-checkout init --cone
git sparse-checkout set $paths
```

For non-GitHub URLs:

```bash
git clone --depth 1 --filter=blob:none "$url" "$tmp"

# If paths specified
cd "$tmp"
git sparse-checkout init --cone
git sparse-checkout set $paths
```

After clone:

```bash
# Record commit for provenance
git rev-parse HEAD  # → stored as "commit" in source entry
# Copy files to topic dir, preserving relative paths
```

**Path handling (when `paths` is specified):**

- `"docs/"` → all files recursively under `docs/`
- `"README.md"` → single file
- Omitting `paths` → entire repo (default, recommended)

### Web Sources

```
type: "web"
url: "https://example.com/docs/guide/"   # Required
```

**Fetch strategy:**

- HTTP GET + HTML-to-markdown conversion (via `reqwest` + `html2text`)
- Save as `<url-slug>.md` in topic dir
- Single page per source entry — ghost lists each page as a separate source (the
  skill teaches this pattern)

## Default Skill: `reference-researcher`

A default skill embedded in the binary that teaches the ghost how to gather
reference material effectively. Located at
`default-prompts/skills/reference-researcher/SKILL.md`.

### What the Skill Teaches

1. **When to create references**: Cutting-edge libraries, complex APIs, any
   domain where the model's training data is likely outdated or insufficient.

2. **Research strategy** (priority order):
   - `gh search repos` and `gh api` to find and explore GitHub repos
   - `gh repo clone` for source code (with selective paths)
   - `web_search` to find documentation sites
   - `web_fetch` to read specific doc pages
   - Read enough to write a good topic description before creating

3. **Source selection best practices**:
   - Default to the entire repo — the embedding system handles large codebases
     well, and the operator approval step gates the fetch
   - If the operator denies (too large), retry with a `paths` filter: prioritize
     `README.md`, `docs/`, `examples/`, and key `src/` files
   - For web sources, list each page as a separate source (no crawling)
   - Add multiple web sources for different doc sections the ghost cares about

4. **Topic quality**:
   - Write a meaningful body that summarizes the library's purpose, key
     concepts, and common patterns
   - Use descriptive tags for discoverability
   - Set appropriate `max_age_days` (30 for actively developed libraries, 90 for
     stable ones, 0 for reference material that won't change)

5. **Before creating, always search first**:
   - Check the system prompt for Available Reference Topics
   - Use `reference_topic_search` for semantic matching
   - If a topic exists but is stale, consider updating it rather than creating a
     duplicate

6. **When to mark obsolete**:
   - Library has been deprecated or superseded
   - The indexed version is fundamentally incompatible with current version
   - Better reference material is now available

### Skill Registration

Add to `t-koma-core/src/default_skills.rs`:

```rust
pub const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        name: "skill-creator",
        content: include_str!("../../default-prompts/skills/skill-creator/SKILL.md"),
    },
    DefaultSkill {
        name: "reference-researcher",
        content: include_str!("../../default-prompts/skills/reference-researcher/SKILL.md"),
    },
];
```

## Implementation Plan

### Phase 0: Centralized Approval System

Extend the existing `APPROVAL_REQUIRED:` mechanism from workspace-only to a
general-purpose approval system.

1. **`tools/context.rs`**: Add `ApprovalReason` enum:

   ```rust
   pub enum ApprovalReason {
       WorkspaceEscape(String),          // existing pattern
       ReferenceTopicCreate {            // new
           title: String,
           summary: String,              // "1,247 files, ~98 MB"
       },
   }
   ```

   Add `fn to_approval_error(&self) -> String` that formats the
   `APPROVAL_REQUIRED:` error string with structured metadata. Add
   `fn parse_approval_reason(error: &str) -> Option<ApprovalReason>`.

2. **`session.rs`**: Update `PendingToolApproval` to store
   `Option<ApprovalReason>` instead of `Option<String>` for `requested_path`.
   Keep backward compatibility — the existing `approval_required_path()` check
   becomes a fallback.

3. **`server.rs`**: Update `approval_required_message()` to render
   human-readable approval prompts based on `ApprovalReason` variant.

4. **`messages/en/`**: Add message templates:
   - `approval-reference-topic-create`: "Create reference topic '{title}'?
     {summary}. Approve / Deny"

This is a reusable foundation — future tools that need operator confirmation
(e.g., large file operations, destructive actions) use the same mechanism.

### Phase 1: Extended Front Matter + Models

1. **`parser.rs`**: Add `TopicSource` struct with `source_type`, `url`, `ref_`,
   `commit`, `paths` fields. Add `status`, `fetched_at`, `max_age_days` to
   `FrontMatter`. Keep existing `SourceEntry` for non-topic notes.
2. **`engine/notes.rs`**: Update `rebuild_front_matter` to serialize
   `TopicSource`, `status`, `fetched_at`, `max_age_days`.
3. **`models.rs`**: Add `TopicCreateRequest`, `TopicListEntry`,
   `TopicSearchResult` structs. Add `TopicStatus` enum (Active, Stale,
   Obsolete).

### Phase 2: Source Fetching

1. **New module: `t-koma-knowledge/src/sources.rs`**:
   - `fetch_git_source(source, target_dir) -> KnowledgeResult<Vec<PathBuf>>`
     Uses `tokio::process::Command` to run `gh repo clone` or `git clone`.
   - `fetch_web_source(url, target_dir) -> KnowledgeResult<PathBuf>` Uses
     `reqwest` + `html2text`.
   - `list_fetched_files(target_dir) -> Vec<String>` — walks dir, returns
     relative paths.
2. Add `reqwest` and `html2text` as dependencies to `t-koma-knowledge`.

### Phase 3: Engine Methods

1. **`engine/reference.rs`**: Add topic management functions:
   - `reference_topic_create(engine, context, request) -> TopicCreateResult`
   - `reference_topic_search(engine, query) -> Vec<TopicSearchResult>`
   - `reference_topic_list(engine, include_obsolete) -> Vec<TopicListEntry>`
   - `reference_topic_update(engine, context, request) -> NoteWriteResult`
2. **`engine/mod.rs`**: Expose public methods on `KnowledgeEngine`.
3. Update `reference_search` to exclude `status = "obsolete"` topics.
4. Add staleness computation (compare `fetched_at + max_age_days` to now).

### Phase 4: Topic Discovery

1. **`engine/reference.rs`**: Extract `search_reference_topics` into a public
   function for `reference_topic_search`.
2. **`session.rs`**: In `add_ghost_prompt_context()`, after projects, query DB
   for 10 most recent reference topics and inject them as a context block.
3. **`storage.rs`**: Add `list_reference_topics()` query for the prompt
   injection.

### Phase 5: Gateway Tools

Create tool files in `t-koma-gateway/src/tools/`:

- `reference_topic_create.rs`
- `reference_topic_search.rs`
- `reference_topic_list.rs`
- `reference_topic_update.rs`

Register in `manager.rs`. Only 4 ghost-facing tools — refresh and delete are
CLI/TUI operations (see AGENTS.md tool design rules).

### Phase 6: Default Skill

1. Create `default-prompts/skills/reference-researcher/SKILL.md`
2. Register in `t-koma-core/src/default_skills.rs`

### Phase 7: Tests

1. Unit tests for source fetching (mock git/web)
2. Integration test: ghost creates reference topic from local files, verifies
   search works
3. Test staleness computation
4. Test topic search with embeddings
5. Test obsolete topic exclusion from reference_search
6. **Full Dioxus integration test** (`slow-tests` feature): actually fetches
   the Dioxus repo + docs via `gh`/`reqwest`, creates a reference topic,
   indexes with real embeddings, and verifies `reference_search` finds
   relevant Dioxus code/docs. This is the end-to-end smoke test.

## Schema Changes

### `reference_topics` table: add columns

```sql
ALTER TABLE reference_topics ADD COLUMN sources_json TEXT;
ALTER TABLE reference_topics ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
ALTER TABLE reference_topics ADD COLUMN fetched_at TEXT;
ALTER TABLE reference_topics ADD COLUMN max_age_days INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reference_topics ADD COLUMN created_by_ghost TEXT;
ALTER TABLE reference_topics ADD COLUMN tags_json TEXT;
```

## Decisions

1. **Default to fetching everything** — the embedding system handles large
   codebases. Operator approval gates the fetch so there's a human check.
2. **Web sources are single-page** — ghost lists each page as a separate source.
   The skill teaches this pattern.
3. **References are always shared** — factual content visible to all ghosts.
   Per-ghost references may be added later.
4. **Use `gh` CLI for GitHub repos** — leverages existing auth. Fall back to
   plain `git` for non-GitHub URLs.
5. **No `reference_topic_refresh` or `reference_topic_delete` tools** — these
   are administrative operations handled by the gateway/CLI, not ghost tools.
   Too many tools confuses models (AGENTS.md tool design rules).

## Key Files

| Action | File                                                   |
| ------ | ------------------------------------------------------ |
| Modify | `t-koma-gateway/src/tools/context.rs`                  |
| Modify | `t-koma-gateway/src/session.rs`                        |
| Modify | `t-koma-gateway/src/server.rs`                         |
| Modify | `t-koma-knowledge/src/parser.rs`                       |
| Modify | `t-koma-knowledge/src/models.rs`                       |
| Modify | `t-koma-knowledge/src/engine/notes.rs`                 |
| Modify | `t-koma-knowledge/src/engine/reference.rs`             |
| Modify | `t-koma-knowledge/src/engine/mod.rs`                   |
| Modify | `t-koma-knowledge/src/storage.rs`                      |
| Modify | `t-koma-gateway/src/tools/manager.rs`                  |
| Modify | `t-koma-core/src/default_skills.rs`                    |
| Create | `t-koma-knowledge/src/sources.rs`                      |
| Create | `t-koma-gateway/src/tools/reference_topic_create.rs`   |
| Create | `t-koma-gateway/src/tools/reference_topic_search.rs`   |
| Create | `t-koma-gateway/src/tools/reference_topic_list.rs`     |
| Create | `t-koma-gateway/src/tools/reference_topic_update.rs`   |
| Create | `default-prompts/skills/reference-researcher/SKILL.md` |

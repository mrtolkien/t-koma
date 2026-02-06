# Knowledge & Memory System Spec (Draft)

## Goals

- Provide long-term memory for each ghost (private, project, diary,
  boot/soul/user files).
- Provide a shared knowledge base across all ghosts in XDG data.
- Deliver high-precision retrieval with low latency and low token cost.
- Ensure deterministic, tool-only API surface for the gateway.

## Mindset & Approach

- Retrieval quality first: prioritize precision and relevance over broad recall.
  The tools should return fewer, better results to reduce token usage and
  back-and-forth.
- Determinism: all reading/writing flows are tool-driven and auditable. No
  background LLM writes.
- Human oversight: everything is file-based and inspectable. Indexes are
  derived, never canonical.
- Incremental correctness: content hashes gate reindexing; periodic
  reconciliation prevents drift.
- Layered retrieval: sparse (BM25/FTS5) for exact term matching and dense
  embeddings for semantic recall, fused deterministically.

## Why Full-Text + Embeddings

- BM25 is strong for exact names, code symbols, and keywords; it avoids
  hallucinated relevance from embeddings alone.
- Embeddings capture paraphrase and conceptual similarity that BM25 misses.
- Hybrid retrieval reduces failure modes and is more robust across knowledge
  types (facts, how-tos, code, notes).

## Why These Tools

- SQLite + FTS5: fast, local, and easy to ship with no external infra.
- sqlite-vec: local vector search with simple deployment. Treat as replaceable
  if needed.
- Tree-sitter: structured code chunking for better precision on code-heavy
  reference sources.

## Non-Goals

- No transport-layer logic changes beyond tool wiring.
- No LLM-managed auto-writing without explicit tool calls.
- No live-tests.

## Storage Layout

### Shared Knowledge

- Root: `xdg_data/knowledge/`
- Notes are Markdown with TOML front matter.
- Optional hierarchical nesting: a note with `parent` is stored under `parent/`
  directory (slugged name).

### Reference Corpus

- Root: `xdg_data/reference/`
- Holds documentation, source code, and other reference materials.
- Topics are stored as plaintext Markdown notes (with front matter) describing
  the topic and listing files for `reference_search`.
  - Use `type = "ReferenceTopic"` with a `files = ["path"]` list in front matter.

### Ghost Memory (per ghost workspace)

- `workspace/projects/` for active projects, each with `README.md` (first
  paragraph describes topic).
- `workspace/projects/.archive/` for archived projects.
- `workspace/diary/YYYY-MM-DD.md` daily summaries; last 2 days loaded for new
  sessions.
- `workspace/private_knowledge/` same structure as shared knowledge, but
  private.
- `workspace/SOUL.md`, `workspace/USER.md`, `workspace/BOOT.md` always loaded.

## Note Format (Knowledge & Private Knowledge)

### Front Matter (TOML)

Required:

- `title`
- `id` (stable, globally unique identifier)
- `type` (see Note Types)
- `created_at` (RFC3339)
- `created_by.ghost`
- `created_by.model`
- `trust_score` (1..10)

Optional:

- `last_validated_at` (RFC3339)
- `last_validated_by.ghost`
- `last_validated_by.model`
- `comments[]` (array of objects: `ghost`, `model`, `at`, `text`)
- `parent` (note title or note id)
- `tags[]`
- `links[]` (resolved note ids; maintained by indexer)
- `source[]` (url/file path + optional checksum)
- `version` (int; increment on rewrite)

### Note Types

Initial canonical list (human-curated):

- Person
- Organization
- Book
- Event
- News
- Libraries
- Concept
- How-to
- Definition
- Idea
- Quote
- Place
- Project
- Article
- Video
- Music

Notes can declare new types in `type`, but they are ignored until validated via
a human-curated allowlist (TOML file of accepted types).

### Body

- Markdown, first paragraph states what the note is and its scope.
- Supports Obsidian-style wiki links `[[Note Name]]` and `[[Note Name|Alias]]`.
- Note titles are short, descriptive, unique within knowledge scope.

## Indexing & Retrieval

### Datastores

- SQLite main DB for metadata and graph edges.
- `sqlite-vec` for embeddings (single index with scope/owner filters).
- FTS5 for BM25 lexical search (single index with scope/owner filters).

### Chunking

- Markdown: chunk by section headings (H1..H6) into independent sections.
- Code: use tree-sitter to extract functions/classes; each node is a chunk.
- Initial language support: Rust, Python, JavaScript, TypeScript, Go.

### Embeddings

- Local Ollama endpoint, model `qwen3-embedding:8b` by default.
- Configurable provider URL/model in config.
- Store embedding per chunk with back-reference to note and section.

### Search

`memory_search` uses:

- Dense (sqlite-vec) + BM25 (FTS5) hybrid retrieval.
- Reciprocal Rank Fusion (RRF) to merge dense + sparse results.
- Graph expansion: include parent, tags, and 1-hop links (incoming/outgoing).

`reference_search`:

- Two-stage: first retrieve topic by embedding topic description, then search
  topic files.

`memory_get`:

- Return full note contents and metadata by name.

`memory_capture`:

- Append raw content to `inbox` (per-ghost + shared), later curated.

## Tool API Surface (Gateway)

- `memory_search(query, scope, options)`
- `memory_get(note_id_or_title, scope)`
- `memory_capture(payload, scope, target)`
- `reference_search(topic, question, options)`

Only these tools are exposed from the new knowledge crate.

## Sync & Indexing Strategy

- Incremental indexer watches filesystem roots.
- Update embeddings/fts/graph on file change.
- Avoid drift: per-document content hash; reindex only on hash changes.
- Batch maintenance: FTS5 `optimize` and vec index maintenance.
- Periodic reconciliation (every ~5 minutes) rechecks hashes to catch missed
  updates.

## Privacy & Safety

- Strict separation between shared and private indices.
- Path canonicalization for workspace boundaries.
- PII safety: no shared promotion of private notes without explicit tool.

## Testing

- Unit tests for parsing, chunking, link resolution, and storage.
- Integration tests for index sync, search fusion, and tool behavior.
- No snapshot updates.

## Open Questions

- Default RRF K value and tuning strategy.
- Title naming guidelines for disambiguation (e.g.,
  `Rust (programming language)`).
- Whether to store link edges for unresolved `[[Note]]` targets (recommended:
  yes).

## Implementation Plan

1. Spec validation and freeze
2. New crate scaffold (`t-koma-knowledge`) with module boundaries
3. Config surface for embedding provider/model and index paths
4. SQLite schema + migrations (metadata, graph, FTS5, sqlite-vec)
5. Note parser (front matter, IDs, links)
6. Chunkers (Markdown + tree-sitter for Rust/Python/JS/TS/Go)
7. Embedding client (Ollama, batching, retries, version metadata)
8. Indexer + reconciliation loop (hash-gated)
9. Hybrid search (BM25 + dense + RRF + graph expansion + trust bias)
10. Tool API exposure in gateway
11. Ghost memory integration (projects/diary/private/shared)
12. Maintenance + performance tuning (FTS optimize, vec maintenance)
13. Documentation in `vibe/knowledge/`

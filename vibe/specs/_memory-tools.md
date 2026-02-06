# Memory Tools Quick Spec (Draft)

## Goals

- Define the tool surface for the knowledge/memory system.
- Keep the tools deterministic and auditable.
- Provide enough structure to enforce data validity in code.

## Scope

- Covers tool behavior, parameters, defaults, and expected outputs.
- Implementation notes are included per tool.
- Does not change any functionality yet.

## Tool: memory_search

### Purpose

Search knowledge and memory using hybrid retrieval (BM25 + embeddings) and graph
expansion.

### Inputs

- `query` (string, required)
- `scope` (enum, optional)
  - `shared_only`
  - `private_only`
  - `projects_only`
  - `diary_only`
  - `all` (default)
- `options` (object, optional)
  - `max_results` (int, default from config)
  - `bm25_weight` (float)
  - `embedding_weight` (float)
  - `rrf_k` (int)
  - `graph_depth` (int, default 1)
  - `include_links_in` (bool, default true)
  - `include_links_out` (bool, default true)
  - `include_parent` (bool, default true)
  - `trust_bias` (float, default from config)

### Output

- List of `MemoryResult`:
  - `summary` (note id/title/type/scope/score/snippet/trust)
  - `parents` (summaries)
  - `links_in` (summaries)
  - `links_out` (summaries)
  - `tags` (string list)

### Implementation Notes

- Hybrid retrieval using BM25 + dense vectors and RRF fusion.
- Graph expansion performed after initial ranking.
- Scope enforcement must restrict private notes to the requesting ghost.
- Shared knowledge is always readable; private is only readable by owner.

## Tool: memory_get

### Purpose

Fetch full note content and metadata by id or title.

### Inputs

- `note_id_or_title` (string, required)
- `scope` (enum, optional; same as memory_search)

### Output

- `NoteDocument`:
  - `note` (metadata)
  - `body` (markdown)
  - `tags`, `links`, `comments`

### Implementation Notes

- Resolve by exact id first, then by title within allowed scope.
- Enforce ghost ownership for private scopes.

## Tool: memory_capture

### Purpose

Store raw, unstructured info into inbox for later curation.

### Inputs

- `payload` (string, required)
- `scope` (enum, optional; defaults to `private_only`)

### Output

- `path` (string): file path written to inbox

### Implementation Notes

- Writes a timestamped markdown file into the correct inbox.
- No automatic parsing/indexing; reconciliation handles later.
- Must respect scope and ownership.

## Tool: reference_search

### Purpose

Search reference topics and their files (docs/source code).

### Inputs

- `topic` (string, required)
- `question` (string, required)
- `options` (object, optional)
  - `max_results` (int)
  - `rrf_k` (int)

### Output

- List of `MemoryResult` for matched reference files.

### Implementation Notes

- Stage 1: embed `topic` string and retrieve best topic note.
- Stage 2: run hybrid search over files listed by that topic.
- Topics are maintained as plaintext markdown notes.

## Tool: memory_note_create (planned)

### Purpose

Create a structured note with validated front matter.

### Inputs

- `title`, `type`, `scope`, `body` (required)
- `parent`, `tags`, `source`, `trust_score` (optional)

### Output

- `note_id`, `path`

### Implementation Notes

- Generates stable id.
- Validates type against allowlist.
- Enforces parent folder placement if parent is provided.
- Writes atomically and queues indexing.

## Tool: memory_note_update (planned)

### Purpose

Update an existing note with structured changes.

### Inputs

- `note_id` (required)
- `patch` (front matter/body changes)

### Output

- `note_id`, `path`

### Implementation Notes

- Rewrites front matter deterministically.
- Increments `version`.
- Recomputes hash.

## Tool: memory_note_validate (planned)

### Purpose

Record validation metadata and adjust trust.

### Inputs

- `note_id` (required)
- `validated_by` (ghost+model)
- `trust_score` (optional)

### Output

- `note_id`

### Implementation Notes

- Updates `last_validated_at` and `last_validated_by`.
- Optional trust adjustment.

## Tool: memory_note_comment (planned)

### Purpose

Add a comment entry to front matter.

### Inputs

- `note_id` (required)
- `comment` (ghost+model+text)

### Output

- `note_id`

### Implementation Notes

- Appends comment entry with timestamp.

## Security & Ownership Rules

- Note titles must be unique within shared knowledge.
- Every private note must have `owner_ghost_id` or `owner_ghost_name` stored in
  DB.
- All private queries must enforce owner == requesting ghost.
- Shared notes are always readable; private notes are not readable by other
  ghosts.

## Open Decisions

- Final defaults for weights/rrf_k.

# Knowledge & Memory System (Concise)

Purpose: long-term, file-backed memory for ghosts and a shared knowledge base, with deterministic tool-only access and hybrid retrieval.

## Storage & Scope

**SHARED** (`xdg_data/knowledge/`): Markdown notes visible to ALL ghosts. Use for cross-ghost knowledge, team documentation, and reference material.

**PRIVATE** (ghost workspace): Owned by a single ghost, invisible to other ghosts.
- `workspace/private_knowledge/` — personal notes and inbox.
- `workspace/projects/` — project-specific notes.
- `workspace/diary/` — diary/log entries.
- `BOOT.md`, `SOUL.md`, `USER.md` — ghost identity files.

**REFERENCE** (`xdg_data/reference/`): System-maintained, read-only corpus. Topic notes with `type = "ReferenceTopic"` pointing to files.

**Cross-scope rule**: Ghost notes can LINK to shared notes via `[[wiki links]]`, but shared notes never see private data. Queries with `scope=all` return shared + own private, never another ghost's private notes.

## Note Format

Front matter (TOML between `+++`):
- Required: `id`, `title`, `type`, `created_at`, `created_by.{ghost,model}`, `trust_score`.
- Optional: `last_validated_*`, `comments[]`, `parent`, `tags[]`, `source[]`, `version`, `files[]`.
- Wiki links `[[Note]]` build a graph; unresolved links are preserved.
- `type` is ignored unless present in `types.toml` allowlist.

## Indexing

- Hash-based reindexing + 5-minute reconciliation to prevent drift.
- Markdown chunks by headings (small sections merged). Code chunks by tree-sitter functions/classes.
- Embeddings stored in sqlite-vec; BM25 in FTS5.

## Retrieval

- Hybrid: BM25 + embeddings, fused with RRF.
- Graph expansion includes parent, tags, and 1-hop links.

## Tools

- `memory_search`: hybrid search across shared + ghost memory. Default scope is `all`.
- `memory_get`: fetch full note by id/title. Resolves across allowed scopes.
- `memory_capture`: append raw info to inbox for later curation. Default scope is `ghost` (private).
- `reference_search`: find topic by embeddings, then search its files. Returns full topic.md body as LLM context plus ranked file chunks. Documentation sources are boosted over code.
- `reference_get`: fetch the full content of a reference file by note_id or topic+path. Use when you need the complete file, not just search snippets.
- `reference_file_update`: mark a reference file as active, problematic, or obsolete. Adds a warning to the topic page. Use when you discover outdated or incorrect reference content.
- `memory_note_create`: create a structured note with validated front matter.
- `memory_note_update`: patch an existing note (title, body, tags, trust).
- `memory_note_validate`: record validation metadata and adjust trust score.
- `memory_note_comment`: append a comment entry to a note.

## Defaults

- Embeddings via Ollama `http://127.0.0.1:11434`, model `qwen3-embedding:8b`.
- Precision over recall; return fewer, higher-quality results.

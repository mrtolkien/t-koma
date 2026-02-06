# Knowledge & Memory System (Concise)

Purpose: long-term, file-backed memory for ghosts and a shared knowledge base, with deterministic tool-only access and hybrid retrieval.

Storage layout:
- Shared knowledge: `xdg_data/knowledge/` Markdown notes with TOML front matter.
- Reference corpus: `xdg_data/reference/` topic notes with `type = "ReferenceTopic"` and `files = ["..."]`.
- Ghost memory: `workspace/projects/`, `workspace/diary/`, `workspace/private_knowledge/` plus `BOOT.md`, `SOUL.md`, `USER.md`.

Note format (front matter):
- Required: `id`, `title`, `type`, `created_at`, `created_by.{ghost,model}`, `trust_score`.
- Optional: `last_validated_*`, `comments[]`, `parent`, `tags[]`, `source[]`, `version`, `files[]`.
- Wiki links `[[Note]]` build a graph; unresolved links are preserved.
- `type` is ignored unless present in `types.toml` allowlist.

Indexing:
- Hash-based reindexing + 5-minute reconciliation to prevent drift.
- Markdown chunks by headings (small sections merged). Code chunks by tree-sitter functions/classes.
- Embeddings stored in sqlite-vec; BM25 in FTS5.

Retrieval:
- Hybrid: BM25 + embeddings, fused with RRF.
- Graph expansion includes parent, tags, and 1-hop links.

Tools:
- `memory_search`: hybrid search across shared + ghost memory.
- `memory_get`: fetch full note by id/title.
- `memory_capture`: append raw info to inbox for later curation.
- `reference_search`: find topic by embeddings, then search its files.

Defaults:
- Embeddings via Ollama `http://127.0.0.1:11434`, model `qwen3-embedding:8b`.
- Precision over recall; return fewer, higher-quality results.

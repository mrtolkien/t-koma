# Knowledge System Notes

- Shared knowledge root: `${XDG_DATA_HOME:-~/.local/share}/t-koma/knowledge`
- Reference root: `${XDG_DATA_HOME:-~/.local/share}/t-koma/reference`

## Note Format

- Markdown with TOML front matter delimited by `+++`.
- Required fields: `id`, `title`, `type`, `created_at`,
  `created_by.{ghost,model}`, `trust_score`.
- Optional: `last_validated_*`, `comments[]`, `parent`, `tags[]`, `source[]`,
  `version`, `files[]` (reference topics).

## Type Allowlist

- `types.toml` under shared knowledge root controls accepted `type` values.
- Example:

```toml
types = ["Person", "Organization", "Concept"]
```

## Prompts

- Concise LLM-facing prompts live in `t-koma-knowledge/knowledge/prompts`.

## Indexing Behavior

- Hash-based reindexing; unchanged files are skipped.
- 5-minute reconciliation loop stored in `meta.last_reconcile_shared` and
  `meta.last_reconcile_ghost:{ghost_name}`.
- Hybrid retrieval uses BM25 (FTS5) + embeddings (sqlite-vec) + RRF fusion.
- Reference topics are markdown notes under `reference/` with
  `type = "ReferenceTopic"` and `files = ["path"]` in front matter.

## Ownership Enforcement (Crucial)

- Private knowledge rows must always carry `owner_ghost`.
- All private queries must enforce `owner_ghost = requesting ghost`.
- Shared/reference rows must never set `owner_ghost`.
- Treat any missing ownership enforcement as a security bug.

## Embeddings

- Default embedding provider: local Ollama `http://127.0.0.1:11434`.
- Default model: `qwen3-embedding:8b`.
- Embeddings are inserted into `chunk_vec` virtual table; vector dimension is
  inferred from first embed if not configured.

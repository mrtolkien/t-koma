-- Prompt cache: store rendered system prompt blocks to survive gateway restarts.

CREATE TABLE IF NOT EXISTS prompt_cache (
    session_id TEXT PRIMARY KEY,
    system_blocks_json TEXT NOT NULL,
    context_hash TEXT NOT NULL,
    cached_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

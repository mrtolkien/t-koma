CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    note_type TEXT NOT NULL,
    type_valid INTEGER NOT NULL DEFAULT 1,
    path TEXT NOT NULL UNIQUE,
    scope TEXT NOT NULL,
    owner_ghost TEXT,
    created_at TEXT NOT NULL,
    created_by_ghost TEXT NOT NULL,
    created_by_model TEXT NOT NULL,
    trust_score INTEGER NOT NULL,
    last_validated_at TEXT,
    last_validated_by_ghost TEXT,
    last_validated_by_model TEXT,
    version INTEGER,
    parent_id TEXT,
    comments_json TEXT,
    content_hash TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (scope IN ('ghost_private','ghost_projects','ghost_diary') AND owner_ghost IS NOT NULL)
        OR (scope IN ('shared','reference') AND owner_ghost IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS note_tags (
    note_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY(note_id, tag)
);

CREATE TABLE IF NOT EXISTS note_links (
    source_id TEXT NOT NULL,
    target_title TEXT NOT NULL,
    target_id TEXT,
    alias TEXT,
    owner_ghost TEXT,
    PRIMARY KEY(source_id, target_title, alias)
);

CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    note_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    embedding_model TEXT,
    embedding_dim INTEGER,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(
    content,
    title,
    note_title,
    note_type,
    note_id UNINDEXED,
    chunk_id UNINDEXED
);

CREATE TABLE IF NOT EXISTS reference_topics (
    topic_id TEXT PRIMARY KEY,
    files_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reference_files (
    topic_id TEXT NOT NULL,
    note_id TEXT NOT NULL,
    path TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'code',
    status TEXT NOT NULL DEFAULT 'active',
    PRIMARY KEY(topic_id, note_id)
);

CREATE INDEX IF NOT EXISTS idx_notes_owner_scope ON notes(owner_ghost, scope);
CREATE INDEX IF NOT EXISTS idx_links_owner_source ON note_links(owner_ghost, source_id);

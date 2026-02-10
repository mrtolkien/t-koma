-- GHOST (ゴースト) database schema
-- Stores sessions and messages for a single ghost

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    operator_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_active INTEGER DEFAULT 1 CHECK (is_active IN (0, 1)),
    compaction_summary TEXT,
    compaction_cursor_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_operator_id ON sessions(operator_id);
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('operator', 'ghost')),
    content TEXT NOT NULL,
    model TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_messages_created_at ON messages(created_at);

CREATE TABLE IF NOT EXISTS job_logs (
    id TEXT PRIMARY KEY,
    job_kind TEXT NOT NULL CHECK (job_kind IN ('heartbeat', 'reflection')),
    session_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    status TEXT,
    transcript TEXT NOT NULL DEFAULT '[]',
    todo_list TEXT,
    handoff_note TEXT,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_job_logs_session_kind
    ON job_logs(session_id, job_kind, started_at DESC);

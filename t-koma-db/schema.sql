-- T-KOMA (ティーコマ) unified database schema
-- Single database for gateway and all ghost data

-- ============================================================================
-- CORE TABLES (gateway scope)
-- ============================================================================

CREATE TABLE IF NOT EXISTS operators (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('discord', 'api', 'cli')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied')),
    access_level TEXT NOT NULL DEFAULT 'standard' CHECK (access_level IN ('puppet_master', 'standard')),
    rate_limit_5m_max INTEGER DEFAULT 10,
    rate_limit_1h_max INTEGER DEFAULT 100,
    allow_workspace_escape INTEGER DEFAULT 0 CHECK (allow_workspace_escape IN (0, 1)),
    verbose INTEGER DEFAULT 0 CHECK (verbose IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    approved_at INTEGER,
    denied_at INTEGER,
    welcomed INTEGER DEFAULT 0 CHECK (welcomed IN (0, 1))
);

CREATE INDEX IF NOT EXISTS idx_operators_status ON operators(status);
CREATE INDEX IF NOT EXISTS idx_operators_platform ON operators(platform);
CREATE INDEX IF NOT EXISTS idx_operators_created_at ON operators(created_at);

CREATE TABLE IF NOT EXISTS operator_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operator_id TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('created', 'approved', 'denied', 'welcomed', 'removed')),
    event_data TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (operator_id) REFERENCES operators(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_operator_events_operator_id ON operator_events(operator_id);
CREATE INDEX IF NOT EXISTS idx_operator_events_created_at ON operator_events(created_at);

CREATE TABLE IF NOT EXISTS ghosts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    owner_operator_id TEXT NOT NULL,
    cwd TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (owner_operator_id) REFERENCES operators(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ghosts_owner_operator_id ON ghosts(owner_operator_id);
CREATE INDEX IF NOT EXISTS idx_ghosts_created_at ON ghosts(created_at);

CREATE TABLE IF NOT EXISTS interfaces (
    id TEXT PRIMARY KEY,
    operator_id TEXT NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('discord', 'api', 'cli')),
    external_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (operator_id) REFERENCES operators(id) ON DELETE CASCADE,
    UNIQUE (platform, external_id)
);

CREATE INDEX IF NOT EXISTS idx_interfaces_operator_id ON interfaces(operator_id);
CREATE INDEX IF NOT EXISTS idx_interfaces_created_at ON interfaces(created_at);

-- ============================================================================
-- GHOST-SCOPED TABLES (partitioned by ghost_id)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    ghost_id TEXT NOT NULL,
    operator_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_active INTEGER DEFAULT 1 CHECK (is_active IN (0, 1)),
    compaction_summary TEXT,
    compaction_cursor_id TEXT,
    FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
    FOREIGN KEY (operator_id) REFERENCES operators(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_ghost_id ON sessions(ghost_id);
CREATE INDEX IF NOT EXISTS idx_sessions_operator_id ON sessions(operator_id);
CREATE INDEX IF NOT EXISTS idx_sessions_ghost_operator ON sessions(ghost_id, operator_id);
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at);
CREATE INDEX IF NOT EXISTS idx_sessions_is_active ON sessions(is_active);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    ghost_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('operator', 'ghost')),
    content TEXT NOT NULL,
    model TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_ghost_id ON messages(ghost_id);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_messages_ghost_session ON messages(ghost_id, session_id);
CREATE INDEX IF NOT EXISTS idx_messages_created_at ON messages(created_at);

CREATE TABLE IF NOT EXISTS job_logs (
    id TEXT PRIMARY KEY,
    ghost_id TEXT NOT NULL,
    job_kind TEXT NOT NULL CHECK (job_kind IN ('heartbeat', 'reflection')),
    session_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    status TEXT,
    transcript TEXT NOT NULL DEFAULT '[]',
    FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_job_logs_ghost_id ON job_logs(ghost_id);
CREATE INDEX IF NOT EXISTS idx_job_logs_session_id ON job_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_job_logs_session_kind ON job_logs(session_id, job_kind, started_at DESC);

CREATE TABLE IF NOT EXISTS usage_log (
    id TEXT PRIMARY KEY,
    ghost_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    message_id TEXT,
    model TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_creation_tokens INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_usage_log_ghost_id ON usage_log(ghost_id);
CREATE INDEX IF NOT EXISTS idx_usage_log_session_id ON usage_log(session_id);
CREATE INDEX IF NOT EXISTS idx_usage_log_created_at ON usage_log(created_at);

CREATE TABLE IF NOT EXISTS prompt_cache (
    id TEXT PRIMARY KEY,
    ghost_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    system_blocks_json TEXT NOT NULL,
    context_hash TEXT NOT NULL,
    cached_at INTEGER NOT NULL,
    FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_prompt_cache_ghost_id ON prompt_cache(ghost_id);
CREATE INDEX IF NOT EXISTS idx_prompt_cache_session_id ON prompt_cache(session_id);
CREATE INDEX IF NOT EXISTS idx_prompt_cache_ghost_hash ON prompt_cache(ghost_id, context_hash);

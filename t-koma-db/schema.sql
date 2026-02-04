-- T-KOMA (ティーコマ) database schema
-- Stores operators and ghost registry

CREATE TABLE IF NOT EXISTS operators (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('discord', 'api', 'cli')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied')),
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

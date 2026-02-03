-- Initial schema for t-koma database
-- Users table for managing approved/pending/denied users

CREATE TABLE IF NOT EXISTS users (
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

CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);
CREATE INDEX IF NOT EXISTS idx_users_platform ON users(platform);
CREATE INDEX IF NOT EXISTS idx_users_created_at ON users(created_at);

-- Events table for audit trail
CREATE TABLE IF NOT EXISTS user_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('created', 'approved', 'denied', 'welcomed', 'removed')),
    event_data TEXT,  -- JSON blob for flexible metadata
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_events_user_id ON user_events(user_id);
CREATE INDEX IF NOT EXISTS idx_events_created_at ON user_events(created_at);

-- Metadata table for migration tracking
CREATE TABLE IF NOT EXISTS app_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Insert initial schema version
INSERT OR REPLACE INTO app_metadata (key, value)
VALUES ('schema_version', '1.0.0');

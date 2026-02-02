# SQLite-Vec User Management System

## Overview

Replace the TOML-based user management with a proper SQLite database using
sqlite-vec extension. The database will be stored in the app's data directory.

## Goals

1. Create a new `t-koma-db` crate for database operations using sqlite-vec
2. Implement user management: approval, denial, pending status
3. Integrate user status checks in gateway before processing any message
4. Add CLI admin feature to approve/deny users

## Architecture

### New Crate: `t-koma-db`

Location: `t-koma-db/` (new workspace member)

**Dependencies:**

- `sqlx` with sqlite, runtime-tokio, macros features
- `sqlite-vec` for vector extension support
- `rusqlite` for extension loading
- `thiserror` for error handling
- `tokio` for async runtime
- `serde` for serialization
- `chrono` for timestamps
- `zerocopy` for vector operations
- `dirs` for data directory path

**Structure:**

```
t-koma-db/
├── Cargo.toml
├── migrations/
│   └── 001_initial_schema.sql
└── src/
    ├── lib.rs
    ├── db.rs (connection pool, sqlite-vec init)
    ├── users.rs (user management operations)
    └── error.rs (error types)
```

### Database Schema

**Users Table:**

```sql
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,  -- 'discord', 'api', 'cli'
    status TEXT NOT NULL,    -- 'pending', 'approved', 'denied'
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    approved_at INTEGER,
    denied_at INTEGER,
    welcomed INTEGER DEFAULT 0  -- boolean for discord users
);

CREATE INDEX idx_users_status ON users(status);
CREATE INDEX idx_users_platform ON users(platform);
CREATE INDEX idx_users_created_at ON users(created_at);
```

**Events Table (for audit trail):**

```sql
CREATE TABLE IF NOT EXISTS user_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    event_type TEXT NOT NULL,  -- 'created', 'approved', 'denied', 'welcomed'
    event_data TEXT,           -- JSON blob
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_events_user_id ON user_events(user_id);
CREATE INDEX idx_events_created_at ON user_events(created_at);
```

### API Design

**DbPool:**

```rust
pub struct DbPool {
    pool: SqlitePool,
}

impl DbPool {
    /// Initialize database, run migrations
    pub async fn new() -> Result<Self, DbError>;

    /// Get the inner pool
    pub fn pool(&self) -> &SqlitePool;
}
```

**User Management:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub denied_at: Option<DateTime<Utc>>,
    pub welcomed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Platform {
    Discord,
    Api,
    Cli,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserStatus {
    Pending,
    Approved,
    Denied,
}

pub struct UserRepository;

impl UserRepository {
    /// Create or get existing user (sets to pending if new)
    pub async fn get_or_create(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        platform: Platform,
    ) -> Result<User, DbError>;

    /// Get user by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<User>, DbError>;

    /// Check if user is approved
    pub async fn is_approved(pool: &SqlitePool, id: &str) -> Result<bool, DbError>;

    /// Approve a user
    pub async fn approve(pool: &SqlitePool, id: &str) -> Result<User, DbError>;

    /// Deny a user
    pub async fn deny(pool: &SqlitePool, id: &str) -> Result<User, DbError>;

    /// Mark user as welcomed (Discord only)
    pub async fn mark_welcomed(pool: &SqlitePool, id: &str) -> Result<(), DbError>;

    /// List users by status
    pub async fn list_by_status(
        pool: &SqlitePool,
        status: UserStatus,
        platform: Option<Platform>,
    ) -> Result<Vec<User>, DbError>;

    /// Remove user (completely delete)
    pub async fn remove(pool: &SqlitePool, id: &str) -> Result<(), DbError>;

    /// Auto-prune old pending users (older than 1 hour)
    pub async fn prune_pending(pool: &SqlitePool) -> Result<usize, DbError>;
}
```

## Integration Points

### Gateway Changes

1. **AppState** (`t-koma-gateway/src/state.rs`):
   - Replace `config: Mutex<PersistentConfig>` with `db: DbPool`
   - Replace `pending: Mutex<PendingUsers>` with database calls

2. **Server** (`t-koma-gateway/src/server.rs`):
   - Add user status check middleware or at start of each handler
   - Check `UserRepository::is_approved()` before processing chat messages
   - Extract user ID from WebSocket connection (for now, use a default or
     connection-based ID)

3. **Discord** (`t-koma-gateway/src/discord.rs`):
   - Replace `PersistentConfig`/`PendingUsers` calls with database operations
   - On message: `get_or_create()` user, check status
   - If pending: store and notify
   - If not approved: ignore or send "pending approval" message

### CLI Changes

1. **Admin Mode** (`t-koma-cli/src/admin.rs`):
   - Replace TOML file operations with database calls via `t-koma-db`
   - Add `approve_user()` and `deny_user()` commands
   - List pending/approved/denied users from database

## Migration Strategy

1. Keep TOML files as fallback during transition
2. On first DB startup, migrate existing approved/pending users to database
3. Mark migration complete in database metadata table

## Error Handling

All database operations return `Result<T, DbError>` where:

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("User not found: {0}")]
    UserNotFound(String),
    #[error("Invalid status transition")]
    InvalidTransition,
    #[error("Config directory not found")]
    NoConfigDir,
    #[error("Migration error: {0}")]
    Migration(String),
}
```

## Testing

1. Unit tests for all repository methods
2. Integration tests with in-memory database
3. Migration tests from TOML to SQLite

## Security Considerations

1. Database stored in user's data directory (platform-appropriate)
2. File permissions set to 0o600 on Unix
3. No sensitive data in error messages
4. Auto-prune pending users to prevent bloat

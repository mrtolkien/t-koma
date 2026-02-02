# SurrealDB Authentication System Specification

## Problem Statement

Current implementation has critical flaws:

1. **Gateway doesn't see CLI approvals** - Separate file reads, no
   synchronization
2. **Config files for dynamic data** - Wrong tool for the job, requires restarts
3. **No real-time updates** - Gateway caches config in memory, never refreshes

## Solution: SurrealDB Embedded

Use SurrealDB in embedded mode with file persistence for:

- User identity storage (pending/approved/denied)
- Real-time synchronization between gateway and CLI
- Proper querying capabilities
- ACID compliance

## Architecture

```
┌─────────────────┐         ┌──────────────────┐
│  t-koma-gateway │◄───────►│ SurrealDB        │
│                 │  IPC    │ (embedded)       │
└─────────────────┘         │ - users table    │
                            │ - audit_log      │
┌─────────────────┐         └──────────────────┘
│  t-koma-cli     │◄──────────────┘
│  (admin mode)   │
└─────────────────┘
```

Both processes connect to the same embedded DB file.

## Database Schema

### Namespace/Database

- **Namespace**: `t-koma`
- **Database**: `auth`

### Tables

#### `users`

```sql
DEFINE TABLE users SCHEMAFULL;
DEFINE FIELD id ON users TYPE record;
DEFINE FIELD user_id ON users TYPE string;
DEFINE FIELD name ON users TYPE string;
DEFINE FIELD service ON users TYPE string;  -- "discord", "api"
DEFINE FIELD status ON users TYPE string;   -- "pending", "approved", "denied"
DEFINE FIELD created_at ON users TYPE datetime;
DEFINE FIELD updated_at ON users TYPE datetime;
DEFINE FIELD welcomed ON users TYPE bool DEFAULT false;

DEFINE INDEX user_service_idx ON users COLUMNS user_id, service UNIQUE;
```

#### `audit_log`

```sql
DEFINE TABLE audit_log SCHEMAFULL;
DEFINE FIELD id ON audit_log TYPE record;
DEFINE FIELD action ON audit_log TYPE string;  -- "message", "approval", "denial"
DEFINE FIELD user_id ON audit_log TYPE string;
DEFINE FIELD service ON audit_log TYPE string;
DEFINE FIELD details ON audit_log TYPE object;
DEFINE FIELD timestamp ON audit_log TYPE datetime DEFAULT time::now();
```

## Authentication Flow

### 1. First Contact (Discord)

```
User messages bot
    │
    ▼
Gateway checks users table:
    SELECT * FROM users WHERE user_id = $id AND service = "discord"
    │
    ├── User exists & approved ──► Process message
    │
    ├── User exists & pending ──► "Still pending approval"
    │
    ├── User exists & denied ──► "Access denied"
    │
    └── No user found ──► INSERT INTO users (pending)
                          "Your request is pending approval"
```

### 2. Admin Approval (CLI)

```
Admin opens CLI option 3
    │
    ▼
Query pending users:
    SELECT * FROM users WHERE status = "pending" AND service = "discord"
    │
    ▼
Admin selects user to approve
    │
    ▼
UPDATE users SET status = "approved", updated_at = time::now()
WHERE user_id = $id AND service = "discord"
    │
    ▼
Gateway sees change immediately (same DB)
```

### 3. Post-Approval First Message

```
Approved user messages
    │
    ▼
Gateway queries user
    │
    ▼
IF welcomed = false:
    UPDATE users SET welcomed = true
    Prepend "Hello! You now have access to t-koma." to response
```

## Implementation Plan

### Phase 1: SurrealDB Integration

1. Add dependency to workspace:

```toml
surrealdb = { version = "2", features = ["kv-rocksdb"] }
```

2. Create `t-koma-core/src/db.rs`:

- `Db` struct wrapping SurrealDB client
- Connection management (embedded + file persistence)
- Schema initialization

3. Database location:

- **Linux**: `~/.local/share/t-koma/db/`
- **macOS**: `~/Library/Application Support/t-koma/db/`
- **Windows**: `%APPDATA%\t-koma\db\`

### Phase 2: User Repository

Create `t-koma-core/src/user_repo.rs`:

```rust
pub struct UserRepo {
    db: Arc<Surreal<Db>>,
}

impl UserRepo {
    /// Get user by ID and service
    pub async fn get(&self, user_id: &str, service: &str) -> Result<Option<User>>;

    /// Create pending user (or return existing)
    pub async fn get_or_create_pending(&self, user_id: &str, name: &str, service: &str) -> Result<User>;

    /// Approve user
    pub async fn approve(&self, user_id: &str, service: &str) -> Result<Option<User>>;

    /// Deny user
    pub async fn deny(&self, user_id: &str, service: &str) -> Result<Option<User>>;

    /// Mark as welcomed
    pub async fn mark_welcomed(&self, user_id: &str, service: &str) -> Result<()>;

    /// List pending users
    pub async fn list_pending(&self, service: &str) -> Result<Vec<User>>;

    /// List approved users
    pub async fn list_approved(&self, service: &str) -> Result<Vec<User>>;

    /// Auto-prune pending users older than 1 hour
    pub async fn prune_pending(&self) -> Result<u64>;
}
```

### Phase 3: Update Gateway

1. Replace `Mutex<PersistentConfig>` and `Mutex<PendingUsers>` with `UserRepo`
2. Update Discord handler:
   - Query DB on every message
   - Handle approval status transitions
   - Mark welcomed on first approved message

### Phase 4: Update CLI Admin Mode

1. Replace file-based operations with `UserRepo`
2. Real-time pending list from DB
3. Approve/deny updates immediately visible

### Phase 5: Cleanup

1. Remove old `persistent_config.rs` and `pending_users.rs`
2. Remove TOML dependencies
3. Update tests

## Code Examples

### DB Initialization

```rust
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

pub async fn init_db() -> Result<Surreal<Db>> {
    let path = db_path()?; // XDG data dir
    let db = Surreal::new::<RocksDb>(path).await?;

    db.use_ns("t-koma").use_db("auth").await?;

    // Initialize schema
    db.query(SCHEMA_SQL).await?;

    Ok(db)
}
```

### Check User Status

```rust
pub async fn check_user(&self, user_id: &str, service: &str) -> Result<UserStatus> {
    let mut result = self.db
        .query("SELECT * FROM users WHERE user_id = $id AND service = $service")
        .bind(("id", user_id))
        .bind(("service", service))
        .await?;

    let user: Option<User> = result.take(0)?;

    match user {
        Some(u) if u.status == "approved" => Ok(UserStatus::Approved(u)),
        Some(u) if u.status == "pending" => Ok(UserStatus::Pending),
        Some(u) if u.status == "denied" => Ok(UserStatus::Denied),
        None => Ok(UserStatus::NotFound),
        _ => Ok(UserStatus::Unknown),
    }
}
```

### Approve User

```rust
pub async fn approve(&self, user_id: &str, service: &str) -> Result<Option<User>> {
    let mut result = self.db
        .query(r#"
            UPDATE users
            SET status = "approved",
                updated_at = time::now()
            WHERE user_id = $id AND service = $service
            RETURN AFTER
        "#)
        .bind(("id", user_id))
        .bind(("service", service))
        .await?;

    let user: Option<User> = result.take(0)?;
    Ok(user)
}
```

## Dependencies

```toml
# t-koma-core
surrealdb = { version = "2", features = ["kv-rocksdb"] }
# Remove: toml, dirs (keep for path only), hex, rand for secrets

# Keep in workspace
serde = { workspace = true }  # For User struct
chrono = { workspace = true }
```

## Migration Path

1. On first DB startup with no users:
   - Check if old `config.toml` exists
   - If yes, migrate approved users to DB
   - Mark migration complete

2. Auto-pruning:
   - Run on gateway startup
   - Delete pending users WHERE created_at < time::now() - 1h

## Security Considerations

1. **DB file permissions**: `chmod 600` on Unix
2. **No remote access**: Embedded only, no network interface
3. **Audit logging**: All approvals/denials logged to `audit_log` table
4. **Backup**: Document location for user backups

## Error Handling

- DB connection failures are fatal (gateway won't start)
- Query errors are logged and return appropriate errors
- Transaction rollback on failures

## Testing

- Unit tests with temporary DB (tempdir)
- Integration tests with real embedded DB
- Test concurrent access (CLI + Gateway)

# SurrealDB with Rust - Comprehensive Knowledge Base

## Overview

SurrealDB is a multi-model database that can run:
- **In-memory** (`kv-mem`) - ephemeral, fast, good for testing
- **Embedded with RocksDB** (`kv-rocksdb`) - file-persisted, single-process
- **Remote via WebSocket** (`ws://` or `wss://`) - client-server

For t-koma: **Embedded RocksDB** is correct choice (single process, file-backed, no external server).

## Dependencies

```toml
[dependencies]
surrealdb = { version = "2", features = ["kv-rocksdb"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Core Architecture

### Connection Types

```rust
// 1. In-memory (ephemeral)
use surrealdb::engine::local::Mem;
let db = Surreal::new::<Mem>(()).await?;

// 2. Embedded with RocksDB (file-persisted)
use surrealdb::engine::local::RocksDb;
let db = Surreal::new::<RocksDb>("path/to/db/folder").await?;

// 3. Remote WebSocket
use surrealdb::engine::remote::ws::Ws;
let db = Surreal::new::<Ws>("127.0.0.1:8000").await?;
```

### Static Singleton Pattern

For sharing across app components:

```rust
use std::sync::LazyLock;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

static DB: LazyLock<Surreal<Db>> = LazyLock::new(Surreal::init);

async fn init() -> Result<()> {
    // Connect
    DB.connect::<RocksDb>("~/.local/share/t-koma/db").await?;
    // Select namespace/database
    DB.use_ns("t-koma").use_db("auth").await?;
    Ok(())
}
```

**Important**: With `LazyLock`, call `DB.connect()` once at startup.

## Namespace and Database

```rust
// Select namespace (like a cluster)
// Select database (like a logical DB)
db.use_ns("t-koma").use_db("auth").await?;
```

## Schema Design

### Table Types

**SCHEMALESS** (default):
```sql
DEFINE TABLE users SCHEMALESS;
-- Fields are optional, types not enforced
```

**SCHEMAFULL**:
```sql
DEFINE TABLE users SCHEMAFULL;
DEFINE FIELD name ON users TYPE string;
DEFINE FIELD age ON users TYPE int;
-- Extra fields rejected, types enforced
```

### Field Types

```sql
-- Basic types
TYPE string
TYPE int
TYPE float
TYPE bool
TYPE datetime
TYPE uuid

-- Complex types
TYPE option<string>          -- nullable
TYPE array<string>           -- array of strings
TYPE record<other_table>     -- foreign key
TYPE object                  -- nested object
```

### Constraints

```sql
-- Required field
DEFINE FIELD email ON users TYPE string ASSERT $value != NONE;

-- Unique constraint
DEFINE INDEX email_unique ON users FIELDS email UNIQUE;

-- Value in set
DEFINE FIELD status ON users TYPE string 
    ASSERT $value IN ["pending", "approved", "denied"];

-- Default value
DEFINE FIELD created_at ON users TYPE datetime DEFAULT time::now();

-- Computed/readonly
DEFINE FIELD updated_at ON users TYPE datetime VALUE time::now();
```

## CRUD Operations

### Create

```rust
#[derive(Serialize)]
struct User {
    user_id: String,
    name: String,
    status: String,
}

// With random ID
let created: Vec<User> = db
    .create("users")
    .content(User { user_id: "123".into(), name: "Alice".into(), status: "pending".into() })
    .await?;

// With specific ID (record ID)
let created: Option<User> = db
    .create(("users", "alice-123"))  // table + ID
    .content(User { ... })
    .await?;
```

### Select

```rust
// All records
let users: Vec<User> = db.select("users").await?;

// Specific record
let user: Option<User> = db.select(("users", "alice-123")).await?;

// Custom query
let mut result = db
    .query("SELECT * FROM users WHERE status = $status")
    .bind(("status", "pending"))
    .await?;

let pending: Vec<User> = result.take(0)?;  // take first result set
```

### Update

```rust
// Full replace
let updated: Option<User> = db
    .update(("users", "alice-123"))
    .content(User { ... })
    .await?;

// Merge (partial update)
#[derive(Serialize)]
struct StatusUpdate {
    status: String,
}

let updated: Option<User> = db
    .update(("users", "alice-123"))
    .merge(StatusUpdate { status: "approved".into() })
    .await?;

// Query update
let mut result = db
    .query(r#"
        UPDATE users 
        SET status = "approved", updated_at = time::now()
        WHERE user_id = $id
        RETURN AFTER
    "#)
    .bind(("id", "123"))
    .await?;

let user: Option<User> = result.take(0)?;
```

### Delete

```rust
// Specific record
let deleted: Option<User> = db.delete(("users", "alice-123")).await?;

// Delete all (careful!)
let deleted: Vec<User> = db.delete("users").await?;
```

## Query Method (Most Flexible)

```rust
let mut response = db
    .query(r#"
        BEGIN TRANSACTION;
        
        -- Get user
        SELECT * FROM users WHERE user_id = $user_id;
        
        -- Count pending
        SELECT count() FROM users WHERE status = "pending" GROUP ALL;
        
        -- Update
        UPDATE users SET status = "approved" WHERE user_id = $user_id;
        
        COMMIT TRANSACTION;
    "#)
    .bind(("user_id", "123"))
    .await?;

// Extract results by index
let user: Option<User> = response.take(0)?;
let count: Option<i64> = response.take(1)?;
```

## Rust Type Mapping

| SurrealDB | Rust |
|-----------|------|
| `string` | `String` or `Cow<'static, str>` |
| `int` | `i64` |
| `float` | `f64` |
| `bool` | `bool` |
| `datetime` | `chrono::DateTime<Utc>` |
| `uuid` | `uuid::Uuid` |
| `record<...>` | Custom struct or `RecordId` |
| `array<T>` | `Vec<T>` |
| `object` | Custom struct |

## Record IDs

SurrealDB uses `table:id` format:

```rust
use surrealdb::RecordId;

// Create with specific ID
let user = db.create(("users", "123")).content(...).await?;

// RecordId type
#[derive(Deserialize)]
struct User {
    id: RecordId,  // e.g., users:123
    name: String,
}

// Extract ID parts
let id: RecordId = ...;
let table = id.tb();  // "users"
let key = id.id().to_string();  // "123"
```

## Transactions

```rust
db.query(r#"
    BEGIN TRANSACTION;
    
    CREATE audit_log:1 SET action = "approve", user_id = "123";
    UPDATE users:123 SET status = "approved";
    
    COMMIT TRANSACTION;
    -- Or: CANCEL TRANSACTION;
"#).await?;
```

## Migrations

SurrealDB has **no built-in migration system**. Options:

### Option 1: Simple Version Tracking (Recommended for embedded)

```rust
const SCHEMA_V1: &str = r#"
    DEFINE TABLE users SCHEMAFULL;
    DEFINE FIELD user_id ON users TYPE string;
    DEFINE FIELD name ON users TYPE string;
"#;

const SCHEMA_V2: &str = r#"
    DEFINE FIELD welcomed ON users TYPE bool DEFAULT false;
"#;

async fn run_migrations(db: &Surreal<Db>) -> Result<()> {
    // Check current version
    let version: Option<i64> = db
        .query("SELECT value FROM config:schema_version")
        .await?
        .take(0)?;
    
    let version = version.unwrap_or(0);
    
    if version < 1 {
        db.query(SCHEMA_V1).await?;
        db.query("CREATE config:schema_version SET value = 1").await?;
    }
    
    if version < 2 {
        db.query(SCHEMA_V2).await?;
        db.query("UPDATE config:schema_version SET value = 2").await?;
    }
    
    Ok(())
}
```

### Option 2: External Crate (`surrealdb-migrations`)

```toml
surrealdb-migrations = "0.9"
```

Uses file-based migrations in `/schemas`, `/migrations` folders.

**Warning**: The crate warns "not production-ready".

## Best Practices

### 1. Use `IF NOT EXISTS` for Idempotent Schema

```sql
DEFINE TABLE IF NOT EXISTS users SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name ON users TYPE string;
```

### 2. Embedded DB: Single Process Only

RocksDB backend locks the database folder. Only one process can access it.

### 3. File Permissions on Unix

```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&db_path)?.permissions();
    perms.set_mode(0o700);  // Only owner
    std::fs::set_permissions(&db_path, perms)?;
}
```

### 4. Use `query()` for Complex Operations

The typed methods (`create()`, `update()`) are ergonomic but limited.
Use `.query()` for:
- Multiple statements
- Transactions
- Complex WHERE clauses
- Aggregations

### 5. Error Handling

```rust
use surrealdb::Error;

match result {
    Err(Error::Db(e)) => println!("Database error: {}", e),
    Err(Error::Api(e)) => println!("API error: {}", e),
    _ => {}
}
```

### 6. Live Queries (Optional)

Subscribe to changes:

```rust
let mut stream = db
    .query("LIVE SELECT * FROM users WHERE status = 'pending'")
    .await?;

while let Some(notification) = stream.next().await {
    println!("{:?}", notification);
}
```

## Common Pitfalls

1. **Forget `use_ns().use_db()`** - Must be called after connect
2. **Wrong result extraction** - Use `.take(0)?` for first result set
3. **Path as string** - RocksDB path is `&str`, not `PathBuf`
4. **Feature flags** - Must enable `kv-rocksdb` for embedded
5. **LazyLock not connected** - Remember to call `.connect()` before using

## t-koma Specific Considerations

### Data Location

```rust
use dirs::data_dir;

fn db_path() -> String {
    data_dir()
        .expect("No data dir")
        .join("t-koma")
        .join("db")
        .to_str()
        .unwrap()
        .to_string()
}
```

### Shared Access (Gateway + CLI)

**Problem**: RocksDB is single-process.
**Solution**: Gateway owns DB, CLI queries via HTTP/WebSocket API.

Alternative: Use SurrealKV if available (newer, designed for concurrent access).

### Recommended Schema for Auth

```sql
-- Users table
DEFINE TABLE IF NOT EXISTS users SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS user_id ON users TYPE string;
DEFINE FIELD IF NOT EXISTS name ON users TYPE string;
DEFINE FIELD IF NOT EXISTS service ON users TYPE string 
    ASSERT $value IN ["discord", "api"];
DEFINE FIELD IF NOT EXISTS status ON users TYPE string
    ASSERT $value IN ["pending", "approved", "denied"];
DEFINE FIELD IF NOT EXISTS created_at ON users TYPE datetime DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS updated_at ON users TYPE datetime VALUE time::now();
DEFINE FIELD IF NOT EXISTS welcomed ON users TYPE bool DEFAULT false;

-- Unique constraint
DEFINE INDEX IF NOT EXISTS user_service_idx ON users FIELDS user_id, service UNIQUE;

-- For fast pending lookups
DEFINE INDEX IF NOT EXISTS status_idx ON users FIELDS status;
```

## Resources

- Docs: https://surrealdb.com/docs/sdk/rust
- API: https://docs.rs/surrealdb/2.0.0/surrealdb/
- Migrations: https://github.com/Odonno/surrealdb-migrations
- SurrealQL: https://surrealdb.com/docs/surrealql

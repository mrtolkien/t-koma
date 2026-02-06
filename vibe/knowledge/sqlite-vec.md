# sqlite-vec + sqlx Guide for AI Agents

A comprehensive guide for building RAG-powered AI agents with sqlite-vec and
sqlx in Rust.

## Table of Contents

- [Installation & Setup](#installation--setup)
- [Best Practices](#best-practices)
- [Schema Management](#schema-management)
- [Testing](#testing)
- [Common Patterns](#common-patterns)

---

## Installation & Setup

### 1. Dependencies

Add to your `Cargo.toml`:

```toml
[dependencies]
# Core dependencies
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros"] }
sqlite-vec = "0.1"
tokio = { version = "1", features = ["full"] }
zerocopy = "0.8"  # For zero-copy vector conversion

# Optional but recommended
rusqlite = "0.32"  # Only needed for extension loading
anyhow = "1.0"     # Error handling
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dev-dependencies]
# For testing
tempfile = "3.0"   # Temporary test databases
```

### 2. Basic Setup

Create a module for database initialization:

```rust
// src/db.rs
use sqlite_vec::sqlite3_vec_init;
use rusqlite::ffi::sqlite3_auto_extension;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use anyhow::Result;

/// Initialize sqlite-vec extension globally
/// Must be called once at application startup, before any database connections
pub fn init_sqlite_vec() -> Result<()> {
    unsafe {
        sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite3_vec_init as *const ())
        ));
    }
    Ok(())
}

/// Create a connection pool with optimal settings for AI agents
pub async fn create_pool(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)  // Adjust based on your concurrency needs
        .connect(database_url)
        .await?;

    // Enable WAL mode for better concurrent read performance
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&pool)
        .await?;

    // Improve performance for write-heavy workloads
    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(&pool)
        .await?;

    Ok(pool)
}
```

### 3. Application Initialization

In your `main.rs` or application entry point:

```rust
// src/main.rs
mod db;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize sqlite-vec extension FIRST
    db::init_sqlite_vec()?;

    // Create database pool
    let pool = db::create_pool("sqlite:./data/agent.db").await?;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    // Your application code here

    Ok(())
}
```

### 4. Environment Configuration

Create a `.env` file for development:

```env
DATABASE_URL=sqlite:./data/agent.db
RUST_LOG=info
```

---

## Best Practices

### Vector Storage & Conversion

Use zerocopy for efficient vector operations:

```rust
use zerocopy::AsBytes;

// Store vectors
let embedding: Vec<f32> = vec![0.1, 0.2, 0.3]; // Your embedding
sqlx::query!(
    "INSERT INTO embeddings(content_id, vector) VALUES (?, ?)",
    content_id,
    embedding.as_bytes()
)
.execute(&pool)
.await?;

// Query vectors
let query_vec: Vec<f32> = vec![0.15, 0.25, 0.35];
let results = sqlx::query!(
    r#"
    SELECT content_id, distance
    FROM vec_content
    WHERE vector MATCH ?
    ORDER BY distance
    LIMIT 10
    "#,
    query_vec.as_bytes()
)
.fetch_all(&pool)
.await?;
```

### Schema Design Principles

1. **Separate metadata from vectors**: Use two tables - one for metadata, one
   for vector search
2. **Use explicit INTEGER PRIMARY KEY**: Don't rely on implicit rowid
3. **Index your filter columns**: Any column used in WHERE clauses should be
   indexed
4. **Keep vectors in BLOB columns**: Efficient storage and retrieval

### Transaction Management

For atomic operations involving multiple tables:

```rust
let mut tx = pool.begin().await?;

// Insert metadata
let content_id = sqlx::query!(
    "INSERT INTO content(text, doc_type, created_at) VALUES (?, ?, ?) RETURNING id",
    text,
    doc_type,
    timestamp
)
.fetch_one(&mut *tx)
.await?
.id;

// Insert vector
sqlx::query!(
    "INSERT INTO vec_content(content_id, vector) VALUES (?, ?)",
    content_id,
    embedding.as_bytes()
)
.execute(&mut *tx)
.await?;

tx.commit().await?;
```

### Performance Optimization

```rust
// Enable mmap for large databases (read-heavy workloads)
sqlx::query("PRAGMA mmap_size = 268435456")  // 256MB
    .execute(&pool)
    .await?;

// Increase cache size for better performance
sqlx::query("PRAGMA cache_size = -64000")  // 64MB (negative = kibibytes)
    .execute(&pool)
    .await?;

// Analyze query plans
let plan = sqlx::query("EXPLAIN QUERY PLAN SELECT ...")
    .fetch_all(&pool)
    .await?;
```

### Error Handling

Use type-safe queries with proper error handling:

```rust
use anyhow::{Context, Result};

pub async fn search_similar(
    pool: &SqlitePool,
    query: &[f32],
    doc_type: &str,
    limit: i64,
) -> Result<Vec<SearchResult>> {
    let results = sqlx::query_as!(
        SearchResult,
        r#"
        SELECT
            c.id,
            c.text,
            c.doc_type,
            v.distance
        FROM content c
        JOIN vec_content v ON c.id = v.content_id
        WHERE c.doc_type = ?
          AND v.vector MATCH ?
        ORDER BY v.distance
        LIMIT ?
        "#,
        doc_type,
        query.as_bytes(),
        limit
    )
    .fetch_all(pool)
    .await
    .context("Failed to execute vector search")?;

    Ok(results)
}
```

---

## Schema Management

### Migration Structure

Organize migrations in the `migrations/` directory:

```
migrations/
├── 20240101000000_initial_schema.sql
├── 20240102000000_add_doc_types.sql
└── 20240103000000_add_indexes.sql
```

### Initial Schema Migration

`migrations/20240101000000_initial_schema.sql`:

```sql
-- Content metadata table
CREATE TABLE IF NOT EXISTS content (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    doc_type TEXT NOT NULL,
    created_at INTEGER NOT NULL,  -- Unix timestamp
    metadata TEXT,  -- JSON blob for flexible metadata
    CHECK(length(text) > 0)
);

-- Vector table using vec0 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS vec_content USING vec0(
    content_id INTEGER PRIMARY KEY,
    vector FLOAT[768]  -- Adjust dimension based on your embedding model
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_content_doc_type ON content(doc_type);
CREATE INDEX IF NOT EXISTS idx_content_created_at ON content(created_at);
CREATE INDEX IF NOT EXISTS idx_content_type_date ON content(doc_type, created_at);
```

### Schema Evolution Example

`migrations/20240102000000_add_relationships.sql`:

```sql
-- Add relationship tracking for knowledge graph
CREATE TABLE IF NOT EXISTS content_relationships (
    source_id INTEGER NOT NULL,
    target_id INTEGER NOT NULL,
    relationship_type TEXT NOT NULL,
    strength REAL DEFAULT 1.0,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (source_id, target_id, relationship_type),
    FOREIGN KEY (source_id) REFERENCES content(id) ON DELETE CASCADE,
    FOREIGN KEY (target_id) REFERENCES content(id) ON DELETE CASCADE,
    CHECK(strength >= 0.0 AND strength <= 1.0)
);

CREATE INDEX IF NOT EXISTS idx_rel_source ON content_relationships(source_id);
CREATE INDEX IF NOT EXISTS idx_rel_target ON content_relationships(target_id);
CREATE INDEX IF NOT EXISTS idx_rel_type ON content_relationships(relationship_type);
```

### Running Migrations

#### Development

```bash
# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features sqlite

# Create a new migration
sqlx migrate add initial_schema

# Run all pending migrations
sqlx migrate run --database-url sqlite:./data/agent.db

# Revert last migration
sqlx migrate revert --database-url sqlite:./data/agent.db
```

#### Production (Embedded)

```rust
// Embed migrations in your binary
use sqlx::migrate::MigrateDatabase;

#[tokio::main]
async fn main() -> Result<()> {
    let db_url = "sqlite:./data/agent.db";

    // Create database if it doesn't exist
    if !sqlx::Sqlite::database_exists(db_url).await? {
        sqlx::Sqlite::create_database(db_url).await?;
    }

    let pool = create_pool(db_url).await?;

    // Run embedded migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    Ok(())
}
```

### Schema Versioning for Local Apps

For desktop/local applications, consider version metadata:

```sql
CREATE TABLE IF NOT EXISTS app_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO app_metadata(key, value)
VALUES ('schema_version', '1.0.0')
ON CONFLICT(key) DO UPDATE SET value = excluded.value;
```

---

## Testing

### Test Setup

Use in-memory databases for fast, isolated tests:

```rust
// tests/common/mod.rs
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use anyhow::Result;

pub async fn setup_test_db() -> Result<SqlitePool> {
    // In-memory database - isolated per test
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    Ok(pool)
}

// Helper to create test embeddings
pub fn create_test_embedding(size: usize, seed: f32) -> Vec<f32> {
    (0..size)
        .map(|i| (i as f32 * seed).sin())
        .collect()
}
```

### Using sqlx::test Macro

For more complex testing scenarios:

```rust
#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;
    use zerocopy::AsBytes;

    // sqlx::test automatically:
    // 1. Creates a fresh test database
    // 2. Runs migrations
    // 3. Provides isolated pool
    #[sqlx::test]
    async fn test_vector_search(pool: SqlitePool) -> sqlx::Result<()> {
        // Insert test data
        let embedding = vec![0.1f32, 0.2, 0.3, 0.4];

        sqlx::query!(
            "INSERT INTO content(text, doc_type, created_at) VALUES (?, ?, ?)",
            "test content",
            "test",
            1234567890
        )
        .execute(&pool)
        .await?;

        sqlx::query!(
            "INSERT INTO vec_content(content_id, vector) VALUES (1, ?)",
            embedding.as_bytes()
        )
        .execute(&pool)
        .await?;

        // Test search
        let query_vec = vec![0.15f32, 0.25, 0.35, 0.45];
        let results = sqlx::query!(
            "SELECT content_id, distance FROM vec_content
             WHERE vector MATCH ?
             ORDER BY distance LIMIT 5",
            query_vec.as_bytes()
        )
        .fetch_all(&pool)
        .await?;

        assert_eq!(results.len(), 1);
        assert!(results[0].distance < 0.5);

        Ok(())
    }
}
```

### Testing with Fixtures

For tests requiring existing data:

```rust
// tests/fixtures/sample_data.sql
INSERT INTO content(id, text, doc_type, created_at) VALUES
    (1, 'Rust is a systems programming language', 'article', 1234567890),
    (2, 'Python is great for AI', 'article', 1234567891),
    (3, 'SQLite is embedded database', 'article', 1234567892);

-- Use actual embeddings or test vectors
INSERT INTO vec_content(content_id, vector) VALUES
    (1, X'3dcccccd3e4ccccd3e99999a'),  -- Binary format
    (2, X'3e19999a3e4ccccd3e800000'),
    (3, X'3e99999a3ecccccd3f000000');
```

```rust
#[sqlx::test(fixtures("sample_data"))]
async fn test_with_fixtures(pool: SqlitePool) -> sqlx::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content")
        .fetch_one(&pool)
        .await?;

    assert_eq!(count, 3);
    Ok(())
}
```

### Integration Testing

```rust
// tests/integration_test.rs
use tempfile::TempDir;

#[tokio::test]
async fn test_full_rag_workflow() -> anyhow::Result<()> {
    // Create temporary directory for test database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    // Initialize
    crate::db::init_sqlite_vec()?;
    let pool = crate::db::create_pool(&db_url).await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    // Test your full workflow
    // 1. Insert documents
    // 2. Generate embeddings
    // 3. Search
    // 4. Verify results

    Ok(())
}
```

### Benchmarking

```rust
#[cfg(test)]
mod benches {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn bench_vector_search() -> anyhow::Result<()> {
        let pool = setup_test_db().await?;

        // Insert 10,000 test vectors
        for i in 0..10_000 {
            let embedding: Vec<f32> = (0..768)
                .map(|j| ((i * j) as f32).sin())
                .collect();

            sqlx::query!(
                "INSERT INTO vec_content(content_id, vector) VALUES (?, ?)",
                i,
                embedding.as_bytes()
            )
            .execute(&pool)
            .await?;
        }

        // Benchmark search
        let query_vec: Vec<f32> = (0..768).map(|i| (i as f32).cos()).collect();
        let start = Instant::now();

        let _results = sqlx::query!(
            "SELECT content_id, distance FROM vec_content
             WHERE vector MATCH ?
             ORDER BY distance LIMIT 10",
            query_vec.as_bytes()
        )
        .fetch_all(&pool)
        .await?;

        let duration = start.elapsed();
        println!("Search took: {:?}", duration);

        // Assert reasonable performance
        assert!(duration.as_millis() < 100, "Search too slow");

        Ok(())
    }
}
```

### Testing Best Practices

1. **Use in-memory databases** for unit tests (fast, isolated)
2. **Use temporary files** for integration tests (realistic I/O)
3. **Test with realistic vector dimensions** (768 for many models)
4. **Verify distance calculations** are within expected ranges
5. **Test concurrent access** if your app is multi-threaded
6. **Mock embedding generation** in tests (focus on database logic)

---

## Common Patterns

### RAG Search Pattern

```rust
pub async fn rag_search(
    pool: &SqlitePool,
    query: &str,
    embedding_fn: impl Fn(&str) -> Vec<f32>,
    doc_type: Option<&str>,
    limit: i64,
) -> Result<Vec<Document>> {
    // Generate query embedding
    let query_embedding = embedding_fn(query);

    // Build dynamic query based on filters
    let results = if let Some(dt) = doc_type {
        sqlx::query_as!(
            Document,
            r#"
            SELECT c.id, c.text, c.doc_type, c.created_at, v.distance
            FROM content c
            JOIN vec_content v ON c.id = v.content_id
            WHERE c.doc_type = ?
              AND v.vector MATCH ?
            ORDER BY v.distance
            LIMIT ?
            "#,
            dt,
            query_embedding.as_bytes(),
            limit
        )
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as!(
            Document,
            r#"
            SELECT c.id, c.text, c.doc_type, c.created_at, v.distance
            FROM content c
            JOIN vec_content v ON c.id = v.content_id
            WHERE v.vector MATCH ?
            ORDER BY v.distance
            LIMIT ?
            "#,
            query_embedding.as_bytes(),
            limit
        )
        .fetch_all(pool)
        .await?
    };

    Ok(results)
}
```

### Hybrid Search (Vector + Full-Text)

First, add FTS5 table in migrations:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS content_fts USING fts5(
    text,
    content=content,
    content_rowid=id
);

-- Triggers to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS content_ai AFTER INSERT ON content BEGIN
    INSERT INTO content_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER IF NOT EXISTS content_ad AFTER DELETE ON content BEGIN
    DELETE FROM content_fts WHERE rowid = old.id;
END;

CREATE TRIGGER IF NOT EXISTS content_au AFTER UPDATE ON content BEGIN
    UPDATE content_fts SET text = new.text WHERE rowid = new.id;
END;
```

Then implement hybrid search:

```rust
pub async fn hybrid_search(
    pool: &SqlitePool,
    query: &str,
    embedding: Vec<f32>,
    limit: i64,
) -> Result<Vec<Document>> {
    // Use Reciprocal Rank Fusion to combine vector and keyword results
    sqlx::query_as!(
        Document,
        r#"
        WITH vector_results AS (
            SELECT content_id, distance,
                   ROW_NUMBER() OVER (ORDER BY distance) as rank
            FROM vec_content
            WHERE vector MATCH ?
            LIMIT 20
        ),
        fts_results AS (
            SELECT rowid as content_id,
                   ROW_NUMBER() OVER (ORDER BY rank) as rank
            FROM content_fts
            WHERE content_fts MATCH ?
            LIMIT 20
        ),
        combined AS (
            SELECT content_id,
                   (1.0 / (60 + COALESCE(v.rank, 999))) +
                   (1.0 / (60 + COALESCE(f.rank, 999))) as score
            FROM (
                SELECT content_id FROM vector_results
                UNION
                SELECT content_id FROM fts_results
            ) all_results
            LEFT JOIN vector_results v USING (content_id)
            LEFT JOIN fts_results f USING (content_id)
        )
        SELECT c.id, c.text, c.doc_type, c.created_at, comb.score
        FROM combined comb
        JOIN content c ON c.id = comb.content_id
        ORDER BY comb.score DESC
        LIMIT ?
        "#,
        embedding.as_bytes(),
        query,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}
```

### Knowledge Graph Pattern

```rust
pub async fn get_related_content(
    pool: &SqlitePool,
    content_id: i64,
    relationship_type: Option<&str>,
    depth: i32,
) -> Result<Vec<RelatedContent>> {
    // Simple one-hop relationship query
    if depth == 1 {
        let results = if let Some(rel_type) = relationship_type {
            sqlx::query_as!(
                RelatedContent,
                r#"
                SELECT
                    c.id,
                    c.text,
                    c.doc_type,
                    r.relationship_type,
                    r.strength
                FROM content_relationships r
                JOIN content c ON c.id = r.target_id
                WHERE r.source_id = ? AND r.relationship_type = ?
                ORDER BY r.strength DESC
                "#,
                content_id,
                rel_type
            )
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as!(
                RelatedContent,
                r#"
                SELECT
                    c.id,
                    c.text,
                    c.doc_type,
                    r.relationship_type,
                    r.strength
                FROM content_relationships r
                JOIN content c ON c.id = r.target_id
                WHERE r.source_id = ?
                ORDER BY r.strength DESC
                "#,
                content_id
            )
            .fetch_all(pool)
            .await?
        };
        Ok(results)
    } else {
        // For multi-hop, use recursive CTE (SQLite 3.8.3+)
        // Implementation depends on your specific graph structure
        todo!("Implement recursive graph traversal")
    }
}
```

### Batch Insert Pattern

For efficient bulk inserts:

```rust
pub async fn batch_insert_with_embeddings(
    pool: &SqlitePool,
    documents: Vec<(String, String, Vec<f32>)>,  // (text, doc_type, embedding)
) -> Result<()> {
    let mut tx = pool.begin().await?;

    for (text, doc_type, embedding) in documents {
        // Insert content
        let id = sqlx::query!(
            "INSERT INTO content(text, doc_type, created_at) VALUES (?, ?, ?) RETURNING id",
            text,
            doc_type,
            chrono::Utc::now().timestamp()
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        // Insert vector
        sqlx::query!(
            "INSERT INTO vec_content(content_id, vector) VALUES (?, ?)",
            id,
            embedding.as_bytes()
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
```

---

## Additional Resources

- [sqlite-vec Documentation](https://alexgarcia.xyz/sqlite-vec/)
- [sqlx Documentation](https://docs.rs/sqlx/)
- [SQLite Documentation](https://sqlite.org/docs.html)
- [Zero to Production in Rust](https://www.zero2prod.com/) - Great resource on
  testing with sqlx

---

## Quick Reference

### Essential Commands

```bash
# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features sqlite

# Create migration
sqlx migrate add <name>

# Run migrations
sqlx migrate run

# Revert last migration
sqlx migrate revert

# Generate offline query data (for compile-time checks in CI)
cargo sqlx prepare
```

### Key Performance Settings

```sql
PRAGMA journal_mode = WAL;        -- Better concurrency
PRAGMA synchronous = NORMAL;      -- Faster writes (still safe)
PRAGMA cache_size = -64000;       -- 64MB cache
PRAGMA mmap_size = 268435456;     -- 256MB mmap (read-heavy)
PRAGMA temp_store = MEMORY;       -- Temp tables in memory
```

### Common Vector Operations

```sql
-- Distance metrics in sqlite-vec
-- Default: L2 (Euclidean) distance
-- Also supports: cosine similarity, L1 distance

-- Get k-nearest neighbors (simple, no JOINs)
SELECT rowid, distance
FROM vec_content
WHERE vector MATCH ?
  AND k = 10;

-- LIMIT works ONLY on SQLite 3.41+ and ONLY when querying vec0 directly
-- (no JOINs). Prefer `k = ?` for portability.
```

### IMPORTANT: vec0 KNN with JOINs

**`LIMIT` does NOT work when JOINing vec0 with other tables.** sqlite-vec
cannot see the LIMIT through the JOIN and will error with:
`"A LIMIT or 'k = ?' constraint is required on vec0 knn queries."`

Always use a CTE to isolate the KNN search, then JOIN in the outer query:

```sql
-- CORRECT: CTE isolates the KNN query
WITH knn AS (
  SELECT rowid, distance
  FROM chunk_vec
  WHERE embedding MATCH ?
    AND k = ?
)
SELECT c.id, knn.distance
FROM knn
JOIN chunks c ON c.id = knn.rowid
JOIN notes n ON n.id = c.note_id
WHERE n.scope = ? AND n.owner_ghost IS NULL
ORDER BY knn.distance ASC
LIMIT ?;

-- WRONG: will fail with "LIMIT or k = ? required"
SELECT c.id, v.distance
FROM chunk_vec v
JOIN chunks c ON c.id = v.rowid
WHERE v.embedding MATCH ?
ORDER BY v.distance ASC
LIMIT 10;
```

When filtering by scope/owner in the outer query, overfetch in the CTE
(e.g. `k = limit * 4`) to compensate for rows filtered out after the KNN.

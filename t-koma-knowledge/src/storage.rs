use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::Utc;
use libsqlite3_sys::{SQLITE_OK, sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
use sqlite_vec::sqlite3_vec_init;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::errors::{KnowledgeError, KnowledgeResult};

static SQLITE_VEC_INIT_RC: OnceLock<i32> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct KnowledgeStore {
    pool: SqlitePool,
}

impl KnowledgeStore {
    pub async fn open(db_path: &Path, embedding_dim: Option<usize>) -> KnowledgeResult<Self> {
        init_sqlite_vec_once()?;
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    sqlx::query("PRAGMA journal_mode = WAL")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA synchronous = NORMAL")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA cache_size = -64000")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;

        run_migrations(&pool).await?;
        ensure_vec_table(&pool, embedding_dim).await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn init_sqlite_vec_once() -> KnowledgeResult<()> {
    let rc = *SQLITE_VEC_INIT_RC.get_or_init(|| unsafe {
        type SqliteVecInitFn =
            unsafe extern "C" fn(*mut sqlite3, *mut *const i8, *const sqlite3_api_routines) -> i32;

        sqlite3_auto_extension(Some(std::mem::transmute::<*const (), SqliteVecInitFn>(
            sqlite3_vec_init as *const (),
        )))
    });

    if rc == SQLITE_OK {
        Ok(())
    } else {
        Err(KnowledgeError::SqliteVec(format!(
            "sqlite-vec init failed with code {rc}"
        )))
    }
}

async fn run_migrations(pool: &SqlitePool) -> KnowledgeResult<()> {
    sqlx::migrate!("./migrations/knowledge").run(pool).await?;
    Ok(())
}

async fn ensure_vec_table(pool: &SqlitePool, embedding_dim: Option<usize>) -> KnowledgeResult<()> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT value FROM meta WHERE key = 'embedding_dim' LIMIT 1")
            .fetch_optional(pool)
            .await?;

    let dim = if let Some((value,)) = existing {
        value.parse::<usize>().ok()
    } else {
        embedding_dim
    };

    if let Some(dimension) = dim {
        let table_exists: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'chunk_vec'",
        )
        .fetch_optional(pool)
        .await?;

        if table_exists.is_none() {
            let create_sql = format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vec USING vec0(embedding float[{}])",
                dimension
            );
            sqlx::query(&create_sql).execute(pool).await?;
        }

        sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES ('embedding_dim', ?)")
            .bind(dimension.to_string())
            .execute(pool)
            .await?;
    }

    Ok(())
}

pub async fn ensure_vec_table_dim(pool: &SqlitePool, dimension: usize) -> KnowledgeResult<()> {
    let table_exists: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'chunk_vec'",
    )
    .fetch_optional(pool)
    .await?;

    if table_exists.is_none() {
        let create_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vec USING vec0(embedding float[{}])",
            dimension
        );
        sqlx::query(&create_sql).execute(pool).await?;
    }

    sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES ('embedding_dim', ?)")
        .bind(dimension.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct NoteRecord {
    pub id: String,
    pub title: String,
    pub note_type: String,
    pub type_valid: bool,
    pub path: PathBuf,
    pub scope: String,
    pub owner_ghost: Option<String>,
    pub created_at: String,
    pub created_by_ghost: String,
    pub created_by_model: String,
    pub trust_score: i64,
    pub last_validated_at: Option<String>,
    pub last_validated_by_ghost: Option<String>,
    pub last_validated_by_model: Option<String>,
    pub version: Option<i64>,
    pub parent_id: Option<String>,
    pub comments_json: Option<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub note_id: String,
    pub chunk_index: i64,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub embedding_model: Option<String>,
    pub embedding_dim: Option<i64>,
}

pub async fn upsert_note(pool: &SqlitePool, record: &NoteRecord) -> KnowledgeResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"INSERT INTO notes (
            id, title, note_type, type_valid, path, scope, owner_ghost, created_at, created_by_ghost, created_by_model,
            trust_score, last_validated_at, last_validated_by_ghost, last_validated_by_model,
            version, parent_id, comments_json, content_hash, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            title=excluded.title,
            note_type=excluded.note_type,
            type_valid=excluded.type_valid,
            path=excluded.path,
            scope=excluded.scope,
            owner_ghost=excluded.owner_ghost,
            trust_score=excluded.trust_score,
            last_validated_at=excluded.last_validated_at,
            last_validated_by_ghost=excluded.last_validated_by_ghost,
            last_validated_by_model=excluded.last_validated_by_model,
            version=excluded.version,
            parent_id=excluded.parent_id,
            comments_json=excluded.comments_json,
            content_hash=excluded.content_hash,
            updated_at=excluded.updated_at"#,
    )
    .bind(&record.id)
    .bind(&record.title)
    .bind(&record.note_type)
    .bind(record.type_valid as i64)
    .bind(record.path.to_string_lossy().to_string())
    .bind(&record.scope)
    .bind(&record.owner_ghost)
    .bind(&record.created_at)
    .bind(&record.created_by_ghost)
    .bind(&record.created_by_model)
    .bind(record.trust_score)
    .bind(&record.last_validated_at)
    .bind(&record.last_validated_by_ghost)
    .bind(&record.last_validated_by_model)
    .bind(record.version)
    .bind(&record.parent_id)
    .bind(&record.comments_json)
    .bind(&record.content_hash)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn replace_tags(
    pool: &SqlitePool,
    note_id: &str,
    tags: &[String],
) -> KnowledgeResult<()> {
    sqlx::query("DELETE FROM note_tags WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;

    for tag in tags {
        sqlx::query("INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?, ?)")
            .bind(note_id)
            .bind(tag)
            .execute(pool)
            .await?;
    }

    Ok(())
}

pub async fn replace_links(
    pool: &SqlitePool,
    note_id: &str,
    owner_ghost: Option<&str>,
    links: &[(String, Option<String>)],
) -> KnowledgeResult<()> {
    sqlx::query("DELETE FROM note_links WHERE source_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;

    for (target_title, alias) in links {
        sqlx::query(
            "INSERT OR IGNORE INTO note_links (source_id, target_title, alias, owner_ghost) VALUES (?, ?, ?, ?)",
        )
        .bind(note_id)
        .bind(target_title)
        .bind(alias)
        .bind(owner_ghost)
        .execute(pool)
        .await?;
    }

    if let Some(owner) = owner_ghost {
        sqlx::query(
            r#"UPDATE note_links
               SET target_id = (
                   SELECT id FROM notes
                   WHERE notes.title = note_links.target_title
                     AND (notes.owner_ghost = ? OR notes.owner_ghost IS NULL)
                   ORDER BY notes.owner_ghost IS NULL ASC
                   LIMIT 1
               )
               WHERE source_id = ?"#,
        )
        .bind(owner)
        .bind(note_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            r#"UPDATE note_links
               SET target_id = (
                   SELECT id FROM notes
                   WHERE notes.title = note_links.target_title
                     AND notes.owner_ghost IS NULL
                   LIMIT 1
               )
               WHERE source_id = ?"#,
        )
        .bind(note_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn replace_chunks(
    pool: &SqlitePool,
    note_id: &str,
    note_title: &str,
    note_type: &str,
    chunks: &[ChunkRecord],
) -> KnowledgeResult<Vec<i64>> {
    let existing_ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .fetch_all(pool)
        .await?;

    sqlx::query("DELETE FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM chunk_fts WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    if !existing_ids.is_empty() {
        let placeholders = existing_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM chunk_vec WHERE rowid IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for (chunk_id,) in &existing_ids {
            q = q.bind(chunk_id);
        }
        q.execute(pool).await?;
    }

    let mut ids = Vec::new();
    for chunk in chunks {
        let result = sqlx::query(
            r#"INSERT INTO chunks (note_id, chunk_index, title, content, content_hash, embedding_model, embedding_dim, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&chunk.note_id)
        .bind(chunk.chunk_index)
        .bind(&chunk.title)
        .bind(&chunk.content)
        .bind(&chunk.content_hash)
        .bind(&chunk.embedding_model)
        .bind(chunk.embedding_dim)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

        let chunk_id = result.last_insert_rowid();
        ids.push(chunk_id);

        sqlx::query(
            r#"INSERT INTO chunk_fts (content, title, note_title, note_type, note_id, chunk_id)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&chunk.content)
        .bind(&chunk.title)
        .bind(note_title)
        .bind(note_type)
        .bind(note_id)
        .bind(chunk_id)
        .execute(pool)
        .await?;
    }

    Ok(ids)
}

pub async fn upsert_vec(
    pool: &SqlitePool,
    chunk_id: i64,
    embedding: &[f32],
) -> KnowledgeResult<()> {
    let payload = serde_json::to_string(embedding)
        .map_err(|e| KnowledgeError::Embedding(format!("embedding serialize failed: {e}")))?;

    sqlx::query("INSERT OR REPLACE INTO chunk_vec(rowid, embedding) VALUES (?, ?)")
        .bind(chunk_id)
        .bind(payload)
        .execute(pool)
        .await?;

    Ok(())
}

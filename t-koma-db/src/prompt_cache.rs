//! Prompt cache persistence for session system prompts.
//!
//! Stores rendered system prompt JSON to survive gateway restarts.

use sqlx::SqlitePool;

use crate::error::DbResult;

/// A cached prompt entry as stored in the DB.
#[derive(Debug, Clone)]
pub struct PromptCacheEntry {
    pub session_id: String,
    pub system_blocks_json: String,
    pub context_hash: String,
    pub cached_at: i64,
}

/// Repository for the prompt_cache table.
pub struct PromptCacheRepository;

impl PromptCacheRepository {
    /// Upsert a prompt cache entry.
    pub async fn upsert(pool: &SqlitePool, entry: &PromptCacheEntry) -> DbResult<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO prompt_cache (session_id, system_blocks_json, context_hash, cached_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&entry.session_id)
        .bind(&entry.system_blocks_json)
        .bind(&entry.context_hash)
        .bind(entry.cached_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Load all cached entries newer than `since_ts`.
    pub async fn load_recent(pool: &SqlitePool, since_ts: i64) -> DbResult<Vec<PromptCacheEntry>> {
        let rows = sqlx::query_as::<_, PromptCacheRow>(
            "SELECT session_id, system_blocks_json, context_hash, cached_at
             FROM prompt_cache
             WHERE cached_at > ?",
        )
        .bind(since_ts)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(PromptCacheEntry::from).collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct PromptCacheRow {
    session_id: String,
    system_blocks_json: String,
    context_hash: String,
    cached_at: i64,
}

impl From<PromptCacheRow> for PromptCacheEntry {
    fn from(row: PromptCacheRow) -> Self {
        PromptCacheEntry {
            session_id: row.session_id,
            system_blocks_json: row.system_blocks_json,
            context_hash: row.context_hash,
            cached_at: row.cached_at,
        }
    }
}

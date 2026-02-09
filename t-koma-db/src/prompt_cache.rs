//! Prompt cache persistence for session system prompts.
//!
//! Stores rendered system prompt JSON to survive gateway restarts.

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::DbResult;

/// A cached prompt entry as stored in the DB.
#[derive(Debug, Clone)]
pub struct PromptCacheEntry {
    pub id: String,
    pub ghost_id: String,
    pub session_id: String,
    pub system_blocks_json: String,
    pub context_hash: String,
    pub cached_at: i64,
}

impl PromptCacheEntry {
    /// Create a new prompt cache entry.
    pub fn new(
        ghost_id: &str,
        session_id: &str,
        system_blocks_json: &str,
        context_hash: &str,
        cached_at: i64,
    ) -> Self {
        Self {
            id: format!("cache_{}", Uuid::new_v4()),
            ghost_id: ghost_id.to_string(),
            session_id: session_id.to_string(),
            system_blocks_json: system_blocks_json.to_string(),
            context_hash: context_hash.to_string(),
            cached_at,
        }
    }
}

/// Repository for the prompt_cache table.
pub struct PromptCacheRepository;

impl PromptCacheRepository {
    /// Upsert a prompt cache entry.
    pub async fn upsert(pool: &SqlitePool, entry: &PromptCacheEntry) -> DbResult<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO prompt_cache (id, ghost_id, session_id, system_blocks_json, context_hash, cached_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.ghost_id)
        .bind(&entry.session_id)
        .bind(&entry.system_blocks_json)
        .bind(&entry.context_hash)
        .bind(entry.cached_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Load all cached entries newer than `since_ts` for a ghost.
    pub async fn load_recent(
        pool: &SqlitePool,
        ghost_id: &str,
        since_ts: i64,
    ) -> DbResult<Vec<PromptCacheEntry>> {
        let rows = sqlx::query_as::<_, PromptCacheRow>(
            "SELECT id, ghost_id, session_id, system_blocks_json, context_hash, cached_at
             FROM prompt_cache
             WHERE ghost_id = ? AND cached_at > ?",
        )
        .bind(ghost_id)
        .bind(since_ts)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(PromptCacheEntry::from).collect())
    }

    /// Delete cache entries for a session.
    pub async fn delete_for_session(pool: &SqlitePool, session_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM prompt_cache WHERE session_id = ?")
            .bind(session_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct PromptCacheRow {
    id: String,
    ghost_id: String,
    session_id: String,
    system_blocks_json: String,
    context_hash: String,
    cached_at: i64,
}

impl From<PromptCacheRow> for PromptCacheEntry {
    fn from(row: PromptCacheRow) -> Self {
        PromptCacheEntry {
            id: row.id,
            ghost_id: row.ghost_id,
            session_id: row.session_id,
            system_blocks_json: row.system_blocks_json,
            context_hash: row.context_hash,
            cached_at: row.cached_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GhostRepository, OperatorAccessLevel, OperatorRepository, Platform,
        sessions::SessionRepository, test_helpers::create_test_pool,
    };

    #[tokio::test]
    async fn test_upsert_and_load() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "TestOp",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let ghost = GhostRepository::create(pool, &operator.id, "TestGhost")
            .await
            .unwrap();
        let session = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();

        let entry = PromptCacheEntry::new(
            &ghost.id,
            &session.id,
            "[{\"type\": \"text\", \"text\": \"hello\"}]",
            "abc123",
            1234567890,
        );

        PromptCacheRepository::upsert(pool, &entry).await.unwrap();

        let loaded = PromptCacheRepository::load_recent(pool, &ghost.id, 0)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, session.id);
        assert_eq!(loaded[0].context_hash, "abc123");
    }

    #[tokio::test]
    async fn test_load_recent_filters_by_time() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "TestOp",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let ghost = GhostRepository::create(pool, &operator.id, "TestGhost")
            .await
            .unwrap();
        let session1 = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();
        let session2 = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();

        let entry1 = PromptCacheEntry::new(&ghost.id, &session1.id, "content1", "hash1", 1000);
        let entry2 = PromptCacheEntry::new(&ghost.id, &session2.id, "content2", "hash2", 2000);

        PromptCacheRepository::upsert(pool, &entry1).await.unwrap();
        PromptCacheRepository::upsert(pool, &entry2).await.unwrap();

        let loaded = PromptCacheRepository::load_recent(pool, &ghost.id, 1500)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session_id, session2.id);
    }
}

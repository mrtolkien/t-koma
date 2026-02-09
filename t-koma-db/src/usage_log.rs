//! Usage log storage for API token consumption tracking.
//!
//! Each API request produces a `UsageLog` row capturing the actual
//! token counts reported by the provider (input, output, cache hits).

use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::DbResult;

/// A single API request's token usage.
#[derive(Debug, Clone)]
pub struct UsageLog {
    pub session_id: String,
    pub message_id: Option<String>,
    pub request_at: i64,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
}

impl UsageLog {
    /// Create a new usage log entry for the current time.
    pub fn new(
        session_id: &str,
        message_id: Option<&str>,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        cache_read_tokens: u32,
        cache_creation_tokens: u32,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.map(|s| s.to_string()),
            request_at: Utc::now().timestamp(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        }
    }
}

/// Aggregated usage totals for a session.
#[derive(Debug, Clone, Default)]
pub struct UsageTotals {
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

/// Repository for usage_log table operations.
pub struct UsageLogRepository;

impl UsageLogRepository {
    /// Insert a usage log entry.
    pub async fn insert(pool: &SqlitePool, log: &UsageLog) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO usage_log (session_id, message_id, request_at, model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.session_id)
        .bind(&log.message_id)
        .bind(log.request_at)
        .bind(&log.model)
        .bind(log.input_tokens)
        .bind(log.output_tokens)
        .bind(log.cache_read_tokens)
        .bind(log.cache_creation_tokens)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get aggregated usage totals for a session.
    pub async fn session_totals(pool: &SqlitePool, session_id: &str) -> DbResult<UsageTotals> {
        let row = sqlx::query_as::<_, UsageTotalsRow>(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as cache_read_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as cache_creation_tokens
             FROM usage_log
             WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(pool)
        .await?;

        Ok(UsageTotals::from(row))
    }
}

#[derive(Debug, sqlx::FromRow)]
struct UsageTotalsRow {
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
}

impl From<UsageTotalsRow> for UsageTotals {
    fn from(row: UsageTotalsRow) -> Self {
        UsageTotals {
            request_count: row.request_count,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_creation_tokens: row.cache_creation_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::SessionRepository;
    use crate::test_helpers::create_test_ghost_pool;

    #[tokio::test]
    async fn test_insert_and_totals() {
        let db = create_test_ghost_pool("UsageGhost").await.unwrap();
        let pool = db.pool();

        let session = SessionRepository::create(pool, "op1").await.unwrap();

        let log1 = UsageLog::new(&session.id, None, "claude-sonnet-4-5", 1000, 200, 500, 100);
        UsageLogRepository::insert(pool, &log1).await.unwrap();

        let log2 = UsageLog::new(&session.id, None, "claude-sonnet-4-5", 1200, 300, 800, 0);
        UsageLogRepository::insert(pool, &log2).await.unwrap();

        let totals = UsageLogRepository::session_totals(pool, &session.id)
            .await
            .unwrap();

        assert_eq!(totals.request_count, 2);
        assert_eq!(totals.input_tokens, 2200);
        assert_eq!(totals.output_tokens, 500);
        assert_eq!(totals.cache_read_tokens, 1300);
        assert_eq!(totals.cache_creation_tokens, 100);
    }

    #[tokio::test]
    async fn test_empty_session_totals() {
        let db = create_test_ghost_pool("UsageGhost2").await.unwrap();
        let pool = db.pool();

        let session = SessionRepository::create(pool, "op1").await.unwrap();

        let totals = UsageLogRepository::session_totals(pool, &session.id)
            .await
            .unwrap();

        assert_eq!(totals.request_count, 0);
        assert_eq!(totals.input_tokens, 0);
    }

    #[tokio::test]
    async fn test_usage_with_message_id() {
        let db = create_test_ghost_pool("UsageGhost3").await.unwrap();
        let pool = db.pool();

        let session = SessionRepository::create(pool, "op1").await.unwrap();

        let log = UsageLog::new(
            &session.id,
            Some("msg_abc"),
            "claude-opus-4",
            5000,
            1000,
            3000,
            500,
        );
        UsageLogRepository::insert(pool, &log).await.unwrap();

        let totals = UsageLogRepository::session_totals(pool, &session.id)
            .await
            .unwrap();
        assert_eq!(totals.request_count, 1);
        assert_eq!(totals.input_tokens, 5000);
    }
}

//! Operator management operations.

use std::fmt;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::{DbError, DbResult};

/// Platform types for operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Discord,
    Api,
    Cli,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Discord => write!(f, "discord"),
            Platform::Api => write!(f, "api"),
            Platform::Cli => write!(f, "cli"),
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = DbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "discord" => Ok(Platform::Discord),
            "api" => Ok(Platform::Api),
            "cli" => Ok(Platform::Cli),
            _ => Err(DbError::Serialization(format!(
                "Invalid platform: {}",
                s
            ))),
        }
    }
}

/// Operator status types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorStatus {
    Pending,
    Approved,
    Denied,
}

impl fmt::Display for OperatorStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OperatorStatus::Pending => write!(f, "pending"),
            OperatorStatus::Approved => write!(f, "approved"),
            OperatorStatus::Denied => write!(f, "denied"),
        }
    }
}

impl std::str::FromStr for OperatorStatus {
    type Err = DbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(OperatorStatus::Pending),
            "approved" => Ok(OperatorStatus::Approved),
            "denied" => Ok(OperatorStatus::Denied),
            _ => Err(DbError::Serialization(format!(
                "Invalid status: {}",
                s
            ))),
        }
    }
}

/// Operator record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operator {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub status: OperatorStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub approved_at: Option<i64>,
    pub denied_at: Option<i64>,
    pub welcomed: bool,
}

/// Operator repository for database operations
pub struct OperatorRepository;

impl OperatorRepository {
    /// Create a new operator with pending status
    pub async fn create_new(
        pool: &SqlitePool,
        name: &str,
        platform: Platform,
    ) -> DbResult<Operator> {
        let id = format!("op_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();
        let status_str = OperatorStatus::Pending.to_string();

        sqlx::query(
            "INSERT INTO operators (id, name, platform, status, created_at, updated_at, welcomed)
             VALUES (?, ?, ?, ?, ?, ?, 0)",
        )
        .bind(&id)
        .bind(name)
        .bind(platform.to_string())
        .bind(status_str)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;

        Self::log_event(pool, &id, "created", None).await?;

        info!("Created new operator: {} (platform: {})", id, platform);

        Self::get_by_id(pool, &id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id))
    }

    /// Get or create an operator
    ///
    /// If the operator exists, returns the existing operator.
    /// If not, creates a new operator with pending status.
    pub async fn get_or_create(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        platform: Platform,
    ) -> DbResult<Operator> {
        if let Some(operator) = Self::get_by_id(pool, id).await? {
            debug!("Found existing operator: {}", id);
            return Ok(operator);
        }

        let now = Utc::now().timestamp();
        let status_str = OperatorStatus::Pending.to_string();

        sqlx::query(
            "INSERT INTO operators (id, name, platform, status, created_at, updated_at, welcomed)
             VALUES (?, ?, ?, ?, ?, ?, 0)",
        )
        .bind(id)
        .bind(name)
        .bind(platform.to_string())
        .bind(status_str)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "created", None).await?;

        info!("Created new operator: {} (platform: {})", id, platform);

        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id.to_string()))
    }

    /// Get operator by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<Operator>> {
        let row = sqlx::query_as::<_, OperatorRow>(
            "SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
             FROM operators
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Operator::from))
    }

    /// Check if an operator is approved
    pub async fn is_approved(pool: &SqlitePool, id: &str) -> DbResult<bool> {
        let row = sqlx::query("SELECT status FROM operators WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

        Ok(row
            .and_then(|r| r.try_get::<String, _>("status").ok())
            .map(|status| status == OperatorStatus::Approved.to_string())
            .unwrap_or(false))
    }

    /// Approve an operator
    pub async fn approve(pool: &SqlitePool, id: &str) -> DbResult<Operator> {
        let operator = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id.to_string()))?;

        if operator.status == OperatorStatus::Approved {
            return Ok(operator);
        }

        if operator.status == OperatorStatus::Denied {
            return Err(DbError::InvalidTransition {
                from: operator.status.to_string(),
                to: OperatorStatus::Approved.to_string(),
            });
        }

        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE operators
             SET status = ?, approved_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(OperatorStatus::Approved.to_string())
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "approved", None).await?;

        info!("Approved operator: {}", id);

        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id.to_string()))
    }

    /// Deny an operator
    pub async fn deny(pool: &SqlitePool, id: &str) -> DbResult<Operator> {
        let operator = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id.to_string()))?;

        if operator.status == OperatorStatus::Denied {
            return Ok(operator);
        }

        if operator.status == OperatorStatus::Approved {
            return Err(DbError::InvalidTransition {
                from: operator.status.to_string(),
                to: OperatorStatus::Denied.to_string(),
            });
        }

        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE operators
             SET status = ?, denied_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(OperatorStatus::Denied.to_string())
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "denied", None).await?;

        info!("Denied operator: {}", id);

        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::OperatorNotFound(id.to_string()))
    }

    /// Mark operator as welcomed (typically for Discord operators)
    pub async fn mark_welcomed(pool: &SqlitePool, id: &str) -> DbResult<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE operators
             SET welcomed = 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "welcomed", None).await?;

        info!("Marked operator as welcomed: {}", id);
        Ok(())
    }

    /// List operators by status (and optionally by platform)
    pub async fn list_by_status(
        pool: &SqlitePool,
        status: OperatorStatus,
        platform: Option<Platform>,
    ) -> DbResult<Vec<Operator>> {
        let status_str = status.to_string();

        let rows = match platform {
            Some(platform) => {
                sqlx::query_as::<_, OperatorRow>(
                    "SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
                     FROM operators
                     WHERE status = ? AND platform = ?
                     ORDER BY created_at ASC",
                )
                .bind(status_str)
                .bind(platform.to_string())
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, OperatorRow>(
                    "SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
                     FROM operators
                     WHERE status = ?
                     ORDER BY created_at ASC",
                )
                .bind(status_str)
                .fetch_all(pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(Operator::from).collect())
    }

    /// List all operators
    pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Operator>> {
        let rows = sqlx::query_as::<_, OperatorRow>(
            "SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
             FROM operators
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Operator::from).collect())
    }

    /// Remove an operator completely
    pub async fn remove(pool: &SqlitePool, id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM operators WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;

        Self::log_event(pool, id, "removed", None).await?;

        info!("Removed operator: {}", id);
        Ok(())
    }

    /// Auto-prune pending operators older than the specified hours
    pub async fn prune_pending(pool: &SqlitePool, hours: i64) -> DbResult<i64> {
        let cutoff = Utc::now().timestamp() - (hours * 3600);

        let pending_ids: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM operators WHERE status = 'pending' AND created_at < ?",
        )
        .bind(cutoff)
        .fetch_all(pool)
        .await?;

        for (operator_id,) in &pending_ids {
            Self::log_event(pool, operator_id, "removed", Some("auto-prune")).await?;
        }

        let result = sqlx::query(
            "DELETE FROM operators WHERE status = 'pending' AND created_at < ?",
        )
        .bind(cutoff)
        .execute(pool)
        .await?;

        let count = result.rows_affected() as i64;
        info!(
            "Pruned {} pending operators older than {} hours",
            count, hours
        );
        Ok(count)
    }

    async fn log_event(
        pool: &SqlitePool,
        operator_id: &str,
        event_type: &str,
        event_data: Option<&str>,
    ) -> DbResult<()> {
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO operator_events (operator_id, event_type, event_data, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(operator_id)
        .bind(event_type)
        .bind(event_data)
        .bind(now)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct OperatorRow {
    id: String,
    name: String,
    platform: String,
    status: String,
    created_at: i64,
    updated_at: i64,
    approved_at: Option<i64>,
    denied_at: Option<i64>,
    welcomed: i64,
}

impl From<OperatorRow> for Operator {
    fn from(row: OperatorRow) -> Self {
        Operator {
            id: row.id,
            name: row.name,
            platform: row.platform.parse().unwrap_or(Platform::Api),
            status: row.status.parse().unwrap_or(OperatorStatus::Pending),
            created_at: row.created_at,
            updated_at: row.updated_at,
            approved_at: row.approved_at,
            denied_at: row.denied_at,
            welcomed: row.welcomed != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::create_test_koma_pool;

    #[tokio::test]
    async fn test_get_or_create_operator() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::get_or_create(
            pool,
            "operator1",
            "Test Operator",
            Platform::Discord,
        )
        .await
        .unwrap();

        assert_eq!(operator.id, "operator1");
        assert_eq!(operator.name, "Test Operator");
        assert_eq!(operator.platform, Platform::Discord);
        assert_eq!(operator.status, OperatorStatus::Pending);
        assert!(!operator.welcomed);

        let operator2 = OperatorRepository::get_or_create(
            pool,
            "operator1",
            "Different Name",
            Platform::Discord,
        )
        .await
        .unwrap();

        assert_eq!(operator2.name, "Test Operator");
    }

    #[tokio::test]
    async fn test_approve_operator() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        OperatorRepository::get_or_create(pool, "operator1", "Test", Platform::Discord)
            .await
            .unwrap();

        let operator = OperatorRepository::approve(pool, "operator1").await.unwrap();
        assert_eq!(operator.status, OperatorStatus::Approved);
        assert!(operator.approved_at.is_some());

        assert!(OperatorRepository::is_approved(pool, "operator1")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_deny_operator() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        OperatorRepository::get_or_create(pool, "operator1", "Test", Platform::Discord)
            .await
            .unwrap();

        let operator = OperatorRepository::deny(pool, "operator1").await.unwrap();
        assert_eq!(operator.status, OperatorStatus::Denied);
        assert!(operator.denied_at.is_some());

        let result = OperatorRepository::approve(pool, "operator1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_by_status() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        OperatorRepository::get_or_create(pool, "operator1", "Op 1", Platform::Discord)
            .await
            .unwrap();
        OperatorRepository::get_or_create(pool, "operator2", "Op 2", Platform::Discord)
            .await
            .unwrap();
        OperatorRepository::get_or_create(pool, "operator3", "Op 3", Platform::Api)
            .await
            .unwrap();

        OperatorRepository::approve(pool, "operator1").await.unwrap();

        let pending = OperatorRepository::list_by_status(pool, OperatorStatus::Pending, None)
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);

        let approved = OperatorRepository::list_by_status(pool, OperatorStatus::Approved, None)
            .await
            .unwrap();
        assert_eq!(approved.len(), 1);

        let discord_pending = OperatorRepository::list_by_status(
            pool,
            OperatorStatus::Pending,
            Some(Platform::Discord),
        )
        .await
        .unwrap();
        assert_eq!(discord_pending.len(), 1);
    }

    #[tokio::test]
    async fn test_prune_pending() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        OperatorRepository::get_or_create(pool, "operator1", "Op 1", Platform::Discord)
            .await
            .unwrap();

        sqlx::query("UPDATE operators SET created_at = ? WHERE id = 'operator1'")
            .bind(Utc::now().timestamp() - 7200)
            .execute(pool)
            .await
            .unwrap();

        let pruned = OperatorRepository::prune_pending(pool, 1).await.unwrap();
        assert_eq!(pruned, 1);

        let operator = OperatorRepository::get_by_id(pool, "operator1").await.unwrap();
        assert!(operator.is_none());
    }

    #[tokio::test]
    async fn test_mark_welcomed() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        OperatorRepository::get_or_create(pool, "operator1", "Op 1", Platform::Discord)
            .await
            .unwrap();
        OperatorRepository::approve(pool, "operator1").await.unwrap();
        OperatorRepository::mark_welcomed(pool, "operator1")
            .await
            .unwrap();

        let operator = OperatorRepository::get_by_id(pool, "operator1")
            .await
            .unwrap()
            .unwrap();
        assert!(operator.welcomed);
    }
}

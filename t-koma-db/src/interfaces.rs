//! Interface management operations.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::error::{DbError, DbResult};
use crate::operators::Platform;

/// Interface record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    pub id: String,
    pub operator_id: String,
    pub platform: Platform,
    pub external_id: String,
    pub display_name: String,
    pub created_at: i64,
}

/// Interface repository for database operations
pub struct InterfaceRepository;

impl InterfaceRepository {
    /// Get interface by platform/external ID
    pub async fn get_by_external_id(
        pool: &SqlitePool,
        platform: Platform,
        external_id: &str,
    ) -> DbResult<Option<Interface>> {
        let row = sqlx::query_as::<_, InterfaceRow>(
            "SELECT id, operator_id, platform, external_id, display_name, created_at
             FROM interfaces
             WHERE platform = ? AND external_id = ?",
        )
        .bind(platform.to_string())
        .bind(external_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Interface::from))
    }

    /// Create a new interface (platform + external_id must be unique)
    pub async fn create(
        pool: &SqlitePool,
        operator_id: &str,
        platform: Platform,
        external_id: &str,
        display_name: &str,
    ) -> DbResult<Interface> {
        if let Some(existing) = Self::get_by_external_id(pool, platform, external_id).await? {
            return Err(DbError::InterfaceAlreadyExists(existing.external_id));
        }

        let id = format!("iface_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO interfaces (id, operator_id, platform, external_id, display_name, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(operator_id)
        .bind(platform.to_string())
        .bind(external_id)
        .bind(display_name)
        .bind(now)
        .execute(pool)
        .await?;

        info!(
            "Created interface {} for operator {} ({:?})",
            external_id, operator_id, platform
        );

        Self::get_by_external_id(pool, platform, external_id)
            .await?
            .ok_or_else(|| DbError::InterfaceNotFound(external_id.to_string()))
    }

    /// List all interfaces for an operator
    pub async fn list_by_operator(
        pool: &SqlitePool,
        operator_id: &str,
    ) -> DbResult<Vec<Interface>> {
        let rows = sqlx::query_as::<_, InterfaceRow>(
            "SELECT id, operator_id, platform, external_id, display_name, created_at
             FROM interfaces
             WHERE operator_id = ?
             ORDER BY created_at ASC",
        )
        .bind(operator_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Interface::from).collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct InterfaceRow {
    id: String,
    operator_id: String,
    platform: String,
    external_id: String,
    display_name: String,
    created_at: i64,
}

impl From<InterfaceRow> for Interface {
    fn from(row: InterfaceRow) -> Self {
        Interface {
            id: row.id,
            operator_id: row.operator_id,
            platform: row.platform.parse().unwrap_or(Platform::Api),
            external_id: row.external_id,
            display_name: row.display_name,
            created_at: row.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OperatorAccessLevel, OperatorRepository, Platform, test_helpers::create_test_koma_pool,
    };

    #[tokio::test]
    async fn test_create_interface() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "Test Operator",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let iface = InterfaceRepository::create(
            pool,
            &operator.id,
            Platform::Discord,
            "discord-123",
            "Test User",
        )
        .await
        .unwrap();

        assert_eq!(iface.external_id, "discord-123");

        let found = InterfaceRepository::get_by_external_id(pool, Platform::Discord, "discord-123")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(found.id, iface.id);
    }
}

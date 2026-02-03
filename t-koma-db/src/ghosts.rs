//! Ghost management operations.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::error::{DbError, DbResult};

const GHOST_NAME_MAX_LEN: usize = 64;

/// Validate and normalize a ghost name.
///
/// - Trim whitespace
/// - Enforce ASCII letters, digits, spaces, '-' and '_'
/// - No empty names
pub fn validate_ghost_name(name: &str) -> DbResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(DbError::InvalidGhostName(
            "Ghost name cannot be empty".to_string(),
        ));
    }

    if trimmed.len() > GHOST_NAME_MAX_LEN {
        return Err(DbError::InvalidGhostName(format!(
            "Ghost name must be <= {} characters",
            GHOST_NAME_MAX_LEN
        )));
    }

    let valid = trimmed.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ' '
    });

    if !valid {
        return Err(DbError::InvalidGhostName(
            "Ghost name may contain only letters, numbers, spaces, '-' and '_'".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

/// Ghost record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ghost {
    pub id: String,
    pub name: String,
    pub owner_operator_id: String,
    pub created_at: i64,
}

/// Ghost repository for database operations
pub struct GhostRepository;

impl GhostRepository {
    /// Create a new ghost for an operator
    pub async fn create(
        pool: &SqlitePool,
        owner_operator_id: &str,
        name: &str,
    ) -> DbResult<Ghost> {
        let name = validate_ghost_name(name)?;

        if let Some(existing) = Self::get_by_name(pool, &name).await? {
            return Err(DbError::GhostNameTaken(existing.name));
        }

        let id = format!("ghost_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO ghosts (id, name, owner_operator_id, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(owner_operator_id)
        .bind(now)
        .execute(pool)
        .await?;

        info!("Created ghost: {} for operator: {}", name, owner_operator_id);

        Self::get_by_id(pool, &id)
            .await?
            .ok_or_else(|| DbError::GhostNotFound(id))
    }

    /// Get ghost by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<Ghost>> {
        let row = sqlx::query_as::<_, GhostRow>(
            "SELECT id, name, owner_operator_id, created_at
             FROM ghosts
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Ghost::from))
    }

    /// Get ghost by name
    pub async fn get_by_name(pool: &SqlitePool, name: &str) -> DbResult<Option<Ghost>> {
        let row = sqlx::query_as::<_, GhostRow>(
            "SELECT id, name, owner_operator_id, created_at
             FROM ghosts
             WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Ghost::from))
    }

    /// List ghosts for an operator
    pub async fn list_by_operator(
        pool: &SqlitePool,
        owner_operator_id: &str,
    ) -> DbResult<Vec<Ghost>> {
        let rows = sqlx::query_as::<_, GhostRow>(
            "SELECT id, name, owner_operator_id, created_at
             FROM ghosts
             WHERE owner_operator_id = ?
             ORDER BY created_at ASC",
        )
        .bind(owner_operator_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Ghost::from).collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct GhostRow {
    id: String,
    name: String,
    owner_operator_id: String,
    created_at: i64,
}

impl From<GhostRow> for Ghost {
    fn from(row: GhostRow) -> Self {
        Ghost {
            id: row.id,
            name: row.name,
            owner_operator_id: row.owner_operator_id,
            created_at: row.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_helpers::create_test_koma_pool, OperatorRepository, Platform};

    #[tokio::test]
    async fn test_validate_ghost_name() {
        assert!(validate_ghost_name("Alpha-1").is_ok());
        assert!(validate_ghost_name("Ghost_02").is_ok());
        assert!(validate_ghost_name(" ").is_err());
        assert!(validate_ghost_name("../oops").is_err());
    }

    #[tokio::test]
    async fn test_create_ghost() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(pool, "Test Operator", Platform::Api)
            .await
            .unwrap();

        let ghost = GhostRepository::create(pool, &operator.id, "Alpha")
            .await
            .unwrap();

        assert_eq!(ghost.name, "Alpha");

        let duplicate = GhostRepository::create(pool, &operator.id, "Alpha").await;
        assert!(duplicate.is_err());
    }
}

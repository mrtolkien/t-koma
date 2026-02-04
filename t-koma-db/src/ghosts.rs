//! Ghost management operations.

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::error::{DbError, DbResult};

const GHOST_NAME_MAX_LEN: usize = 64;

/// Validate and normalize a ghost name.
///
/// - Trim whitespace
/// - Enforce ASCII letters, digits, spaces, '-' and '_' plus kanji and katakana
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

    let valid = Regex::new(r"^[A-Za-z0-9 _\-\p{Han}\p{Katakana}]+$")
        .expect("ghost name regex should compile")
        .is_match(trimmed);

    if !valid {
        return Err(DbError::InvalidGhostName(
            "Ghost name may contain only letters, numbers, spaces, '-', '_', kanji, and katakana"
                .to_string(),
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
    pub cwd: Option<String>,
    pub created_at: i64,
}

/// Ghost tool state (cwd only)
#[derive(Debug, Clone)]
pub struct GhostToolState {
    pub cwd: Option<String>,
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
        let default_cwd = crate::ghost_db::GhostDbPool::workspace_path_for(&name)?;
        let default_cwd = default_cwd.to_string_lossy().to_string();

        if let Some(existing) = Self::get_by_name(pool, &name).await? {
            return Err(DbError::GhostNameTaken(existing.name));
        }

        let id = format!("ghost_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO ghosts (id, name, owner_operator_id, cwd, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(owner_operator_id)
        .bind(default_cwd)
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
            "SELECT id, name, owner_operator_id, cwd, created_at
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
            "SELECT id, name, owner_operator_id, cwd, created_at
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
            "SELECT id, name, owner_operator_id, cwd, created_at
             FROM ghosts
             WHERE owner_operator_id = ?
             ORDER BY created_at ASC",
        )
        .bind(owner_operator_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Ghost::from).collect())
    }

    /// List all ghosts.
    pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Ghost>> {
        let rows = sqlx::query_as::<_, GhostRow>(
            "SELECT id, name, owner_operator_id, cwd, created_at
             FROM ghosts
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Ghost::from).collect())
    }

    /// Get tool state (cwd + allow flag) for a ghost by name
    pub async fn get_tool_state_by_name(
        pool: &SqlitePool,
        name: &str,
    ) -> DbResult<GhostToolState> {
        let row = sqlx::query_as::<_, GhostToolStateRow>(
            "SELECT cwd
             FROM ghosts
             WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        let Some(row) = row else {
            return Err(DbError::GhostNotFound(name.to_string()));
        };

        Ok(GhostToolState { cwd: row.cwd })
    }

    /// Update tool state (cwd) for a ghost by name
    pub async fn update_tool_state_by_name(
        pool: &SqlitePool,
        name: &str,
        cwd: &str,
    ) -> DbResult<()> {
        let updated = sqlx::query(
            "UPDATE ghosts
             SET cwd = ?
             WHERE name = ?",
        )
        .bind(cwd)
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected();

        if updated == 0 {
            return Err(DbError::GhostNotFound(name.to_string()));
        }

        Ok(())
    }

    /// Delete a ghost by name.
    pub async fn delete_by_name(pool: &SqlitePool, name: &str) -> DbResult<()> {
        let deleted = sqlx::query("DELETE FROM ghosts WHERE name = ?")
            .bind(name)
            .execute(pool)
            .await?
            .rows_affected();

        if deleted == 0 {
            return Err(DbError::GhostNotFound(name.to_string()));
        }

        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct GhostRow {
    id: String,
    name: String,
    owner_operator_id: String,
    cwd: Option<String>,
    created_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct GhostToolStateRow {
    cwd: Option<String>,
}

impl From<GhostRow> for Ghost {
    fn from(row: GhostRow) -> Self {
        Ghost {
            id: row.id,
            name: row.name,
            owner_operator_id: row.owner_operator_id,
            cwd: row.cwd,
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
        assert!(validate_ghost_name("カタカナ").is_ok());
        assert!(validate_ghost_name("漢字テスト").is_ok());
        assert!(validate_ghost_name("ひらがな").is_err());
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

    #[tokio::test]
    async fn test_list_all_and_delete_by_name() {
        let db = create_test_koma_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(pool, "Test Operator", Platform::Api)
            .await
            .unwrap();

        GhostRepository::create(pool, &operator.id, "Alpha")
            .await
            .unwrap();
        GhostRepository::create(pool, &operator.id, "Beta")
            .await
            .unwrap();

        let all = GhostRepository::list_all(pool).await.unwrap();
        assert_eq!(all.len(), 2);

        GhostRepository::delete_by_name(pool, "Alpha").await.unwrap();
        let remaining = GhostRepository::list_all(pool).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "Beta");
    }
}

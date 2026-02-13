//! Ghost management operations.

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;

use crate::error::{DbError, DbResult};

const GHOST_NAME_MAX_LEN: usize = 64;

/// Validate and normalize a ghost name.
///
/// - Trim whitespace
/// - Enforce Unicode letters, numbers, spaces, '-' and '_'
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

    let valid = Regex::new(r"^[\p{L}\p{M}\p{N} _\-]+$")
        .expect("ghost name regex should compile")
        .is_match(trimmed);

    if !valid {
        return Err(DbError::InvalidGhostName(
            "Ghost name may contain only letters, numbers, spaces, '-', and '_'".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

/// Get workspace path for a ghost
pub fn ghost_workspace_path(ghost_name: &str) -> DbResult<PathBuf> {
    let ghost_name = validate_ghost_name(ghost_name)?;
    if let Ok(override_dir) = std::env::var("T_KOMA_DATA_DIR") {
        return Ok(PathBuf::from(override_dir).join("ghosts").join(ghost_name));
    }
    let data_dir = dirs::data_dir().ok_or(DbError::NoConfigDir)?;
    Ok(data_dir.join("t-koma").join("ghosts").join(ghost_name))
}

/// Ghost record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ghost {
    pub id: String,
    pub name: String,
    pub owner_operator_id: String,
    pub cwd: Option<String>,
    /// JSON-encoded model alias override (single string or list).
    /// When set, this ghost uses its own model chain instead of the global default.
    pub model_aliases: Option<String>,
    /// Optional heartbeat model override (single alias or fallback chain JSON).
    pub heartbeat_model_aliases: Option<String>,
    /// Optional reflection model override (single alias or fallback chain JSON).
    pub reflection_model_aliases: Option<String>,
    /// Whether to append a metadata statusline to each response.
    pub statusline: bool,
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
    pub async fn create(pool: &SqlitePool, owner_operator_id: &str, name: &str) -> DbResult<Ghost> {
        let name = validate_ghost_name(name)?;
        let default_cwd = ghost_workspace_path(&name)?;
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
        .bind(&default_cwd)
        .bind(now)
        .execute(pool)
        .await?;

        info!(
            "Created ghost: {} for operator: {}",
            name, owner_operator_id
        );

        Self::get_by_id(pool, &id)
            .await?
            .ok_or_else(|| DbError::GhostNotFound(id))
    }

    /// Get ghost by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<Ghost>> {
        let row = sqlx::query_as::<_, GhostRow>(
            "SELECT id, name, owner_operator_id, cwd, model_aliases, heartbeat_model_aliases, reflection_model_aliases, statusline, created_at
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
            "SELECT id, name, owner_operator_id, cwd, model_aliases, heartbeat_model_aliases, reflection_model_aliases, statusline, created_at
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
            "SELECT id, name, owner_operator_id, cwd, model_aliases, heartbeat_model_aliases, reflection_model_aliases, statusline, created_at
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
            "SELECT id, name, owner_operator_id, cwd, model_aliases, heartbeat_model_aliases, reflection_model_aliases, statusline, created_at
             FROM ghosts
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Ghost::from).collect())
    }

    /// Get tool state (cwd + allow flag) for a ghost by name
    pub async fn get_tool_state_by_name(pool: &SqlitePool, name: &str) -> DbResult<GhostToolState> {
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

    /// Update the model alias override for a ghost by name.
    ///
    /// Pass `None` to clear the override (revert to global default).
    /// Pass `Some(json)` with a JSON-encoded `ModelAliases` value.
    pub async fn update_model_aliases(
        pool: &SqlitePool,
        name: &str,
        model_aliases: Option<&str>,
    ) -> DbResult<()> {
        let updated = sqlx::query(
            "UPDATE ghosts
             SET model_aliases = ?
             WHERE name = ?",
        )
        .bind(model_aliases)
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected();

        if updated == 0 {
            return Err(DbError::GhostNotFound(name.to_string()));
        }

        Ok(())
    }

    /// Toggle the statusline preference for a ghost.
    pub async fn set_statusline(pool: &SqlitePool, name: &str, enabled: bool) -> DbResult<()> {
        let updated = sqlx::query("UPDATE ghosts SET statusline = ? WHERE name = ?")
            .bind(enabled)
            .bind(name)
            .execute(pool)
            .await?
            .rows_affected();

        if updated == 0 {
            return Err(DbError::GhostNotFound(name.to_string()));
        }
        Ok(())
    }

    /// Update the heartbeat model alias override for a ghost by name.
    pub async fn update_heartbeat_model_aliases(
        pool: &SqlitePool,
        name: &str,
        model_aliases: Option<&str>,
    ) -> DbResult<()> {
        let updated = sqlx::query(
            "UPDATE ghosts
             SET heartbeat_model_aliases = ?
             WHERE name = ?",
        )
        .bind(model_aliases)
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected();

        if updated == 0 {
            return Err(DbError::GhostNotFound(name.to_string()));
        }

        Ok(())
    }

    /// Update the reflection model alias override for a ghost by name.
    pub async fn update_reflection_model_aliases(
        pool: &SqlitePool,
        name: &str,
        model_aliases: Option<&str>,
    ) -> DbResult<()> {
        let updated = sqlx::query(
            "UPDATE ghosts
             SET reflection_model_aliases = ?
             WHERE name = ?",
        )
        .bind(model_aliases)
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
    model_aliases: Option<String>,
    heartbeat_model_aliases: Option<String>,
    reflection_model_aliases: Option<String>,
    statusline: bool,
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
            model_aliases: row.model_aliases,
            heartbeat_model_aliases: row.heartbeat_model_aliases,
            reflection_model_aliases: row.reflection_model_aliases,
            statusline: row.statusline,
            created_at: row.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OperatorAccessLevel, OperatorRepository, Platform, test_helpers::create_test_pool,
    };

    #[tokio::test]
    async fn test_validate_ghost_name() {
        assert!(validate_ghost_name("Alpha-1").is_ok());
        assert!(validate_ghost_name("Ghost_02").is_ok());
        assert!(validate_ghost_name("カタカナ").is_ok());
        assert!(validate_ghost_name("漢字テスト").is_ok());
        assert!(validate_ghost_name("ひらがな").is_ok());
        assert!(validate_ghost_name("クランカー").is_ok());
        assert!(validate_ghost_name(" ").is_err());
        assert!(validate_ghost_name("../oops").is_err());
    }

    #[tokio::test]
    async fn test_create_ghost() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "Test Operator",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
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
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "Test Operator",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
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

        GhostRepository::delete_by_name(pool, "Alpha")
            .await
            .unwrap();
        let remaining = GhostRepository::list_all(pool).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "Beta");
    }

    #[tokio::test]
    async fn test_set_statusline() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "Test Operator",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();

        let ghost = GhostRepository::create(pool, &operator.id, "Alpha")
            .await
            .unwrap();
        assert!(!ghost.statusline);

        GhostRepository::set_statusline(pool, "Alpha", true)
            .await
            .unwrap();
        let updated = GhostRepository::get_by_name(pool, "Alpha")
            .await
            .unwrap()
            .unwrap();
        assert!(updated.statusline);

        GhostRepository::set_statusline(pool, "Alpha", false)
            .await
            .unwrap();
        let toggled = GhostRepository::get_by_name(pool, "Alpha")
            .await
            .unwrap()
            .unwrap();
        assert!(!toggled.statusline);

        let err = GhostRepository::set_statusline(pool, "nonexistent", true).await;
        assert!(err.is_err());
    }
}

//! GHOST (ゴースト) database connection pool and initialization.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tracing::info;

use crate::{
    error::{DbError, DbResult},
    ghosts::validate_ghost_name,
    sqlite_runtime::create_file_pool,
};

/// GHOST database pool wrapper (per-ghost)
#[derive(Debug, Clone)]
pub struct GhostDbPool {
    pool: SqlitePool,
    ghost_name: String,
    workspace_path: PathBuf,
}

impl GhostDbPool {
    /// Initialize a ghost database with migrations
    pub async fn new(ghost_name: &str) -> DbResult<Self> {
        let ghost_name = validate_ghost_name(ghost_name)?;
        let workspace_path = Self::workspace_path_for(&ghost_name)?;
        let db_path = workspace_path.join("db.sqlite3");

        info!("Initializing GHOST database at: {}", db_path.display());

        std::fs::create_dir_all(&workspace_path)?;

        let pool = create_file_pool(&db_path, 5).await?;

        Self::run_migrations(&pool).await?;

        info!("GHOST database initialized successfully");
        Ok(Self {
            pool,
            ghost_name,
            workspace_path,
        })
    }

    /// Get the inner SQLx pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get ghost name
    pub fn ghost_name(&self) -> &str {
        &self.ghost_name
    }

    /// Get ghost workspace path
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Get workspace path for a ghost
    pub fn workspace_path_for(ghost_name: &str) -> DbResult<PathBuf> {
        let ghost_name = validate_ghost_name(ghost_name)?;
        if let Ok(override_dir) = std::env::var("T_KOMA_DATA_DIR") {
            return Ok(PathBuf::from(override_dir).join("ghosts").join(ghost_name));
        }
        let data_dir = dirs::data_dir().ok_or(DbError::NoConfigDir)?;
        Ok(data_dir.join("t-koma").join("ghosts").join(ghost_name))
    }

    async fn run_migrations(pool: &SqlitePool) -> DbResult<()> {
        sqlx::migrate!("./migrations/ghost")
            .run(pool)
            .await
            .map_err(|e| DbError::Migration(e.to_string()))?;

        info!("GHOST database migrations completed");
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub fn from_pool(
        pool: SqlitePool,
        ghost_name: &str,
        workspace_path: PathBuf,
    ) -> DbResult<Self> {
        let ghost_name = validate_ghost_name(ghost_name)?;
        Ok(Self {
            pool,
            ghost_name,
            workspace_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GhostDbPool;
    use crate::ENV_MUTEX;

    #[test]
    fn test_workspace_path_for_uses_env_override() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let value = dir.path().to_string_lossy().to_string();
        // SAFETY: test-scoped env mutation.
        unsafe { std::env::set_var("T_KOMA_DATA_DIR", &value) };
        let path = GhostDbPool::workspace_path_for("Alpha").unwrap();
        // SAFETY: test-scoped env mutation cleanup.
        unsafe { std::env::remove_var("T_KOMA_DATA_DIR") };
        assert_eq!(path, dir.path().join("ghosts").join("Alpha"));
    }
}

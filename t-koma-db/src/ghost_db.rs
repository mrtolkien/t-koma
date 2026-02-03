//! GHOST (ゴースト) database connection pool and initialization.

use std::path::{Path, PathBuf};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use tracing::info;

use crate::{error::{DbError, DbResult}, ghosts::validate_ghost_name};

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

        info!(
            "Initializing GHOST database at: {}",
            db_path.display()
        );

        std::fs::create_dir_all(&workspace_path)?;

        Self::init_sqlite_vec()?;

        let db_url = format!("sqlite:{}", db_path.display());
        let pool = Self::create_pool(&db_url).await?;

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

    /// Get ghost workspace path (safe house)
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Get workspace path for a ghost
    pub fn workspace_path_for(ghost_name: &str) -> DbResult<PathBuf> {
        let ghost_name = validate_ghost_name(ghost_name)?;
        let data_dir = dirs::data_dir().ok_or(DbError::NoConfigDir)?;
        Ok(data_dir.join("t-koma").join("ghosts").join(ghost_name))
    }

    fn init_sqlite_vec() -> DbResult<()> {
        use rusqlite::ffi::sqlite3_auto_extension;
        use sqlite_vec::sqlite3_vec_init;

        unsafe {
            type SqliteVecInitFn = unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32;
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                SqliteVecInitFn,
            >(sqlite3_vec_init as *const ())));
        }
        Ok(())
    }

    async fn create_pool(database_url: &str) -> DbResult<SqlitePool> {
        let options = SqliteConnectOptions::new()
            .filename(database_url.strip_prefix("sqlite:").unwrap_or(database_url))
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA cache_size = -64000")
            .execute(&pool)
            .await?;

        Ok(pool)
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

    pub fn from_pool(pool: SqlitePool, ghost_name: &str, workspace_path: PathBuf) -> DbResult<Self> {
        let ghost_name = validate_ghost_name(ghost_name)?;
        Ok(Self {
            pool,
            ghost_name,
            workspace_path,
        })
    }
}

//! T-KOMA (ティーコマ) database connection pool and initialization.

use std::path::PathBuf;

use sqlx::SqlitePool;
use tracing::info;

use crate::{
    error::{DbError, DbResult},
    sqlite_runtime::create_file_pool,
};

/// T-KOMA database pool wrapper
#[derive(Debug, Clone)]
pub struct KomaDbPool {
    pool: SqlitePool,
}

impl KomaDbPool {
    /// Initialize database with migrations
    ///
    /// This function:
    /// 1. Ensures the data directory exists
    /// 2. Initializes sqlite-vec extension
    /// 3. Creates/connects to the database
    /// 4. Runs migrations
    pub async fn new() -> DbResult<Self> {
        let db_path = Self::db_path()?;
        info!("Initializing T-KOMA database at: {}", db_path.display());

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let pool = create_file_pool(&db_path, 5).await?;

        Self::run_migrations(&pool).await?;

        info!("T-KOMA database initialized successfully");
        Ok(Self { pool })
    }

    /// Get the inner SQLx pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get database file path
    pub fn db_path() -> DbResult<PathBuf> {
        let data_dir = dirs::data_dir().ok_or(DbError::NoConfigDir)?;
        Ok(data_dir.join("t-koma").join("koma.sqlite3"))
    }

    /// Run database migrations using sqlx migrate macro
    async fn run_migrations(pool: &SqlitePool) -> DbResult<()> {
        sqlx::migrate!("./migrations/koma")
            .run(pool)
            .await
            .map_err(|e| DbError::Migration(e.to_string()))?;

        info!("T-KOMA database migrations completed");
        Ok(())
    }

    /// Close the pool gracefully
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Create a KomaDbPool from an existing SqlitePool (for testing)
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

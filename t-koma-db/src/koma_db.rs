//! T-KOMA (ティーコマ) database connection pool and initialization.

use std::path::PathBuf;

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use tracing::info;

use crate::error::{DbError, DbResult};

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

        Self::init_sqlite_vec()?;

        let db_url = format!("sqlite:{}", db_path.display());
        let pool = Self::create_pool(&db_url).await?;

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

    /// Initialize sqlite-vec extension globally
    ///
    /// Must be called once at application startup, before any database connections
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

    /// Create a connection pool with optimal settings
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

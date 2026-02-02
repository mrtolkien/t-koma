//! Database connection pool and initialization.

use std::path::PathBuf;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tracing::info;

use crate::error::{DbError, DbResult};

/// Database pool wrapper
#[derive(Debug, Clone)]
pub struct DbPool {
    pool: SqlitePool,
}

impl DbPool {
    /// Initialize database with migrations
    ///
    /// This function:
    /// 1. Ensures the data directory exists
    /// 2. Initializes sqlite-vec extension
    /// 3. Creates/connects to the database
    /// 4. Runs migrations
    pub async fn new() -> DbResult<Self> {
        let db_path = Self::db_path()?;
        info!("Initializing database at: {}", db_path.display());

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Initialize sqlite-vec extension globally
        Self::init_sqlite_vec()?;

        // Create connection pool with optimal settings
        let db_url = format!("sqlite:{}", db_path.display());
        let pool = Self::create_pool(&db_url).await?;

        // Run migrations
        Self::run_migrations(&pool).await?;

        info!("Database initialized successfully");
        Ok(Self { pool })
    }

    /// Get the inner SQLx pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get database file path
    pub fn db_path() -> DbResult<PathBuf> {
        let data_dir = dirs::data_dir().ok_or(DbError::NoConfigDir)?;
        Ok(data_dir.join("t-koma").join("db.sqlite3"))
    }

    /// Initialize sqlite-vec extension globally
    ///
    /// Must be called once at application startup, before any database connections
    fn init_sqlite_vec() -> DbResult<()> {
        use rusqlite::ffi::sqlite3_auto_extension;
        use sqlite_vec::sqlite3_vec_init;

        unsafe {
            // Initialize sqlite-vec extension
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

        // Enable WAL mode for better concurrent read performance
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;

        // Improve performance for write-heavy workloads
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;

        // Increase cache size for better performance (64MB)
        sqlx::query("PRAGMA cache_size = -64000")
            .execute(&pool)
            .await?;

        Ok(pool)
    }

    /// Run database migrations
    async fn run_migrations(pool: &SqlitePool) -> DbResult<()> {
        // Load migration SQL
        let migration_sql = include_str!("../migrations/001_initial_schema.sql");

        // Split and execute each statement
        for statement in migration_sql.split(";") {
            let stmt = statement.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await.map_err(|e| {
                    DbError::Migration(format!("Failed to execute migration: {}", e))
                })?;
            }
        }

        info!("Database migrations completed");
        Ok(())
    }

    /// Close the pool gracefully
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Test helpers
#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Create an in-memory database for testing
    pub async fn create_test_pool() -> DbResult<DbPool> {
        // Initialize sqlite-vec
        unsafe {
            use rusqlite::ffi::sqlite3_auto_extension;
            use sqlite_vec::sqlite3_vec_init;
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

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await?;

        // Run migrations
        let migration_sql = include_str!("../migrations/001_initial_schema.sql");
        for statement in migration_sql.split(";") {
            let stmt = statement.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(&pool).await?;
            }
        }

        Ok(DbPool { pool })
    }
}

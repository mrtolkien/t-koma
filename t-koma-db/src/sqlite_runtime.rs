//! Shared SQLite runtime bootstrap helpers for DB pools.

use std::{path::Path, sync::OnceLock};

use libsqlite3_sys::{SQLITE_OK, sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use sqlite_vec::sqlite3_vec_init;

use crate::error::{DbError, DbResult};

static SQLITE_VEC_INIT_RC: OnceLock<i32> = OnceLock::new();

pub(crate) fn init_sqlite_vec_once() -> DbResult<()> {
    let rc = *SQLITE_VEC_INIT_RC.get_or_init(|| unsafe {
        type SqliteVecInitFn = unsafe extern "C" fn(
            *mut sqlite3,
            *mut *const i8,
            *const sqlite3_api_routines,
        ) -> i32;

        sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            SqliteVecInitFn,
        >(sqlite3_vec_init as *const ())))
    });

    if rc == SQLITE_OK {
        Ok(())
    } else {
        Err(DbError::SqliteVec(format!(
            "sqlite3_auto_extension failed with code {rc}"
        )))
    }
}

pub(crate) async fn create_file_pool(db_path: &Path, max_connections: u32) -> DbResult<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true);

    create_pool(options, max_connections).await
}

#[cfg(any(test, feature = "test-helpers"))]
pub(crate) async fn create_in_memory_pool(max_connections: u32) -> DbResult<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(":memory:")
        .foreign_keys(true);

    create_pool(options, max_connections).await
}

async fn create_pool(options: SqliteConnectOptions, max_connections: u32) -> DbResult<SqlitePool> {
    init_sqlite_vec_once()?;

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    apply_common_pragmas(&pool).await?;

    Ok(pool)
}

async fn apply_common_pragmas(pool: &SqlitePool) -> DbResult<()> {
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA cache_size = -64000")
        .execute(pool)
        .await?;

    Ok(())
}

//! Test helpers for T-KOMA/GHOST databases.

use sqlx::sqlite::SqlitePoolOptions;

use crate::{
    error::{DbError, DbResult},
    ghost_db::GhostDbPool,
    koma_db::KomaDbPool,
};

/// Create an in-memory T-KOMA database for testing
pub async fn create_test_koma_pool() -> DbResult<KomaDbPool> {
    init_sqlite_vec()?;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await?;

    sqlx::migrate!("./migrations/koma")
        .run(&pool)
        .await
        .map_err(|e| DbError::Migration(e.to_string()))?;

    Ok(KomaDbPool::from_pool(pool))
}

/// Create an in-memory GHOST database for testing
pub async fn create_test_ghost_pool(ghost_name: &str) -> DbResult<GhostDbPool> {
    init_sqlite_vec()?;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(":memory:")
        .await?;

    sqlx::migrate!("./migrations/ghost")
        .run(&pool)
        .await
        .map_err(|e| DbError::Migration(e.to_string()))?;

    let workspace_path = std::path::PathBuf::from("/tmp/t-koma-test-ghost");
    GhostDbPool::from_pool(pool, ghost_name, workspace_path)
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

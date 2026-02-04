//! Test helpers for T-KOMA/GHOST databases.

use crate::{
    error::{DbError, DbResult},
    ghost_db::GhostDbPool,
    koma_db::KomaDbPool,
    sqlite_runtime::create_in_memory_pool,
};

/// Create an in-memory T-KOMA database for testing
pub async fn create_test_koma_pool() -> DbResult<KomaDbPool> {
    let pool = create_in_memory_pool(1).await?;

    sqlx::migrate!("./migrations/koma")
        .run(&pool)
        .await
        .map_err(|e| DbError::Migration(e.to_string()))?;

    Ok(KomaDbPool::from_pool(pool))
}

/// Create an in-memory GHOST database for testing
pub async fn create_test_ghost_pool(ghost_name: &str) -> DbResult<GhostDbPool> {
    let pool = create_in_memory_pool(1).await?;

    sqlx::migrate!("./migrations/ghost")
        .run(&pool)
        .await
        .map_err(|e| DbError::Migration(e.to_string()))?;

    let workspace_path = std::path::PathBuf::from("/tmp/t-koma-test-ghost");
    GhostDbPool::from_pool(pool, ghost_name, workspace_path)
}

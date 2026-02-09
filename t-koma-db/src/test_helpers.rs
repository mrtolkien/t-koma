//! Test helpers for T-KOMA database.

use crate::{
    error::{DbError, DbResult},
    koma_db::KomaDbPool,
    sqlite_runtime::create_in_memory_pool,
};

/// Create an in-memory T-KOMA database for testing
pub async fn create_test_pool() -> DbResult<KomaDbPool> {
    let pool = create_in_memory_pool(1).await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| DbError::Migration(e.to_string()))?;

    Ok(KomaDbPool::from_pool(pool))
}

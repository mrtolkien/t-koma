use sqlx::SqlitePool;
use t_koma_db::{GhostRepository, OperatorRepository, Platform};

#[tokio::test]
async fn ghost_repository_list_all_and_delete_roundtrip() {
    let pool = SqlitePool::connect(":memory:").await.unwrap();
    sqlx::migrate!("./migrations/koma").run(&pool).await.unwrap();

    let operator = OperatorRepository::create_new(&pool, "Integration Operator", Platform::Api)
        .await
        .unwrap();

    GhostRepository::create(&pool, &operator.id, "IntegrationAlpha")
        .await
        .unwrap();
    GhostRepository::create(&pool, &operator.id, "IntegrationBeta")
        .await
        .unwrap();

    let all = GhostRepository::list_all(&pool).await.unwrap();
    assert_eq!(all.len(), 2);

    GhostRepository::delete_by_name(&pool, "IntegrationAlpha")
        .await
        .unwrap();
    let remaining = GhostRepository::list_all(&pool).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].name, "IntegrationBeta");
}

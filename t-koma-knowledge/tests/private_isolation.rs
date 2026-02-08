use tempfile::TempDir;

use t_koma_knowledge::models::OwnershipScope;
use t_koma_knowledge::storage::{KnowledgeStore, NoteRecord, upsert_note};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};

#[tokio::test]
async fn test_private_note_isolation() {
    let temp = TempDir::new().expect("tempdir");
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();

    // Set T_KOMA_DATA_DIR so paths resolve to our temp dir
    unsafe { std::env::set_var("T_KOMA_DATA_DIR", data_root.to_str().unwrap()) };

    let db_path = data_root.join("shared").join("index.sqlite3");
    let settings = KnowledgeSettings {
        knowledge_db_path_override: Some(db_path.clone()),
        embedding_dim: Some(8),
        ..Default::default()
    };

    let store = KnowledgeStore::open(&db_path, settings.embedding_dim)
        .await
        .unwrap();

    let ghost_a_note = NoteRecord {
        id: "ghost-a-note".to_string(),
        title: "Ghost A Note".to_string(),
        entry_type: "Idea".to_string(),
        archetype: None,
        path: shared_root.join("ghost-a.md"),
        scope: "ghost_note".to_string(),
        owner_ghost: Some("ghost-a".to_string()),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        created_by_ghost: "ghost-a".to_string(),
        created_by_model: "model".to_string(),
        trust_score: 5,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: "hash-a".to_string(),
    };

    let ghost_b_note = NoteRecord {
        id: "ghost-b-note".to_string(),
        title: "Ghost B Note".to_string(),
        entry_type: "Idea".to_string(),
        archetype: None,
        path: shared_root.join("ghost-b.md"),
        scope: "ghost_note".to_string(),
        owner_ghost: Some("ghost-b".to_string()),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        created_by_ghost: "ghost-b".to_string(),
        created_by_model: "model".to_string(),
        trust_score: 5,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: "hash-b".to_string(),
    };

    upsert_note(store.pool(), &ghost_a_note).await.unwrap();
    upsert_note(store.pool(), &ghost_b_note).await.unwrap();

    let engine = KnowledgeEngine::open(settings).await.expect("open engine");

    let own = engine
        .memory_get("ghost-a", "ghost-a-note", OwnershipScope::Private)
        .await
        .expect("ghost a should read own note");
    assert_eq!(own.id, "ghost-a-note");

    let other = engine
        .memory_get("ghost-a", "ghost-b-note", OwnershipScope::Private)
        .await;
    assert!(other.is_err(), "ghost a should not read ghost b note");
}

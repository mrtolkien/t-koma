use tempfile::TempDir;

use t_koma_knowledge::storage::{upsert_note, KnowledgeStore, NoteRecord};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};
use t_koma_knowledge::models::{KnowledgeContext, MemoryScope};

#[tokio::test]
async fn test_private_note_isolation() {
    let temp = TempDir::new().expect("tempdir");
    let shared_root = temp.path().join("knowledge");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();

    let settings = KnowledgeSettings {
        shared_root_override: Some(shared_root.clone()),
        knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
        embedding_dim: Some(8),
        ..Default::default()
    };

    let store = KnowledgeStore::open(
        &settings.knowledge_db_path_override.clone().unwrap(),
        settings.embedding_dim,
    )
    .await
    .unwrap();

    let ghost_a_note = NoteRecord {
        id: "ghost-a-note".to_string(),
        title: "Ghost A Note".to_string(),
        note_type: "Idea".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-a.md"),
        scope: "ghost_private".to_string(),
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
        note_type: "Idea".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-b.md"),
        scope: "ghost_private".to_string(),
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

    let engine = KnowledgeEngine::new(settings);
    let context = KnowledgeContext {
        ghost_name: "ghost-a".to_string(),
        workspace_root: temp.path().join("ghost-a-workspace"),
    };

    let own = engine
        .memory_get(&context, "ghost-a-note", MemoryScope::GhostPrivate)
        .await
        .expect("ghost a should read own note");
    assert_eq!(own.id, "ghost-a-note");

    let other = engine
        .memory_get(&context, "ghost-b-note", MemoryScope::GhostPrivate)
        .await;
    assert!(other.is_err(), "ghost a should not read ghost b note");
}

use tempfile::TempDir;

use t_koma_knowledge::graph::load_links_out;
use t_koma_knowledge::storage::{replace_links, upsert_note, KnowledgeStore, NoteRecord};

#[tokio::test]
async fn test_cross_scope_link_resolution() {
    let temp = TempDir::new().expect("tempdir");
    let shared_root = temp.path().join("knowledge");
    let reference_root = temp.path().join("reference");
    let ghost_root = temp.path().join("ghost");
    let ghost_private = ghost_root.join("private_knowledge");

    tokio::fs::create_dir_all(&shared_root).await.unwrap();
    tokio::fs::create_dir_all(&reference_root).await.unwrap();
    tokio::fs::create_dir_all(&ghost_private).await.unwrap();

    let shared_note = shared_root.join("shared-note.md");
    let ghost_note = ghost_private.join("ghost-note.md");

    tokio::fs::write(
        &shared_note,
        "+++
+id = \"shared-1\"
+title = \"Shared Note\"
+type = \"Concept\"
+created_at = \"2025-01-01T00:00:00Z\"
+trust_score = 8
+[created_by]
+ghost = \"system\"
+model = \"system\"
++++
+
+Shared content.\n",
    )
    .await
    .unwrap();

    tokio::fs::write(
        &ghost_note,
        "+++
+id = \"ghost-1\"
+title = \"Ghost Note\"
+type = \"Idea\"
+created_at = \"2025-01-01T00:00:00Z\"
+trust_score = 5
+[created_by]
+ghost = \"ghost\"
+model = \"model\"
++++
+
+See [[Shared Note]].\n",
    )
    .await
    .unwrap();

    let settings = t_koma_knowledge::KnowledgeSettings {
        shared_root_override: Some(shared_root.clone()),
        reference_root_override: Some(reference_root.clone()),
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

    let shared_note_record = NoteRecord {
        id: "shared-1".to_string(),
        title: "Shared Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: shared_note.clone(),
        scope: "shared".to_string(),
        owner_ghost: None,
        created_at: "2025-01-01T00:00:00Z".to_string(),
        created_by_ghost: "system".to_string(),
        created_by_model: "system".to_string(),
        trust_score: 8,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: "shared-hash".to_string(),
    };
    upsert_note(store.pool(), &shared_note_record)
        .await
        .unwrap();

    let ghost_note_record = NoteRecord {
        id: "ghost-1".to_string(),
        title: "Ghost Note".to_string(),
        note_type: "Idea".to_string(),
        type_valid: true,
        path: ghost_note.clone(),
        scope: "ghost_private".to_string(),
        owner_ghost: Some("ghost".to_string()),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        created_by_ghost: "ghost".to_string(),
        created_by_model: "model".to_string(),
        trust_score: 5,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: "ghost-hash".to_string(),
    };
    upsert_note(store.pool(), &ghost_note_record)
        .await
        .unwrap();

    replace_links(
        store.pool(),
        "ghost-1",
        Some("ghost"),
        &[("Shared Note".to_string(), None)],
    )
    .await
    .unwrap();

    let links = load_links_out(
        store.pool(),
        "ghost-1",
        10,
        t_koma_knowledge::models::KnowledgeScope::GhostPrivate,
        "ghost",
    )
        .await
        .unwrap();
    assert!(links.iter().any(|link| link.title == "Shared Note"));
    assert!(links
        .iter()
        .any(|link| matches!(link.scope, t_koma_knowledge::models::KnowledgeScope::Shared)));
}

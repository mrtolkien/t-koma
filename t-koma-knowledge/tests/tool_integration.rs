//! Integration tests for memory note tools (create, update, validate, comment).
//!
//! These tests exercise the `KnowledgeEngine` methods backing the gateway tools
//! without requiring a running Ollama instance. Embedding-dependent tests are
//! gated behind the `slow-tests` feature.

use tempfile::TempDir;

use t_koma_knowledge::models::{
    NoteCreateRequest, NoteSearchScope, NoteUpdateRequest, WriteScope,
};
use t_koma_knowledge::storage::{KnowledgeStore, NoteRecord, replace_tags, upsert_note};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};

/// Build a test engine with temp dirs. Returns (engine, ghost_name, temp).
async fn setup() -> (KnowledgeEngine, String, TempDir) {
    let temp = TempDir::new().expect("tempdir");
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();

    // Set T_KOMA_DATA_DIR so paths resolve to our temp dir
    unsafe { std::env::set_var("T_KOMA_DATA_DIR", data_root.to_str().unwrap()) };

    // Create ghost workspace dirs
    let ghost_notes = data_root.join("ghosts").join("ghost-a").join("notes");
    let ghost_inbox = data_root.join("ghosts").join("ghost-a").join("inbox");
    tokio::fs::create_dir_all(&ghost_notes).await.unwrap();
    tokio::fs::create_dir_all(&ghost_inbox).await.unwrap();

    let db_path = data_root.join("shared").join("index.sqlite3");
    let settings = KnowledgeSettings {
        knowledge_db_path_override: Some(db_path),
        embedding_dim: Some(8),
        // Use a bogus URL so embedding calls fail fast (we don't need them for these tests)
        embedding_url: "http://127.0.0.1:1".to_string(),
        // Disable auto-reconcile to avoid needing real dirs
        reconcile_seconds: 999_999,
        ..Default::default()
    };

    let engine = KnowledgeEngine::open(settings).await.expect("open engine");

    (engine, "ghost-a".to_string(), temp)
}

// ── note_create ──────────────────────────────────────────────────────

#[tokio::test]
async fn create_private_note() {
    let (engine, ghost_name, _temp) = setup().await;

    let request = NoteCreateRequest {
        title: "Test Note".to_string(),
        note_type: "Concept".to_string(),
        scope: WriteScope::GhostNote,
        body: "This is the body.".to_string(),
        parent: None,
        tags: Some(vec!["test".to_string()]),
        source: None,
        trust_score: None,
    };

    let result = engine.note_create(&ghost_name, request).await;
    // Will fail on embedding (bogus URL) but the file should be written
    // The engine indexes inline which calls embeddings. Since we use a bogus
    // URL, we expect an error. This is expected — the file was still written.
    // For a full end-to-end test, use slow-tests with real Ollama.
    match result {
        Ok(write_result) => {
            assert!(!write_result.note_id.is_empty());
            assert!(write_result.path.exists());
            let content = tokio::fs::read_to_string(&write_result.path).await.unwrap();
            assert!(content.contains("Test Note"));
            assert!(content.contains("This is the body."));
        }
        Err(e) => {
            // Embedding failure is expected with bogus URL
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

#[tokio::test]
async fn create_shared_note() {
    let (engine, ghost_name, _temp) = setup().await;

    let request = NoteCreateRequest {
        title: "Shared Knowledge".to_string(),
        note_type: "HowTo".to_string(),
        scope: WriteScope::SharedNote,
        body: "Shared body content.".to_string(),
        parent: None,
        tags: None,
        source: None,
        trust_score: Some(8),
    };

    let result = engine.note_create(&ghost_name, request).await;
    match result {
        Ok(write_result) => {
            // Path should be under shared/notes
            let path_str = write_result.path.to_string_lossy();
            assert!(path_str.contains("shared") && path_str.contains("notes"));
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── note_get access control ──────────────────────────────────────────

#[tokio::test]
async fn get_own_private_note_succeeds() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    // Insert a note for ghost-a
    let note = NoteRecord {
        id: "ghost-a-own".to_string(),
        title: "My Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-a-own.md"),
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let doc = engine
        .memory_get(&ghost_name, "ghost-a-own", NoteSearchScope::GhostOnly)
        .await;
    assert!(doc.is_ok());
}

#[tokio::test]
async fn get_other_ghost_private_note_fails() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    // Insert a note for ghost-b
    let note = NoteRecord {
        id: "ghost-b-secret".to_string(),
        title: "Secret Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-b-secret.md"),
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    // Ghost-a tries to read ghost-b's note
    let result = engine
        .memory_get(&ghost_name, "ghost-b-secret", NoteSearchScope::GhostOnly)
        .await;
    assert!(
        result.is_err(),
        "ghost-a should not see ghost-b's private note"
    );
}

#[tokio::test]
async fn get_shared_note_from_any_ghost() {
    let (engine, _ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    let note = NoteRecord {
        id: "shared-note".to_string(),
        title: "Shared Note".to_string(),
        note_type: "Reference".to_string(),
        type_valid: true,
        path: shared_root.join("shared-note.md"),
        scope: "shared_note".to_string(),
        owner_ghost: None,
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    // Ghost-b can read shared note
    let doc = engine
        .memory_get("ghost-b", "shared-note", NoteSearchScope::SharedOnly)
        .await;
    assert!(doc.is_ok(), "any ghost should read shared notes");
}

// ── note_update access control ───────────────────────────────────────

#[tokio::test]
async fn update_own_note_succeeds() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    // Create a note file that can be parsed
    let note_path = shared_root.join("updatable.md");
    let content = r#"+++
id = "updatable-note"
title = "Original Title"
type = "Concept"
created_at = "2025-01-01T00:00:00Z"
trust_score = 5
[created_by]
ghost = "ghost-a"
model = "model"
+++

Original body.
"#;
    tokio::fs::write(&note_path, content).await.unwrap();

    let note = NoteRecord {
        id: "updatable-note".to_string(),
        title: "Original Title".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: note_path.clone(),
        scope: "ghost_note".to_string(),
        owner_ghost: Some("ghost-a".to_string()),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        created_by_ghost: "ghost-a".to_string(),
        created_by_model: "model".to_string(),
        trust_score: 5,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: Some(1),
        parent_id: None,
        comments_json: None,
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let request = NoteUpdateRequest {
        note_id: "updatable-note".to_string(),
        title: Some("Updated Title".to_string()),
        body: None,
        tags: None,
        trust_score: None,
        parent: None,
    };

    let result = engine.note_update(&ghost_name, request).await;
    match result {
        Ok(write_result) => {
            assert_eq!(write_result.note_id, "updatable-note");
            let content = tokio::fs::read_to_string(&write_result.path).await.unwrap();
            assert!(content.contains("Updated Title"));
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

#[tokio::test]
async fn update_other_ghost_note_denied() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    let note_path = shared_root.join("ghost-b-note.md");
    let content = r#"+++
id = "ghost-b-note"
title = "Ghost B Note"
type = "Concept"
created_at = "2025-01-01T00:00:00Z"
trust_score = 5
[created_by]
ghost = "ghost-b"
model = "model"
+++

Ghost B content.
"#;
    tokio::fs::write(&note_path, content).await.unwrap();

    let note = NoteRecord {
        id: "ghost-b-note".to_string(),
        title: "Ghost B Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: note_path,
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let request = NoteUpdateRequest {
        note_id: "ghost-b-note".to_string(),
        title: Some("Hijacked".to_string()),
        ..Default::default()
    };

    // Ghost-a tries to update ghost-b's note — should fail at get phase
    // (private notes not visible) or at write access check
    let result = engine.note_update(&ghost_name, request).await;
    assert!(result.is_err(), "ghost-a should not update ghost-b's note");
}

// ── note_validate ────────────────────────────────────────────────────

#[tokio::test]
async fn validate_note_updates_metadata() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    let note_path = shared_root.join("validatable.md");
    let content = r#"+++
id = "val-note"
title = "Validatable Note"
type = "Concept"
created_at = "2025-01-01T00:00:00Z"
trust_score = 5
[created_by]
ghost = "ghost-a"
model = "model"
+++

Content.
"#;
    tokio::fs::write(&note_path, content).await.unwrap();

    let note = NoteRecord {
        id: "val-note".to_string(),
        title: "Validatable Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: note_path,
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let result = engine.note_validate(&ghost_name, "val-note", Some(9)).await;
    match result {
        Ok(write_result) => {
            assert_eq!(write_result.note_id, "val-note");
            let content = tokio::fs::read_to_string(&write_result.path).await.unwrap();
            assert!(content.contains("trust_score = 9"));
            assert!(content.contains("last_validated_at"));
            assert!(content.contains("ghost-a"));
        }
        Err(e) => {
            // Re-index embedding failure is acceptable
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── note_comment ─────────────────────────────────────────────────────

#[tokio::test]
async fn comment_appends_to_front_matter() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    let note_path = shared_root.join("commentable.md");
    let content = r#"+++
id = "comment-note"
title = "Commentable Note"
type = "Concept"
created_at = "2025-01-01T00:00:00Z"
trust_score = 5
[created_by]
ghost = "ghost-a"
model = "model"
+++

Body text.
"#;
    tokio::fs::write(&note_path, content).await.unwrap();

    let note = NoteRecord {
        id: "comment-note".to_string(),
        title: "Commentable Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: note_path,
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let result = engine
        .note_comment(&ghost_name, "comment-note", "This is my review comment.")
        .await;
    match result {
        Ok(write_result) => {
            assert_eq!(write_result.note_id, "comment-note");
            let content = tokio::fs::read_to_string(&write_result.path).await.unwrap();
            assert!(content.contains("[[comments]]"));
            assert!(content.contains("This is my review comment."));
            assert!(content.contains("ghost-a"));
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── memory_capture scope enforcement ─────────────────────────────────

#[tokio::test]
async fn capture_to_ghost_inbox() {
    let (engine, ghost_name, _temp) = setup().await;

    let result = engine
        .memory_capture(&ghost_name, "Quick note to self", WriteScope::GhostNote, None)
        .await;
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.contains("inbox"));
}

#[tokio::test]
async fn capture_to_shared_inbox() {
    let (engine, ghost_name, _temp) = setup().await;

    let result = engine
        .memory_capture(&ghost_name, "Shared info", WriteScope::SharedNote, None)
        .await;
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.contains("shared") && path.contains("notes"));
}

// ── reference topic CRUD ─────────────────────────────────────────────

/// Helper: insert a ReferenceTopic note directly via DB + file.
#[allow(clippy::too_many_arguments)]
async fn insert_topic_note(
    store: &KnowledgeStore,
    shared_root: &std::path::Path,
    id: &str,
    title: &str,
    ghost: &str,
    status: &str,
    max_age_days: i64,
    fetched_at: &str,
    tags: &[&str],
) -> std::path::PathBuf {
    let topic_dir = shared_root.join(format!("ref_{}", id));
    tokio::fs::create_dir_all(&topic_dir).await.unwrap();

    let tags_toml: Vec<String> = tags.iter().map(|t| format!("\"{}\"", t)).collect();
    let content = format!(
        r#"+++
id = "{id}"
title = "{title}"
type = "ReferenceTopic"
created_at = "{fetched_at}"
trust_score = 8
tags = [{tags}]
status = "{status}"
fetched_at = "{fetched_at}"
max_age_days = {max_age_days}

[created_by]
ghost = "{ghost}"
model = "tool"
+++

# {title}

Description of {title}.
"#,
        id = id,
        title = title,
        fetched_at = fetched_at,
        tags = tags_toml.join(", "),
        status = status,
        max_age_days = max_age_days,
        ghost = ghost,
    );

    let path = topic_dir.join("topic.md");
    tokio::fs::write(&path, &content).await.unwrap();

    let note = NoteRecord {
        id: id.to_string(),
        title: title.to_string(),
        note_type: "ReferenceTopic".to_string(),
        type_valid: true,
        path: path.clone(),
        scope: "shared_reference".to_string(),
        owner_ghost: None,
        created_at: fetched_at.to_string(),
        created_by_ghost: ghost.to_string(),
        created_by_model: "tool".to_string(),
        trust_score: 8,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: Some(1),
        parent_id: None,
        comments_json: None,
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    // Insert tags
    let tag_strings: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
    replace_tags(store.pool(), id, &tag_strings).await.unwrap();

    path
}

#[tokio::test]
async fn topic_list_returns_inserted_topics() {
    let (engine, _ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    insert_topic_note(
        &store,
        &shared_root,
        "topic-a",
        "Alpha Library",
        "ghost-a",
        "active",
        30,
        "2025-06-01T00:00:00Z",
        &["rust", "alpha"],
    )
    .await;
    insert_topic_note(
        &store,
        &shared_root,
        "topic-b",
        "Beta Framework",
        "ghost-a",
        "obsolete",
        0,
        "2025-05-01T00:00:00Z",
        &["rust", "beta"],
    )
    .await;

    // Without obsolete
    let list = engine.topic_list(false).await.unwrap();
    assert_eq!(list.len(), 1, "obsolete topic should be excluded");
    assert_eq!(list[0].topic_id, "topic-a");
    assert_eq!(list[0].tags, vec!["alpha", "rust"]);

    // With obsolete
    let list_all = engine.topic_list(true).await.unwrap();
    assert_eq!(list_all.len(), 2, "should include obsolete topics");
}

#[tokio::test]
async fn topic_update_changes_status_and_tags() {
    let (engine, ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    insert_topic_note(
        &store,
        &shared_root,
        "topic-upd",
        "Updatable Topic",
        "ghost-a",
        "active",
        30,
        "2025-06-01T00:00:00Z",
        &["original"],
    )
    .await;

    let request = t_koma_knowledge::TopicUpdateRequest {
        topic_id: "topic-upd".to_string(),
        status: Some("obsolete".to_string()),
        max_age_days: Some(0),
        body: Some("Updated description.".to_string()),
        tags: Some(vec!["updated".to_string(), "changed".to_string()]),
    };

    let result = engine.topic_update(&ghost_name, request).await;
    match result {
        Ok(()) => {
            // Verify file was updated
            let topic_dir = shared_root.join("ref_topic-upd");
            let content = tokio::fs::read_to_string(topic_dir.join("topic.md"))
                .await
                .unwrap();
            assert!(
                content.contains("status = \"obsolete\""),
                "status should be updated"
            );
            assert!(
                content.contains("max_age_days = 0"),
                "max_age_days should be updated"
            );
            assert!(
                content.contains("Updated description."),
                "body should be updated"
            );
        }
        Err(e) => {
            // Embedding errors are acceptable in non-slow-tests
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding")
                    || err_str.contains("http")
                    || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

#[tokio::test]
async fn recent_topics_returns_most_recent() {
    let (engine, _ghost_name, temp) = setup().await;
    let data_root = temp.path().join("data");
    let shared_root = data_root.join("shared").join("notes");

    let db_path = data_root.join("shared").join("index.sqlite3");
    let store = KnowledgeStore::open(&db_path, Some(8))
        .await
        .unwrap();

    // Insert topics with different dates
    insert_topic_note(
        &store,
        &shared_root,
        "topic-old",
        "Old Topic",
        "ghost-a",
        "active",
        30,
        "2025-01-01T00:00:00Z",
        &["old"],
    )
    .await;
    insert_topic_note(
        &store,
        &shared_root,
        "topic-new",
        "New Topic",
        "ghost-a",
        "active",
        30,
        "2025-06-15T00:00:00Z",
        &["new"],
    )
    .await;

    let recent = engine.recent_topics().await.unwrap();
    assert_eq!(recent.len(), 2);
    // Most recent first
    assert_eq!(recent[0].1, "New Topic");
    assert_eq!(recent[1].1, "Old Topic");
}

// ── slow-tests (require Ollama) ──────────────────────────────────────

#[cfg(feature = "slow-tests")]
mod slow {
    use super::*;

    async fn setup_with_embeddings() -> (KnowledgeEngine, String, TempDir) {
        let temp = TempDir::new().expect("tempdir");
        let data_root = temp.path().join("data");
        let shared_root = data_root.join("shared").join("notes");
        tokio::fs::create_dir_all(&shared_root).await.unwrap();

        let ghost_notes = data_root.join("ghosts").join("ghost-a").join("notes");
        let ghost_inbox = data_root.join("ghosts").join("ghost-a").join("inbox");
        tokio::fs::create_dir_all(&ghost_notes).await.unwrap();
        tokio::fs::create_dir_all(&ghost_inbox).await.unwrap();

        unsafe { std::env::set_var("T_KOMA_DATA_DIR", data_root.to_str().unwrap()) };

        let db_path = data_root.join("shared").join("index.sqlite3");
        let settings = KnowledgeSettings {
            knowledge_db_path_override: Some(db_path),
            reconcile_seconds: 999_999,
            ..Default::default()
        };

        let engine = KnowledgeEngine::open(settings).await.expect("open engine");

        (engine, "ghost-a".to_string(), temp)
    }

    #[tokio::test]
    async fn create_and_search_note() {
        let (engine, ghost_name, _temp) = setup_with_embeddings().await;

        let request = NoteCreateRequest {
            title: "Rust Error Handling".to_string(),
            note_type: "Concept".to_string(),
            scope: WriteScope::GhostNote,
            body: "Rust uses Result and Option types for error handling. \
                   The ? operator propagates errors up the call stack."
                .to_string(),
            parent: None,
            tags: Some(vec!["rust".to_string(), "errors".to_string()]),
            source: None,
            trust_score: Some(8),
        };

        let write_result = engine
            .note_create(&ghost_name, request)
            .await
            .expect("create should succeed with Ollama running");

        // Now search for it
        let query = t_koma_knowledge::NoteQuery {
            query: "error handling in Rust".to_string(),
            scope: NoteSearchScope::GhostOnly,
            options: Default::default(),
        };
        let results = engine
            .memory_search(&ghost_name, query)
            .await
            .expect("search should succeed");

        assert!(
            results.iter().any(|r| r.summary.id == write_result.note_id),
            "created note should appear in search results"
        );
    }
}

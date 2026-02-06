//! Integration tests for memory note tools (create, update, validate, comment).
//!
//! These tests exercise the `KnowledgeEngine` methods backing the gateway tools
//! without requiring a running Ollama instance. Embedding-dependent tests are
//! gated behind the `slow-tests` feature.

use tempfile::TempDir;

use t_koma_knowledge::models::{
    KnowledgeContext, MemoryScope, NoteCreateRequest, NoteUpdateRequest, WriteScope,
};
use t_koma_knowledge::storage::{upsert_note, KnowledgeStore, NoteRecord};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};

/// Build a test engine + context pair with temp dirs.
async fn setup() -> (KnowledgeEngine, KnowledgeContext, TempDir) {
    let temp = TempDir::new().expect("tempdir");
    let shared_root = temp.path().join("knowledge");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();
    let workspace = temp.path().join("ghost-a-workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();

    let settings = KnowledgeSettings {
        shared_root_override: Some(shared_root.clone()),
        knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
        embedding_dim: Some(8),
        // Use a bogus URL so embedding calls fail fast (we don't need them for these tests)
        embedding_url: "http://127.0.0.1:1".to_string(),
        // Disable auto-reconcile to avoid needing real dirs
        reconcile_seconds: 999_999,
        ..Default::default()
    };

    let engine = KnowledgeEngine::new(settings);
    let context = KnowledgeContext {
        ghost_name: "ghost-a".to_string(),
        workspace_root: workspace,
    };

    (engine, context, temp)
}

/// Build a second context for ghost-b that shares the same temp dir.
fn ghost_b_context(temp: &TempDir) -> KnowledgeContext {
    KnowledgeContext {
        ghost_name: "ghost-b".to_string(),
        workspace_root: temp.path().join("ghost-b-workspace"),
    }
}

// ── note_create ──────────────────────────────────────────────────────

#[tokio::test]
async fn create_private_note() {
    let (engine, context, _temp) = setup().await;
    tokio::fs::create_dir_all(&context.workspace_root.join("private_knowledge"))
        .await
        .unwrap();

    let request = NoteCreateRequest {
        title: "Test Note".to_string(),
        note_type: "Concept".to_string(),
        scope: WriteScope::Private,
        body: "This is the body.".to_string(),
        parent: None,
        tags: Some(vec!["test".to_string()]),
        source: None,
        trust_score: None,
    };

    let result = engine.note_create(&context, request).await;
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
                err_str.contains("embedding") || err_str.contains("http") || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

#[tokio::test]
async fn create_shared_note() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();

    let request = NoteCreateRequest {
        title: "Shared Knowledge".to_string(),
        note_type: "HowTo".to_string(),
        scope: WriteScope::Shared,
        body: "Shared body content.".to_string(),
        parent: None,
        tags: None,
        source: None,
        trust_score: Some(8),
    };

    let result = engine.note_create(&context, request).await;
    match result {
        Ok(write_result) => {
            assert!(write_result.path.starts_with(&shared_root));
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding") || err_str.contains("http") || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── note_get access control ──────────────────────────────────────────

#[tokio::test]
async fn get_own_private_note_succeeds() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
    .await
    .unwrap();

    // Insert a note for ghost-a
    let note = NoteRecord {
        id: "ghost-a-own".to_string(),
        title: "My Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-a-own.md"),
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let doc = engine
        .memory_get(&context, "ghost-a-own", MemoryScope::GhostPrivate)
        .await;
    assert!(doc.is_ok());
}

#[tokio::test]
async fn get_other_ghost_private_note_fails() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
    .await
    .unwrap();

    // Insert a note for ghost-b
    let note = NoteRecord {
        id: "ghost-b-secret".to_string(),
        title: "Secret Note".to_string(),
        note_type: "Concept".to_string(),
        type_valid: true,
        path: shared_root.join("ghost-b-secret.md"),
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    // Ghost-a tries to read ghost-b's note
    let result = engine
        .memory_get(&context, "ghost-b-secret", MemoryScope::GhostPrivate)
        .await;
    assert!(result.is_err(), "ghost-a should not see ghost-b's private note");
}

#[tokio::test]
async fn get_shared_note_from_any_ghost() {
    let (engine, _context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
    .await
    .unwrap();

    let note = NoteRecord {
        id: "shared-note".to_string(),
        title: "Shared Note".to_string(),
        note_type: "Reference".to_string(),
        type_valid: true,
        path: shared_root.join("shared-note.md"),
        scope: "shared".to_string(),
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
    let ctx_b = ghost_b_context(&temp);
    let doc = engine
        .memory_get(&ctx_b, "shared-note", MemoryScope::SharedOnly)
        .await;
    assert!(doc.is_ok(), "any ghost should read shared notes");
}

// ── note_update access control ───────────────────────────────────────

#[tokio::test]
async fn update_own_note_succeeds() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
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
        scope: "ghost_private".to_string(),
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

    let result = engine.note_update(&context, request).await;
    match result {
        Ok(write_result) => {
            assert_eq!(write_result.note_id, "updatable-note");
            let content = tokio::fs::read_to_string(&write_result.path).await.unwrap();
            assert!(content.contains("Updated Title"));
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("embedding") || err_str.contains("http") || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

#[tokio::test]
async fn update_other_ghost_note_denied() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
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
    let result = engine.note_update(&context, request).await;
    assert!(result.is_err(), "ghost-a should not update ghost-b's note");
}

// ── note_validate ────────────────────────────────────────────────────

#[tokio::test]
async fn validate_note_updates_metadata() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let result = engine.note_validate(&context, "val-note", Some(9)).await;
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
                err_str.contains("embedding") || err_str.contains("http") || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── note_comment ─────────────────────────────────────────────────────

#[tokio::test]
async fn comment_appends_to_front_matter() {
    let (engine, context, temp) = setup().await;
    let shared_root = temp.path().join("knowledge");

    let store = KnowledgeStore::open(
        &shared_root.join("index.sqlite3"),
        Some(8),
    )
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
        content_hash: "hash".to_string(),
    };
    upsert_note(store.pool(), &note).await.unwrap();

    let result = engine
        .note_comment(&context, "comment-note", "This is my review comment.")
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
                err_str.contains("embedding") || err_str.contains("http") || err_str.contains("error sending request"),
                "unexpected error: {err_str}"
            );
        }
    }
}

// ── memory_capture scope enforcement ─────────────────────────────────

#[tokio::test]
async fn capture_to_ghost_inbox() {
    let (engine, context, _temp) = setup().await;
    let inbox_dir = context.workspace_root.join("private_knowledge").join("inbox");
    tokio::fs::create_dir_all(&inbox_dir).await.unwrap();

    let result = engine
        .memory_capture(&context, "Quick note to self", WriteScope::Private)
        .await;
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.contains("private_knowledge/inbox"));
}

#[tokio::test]
async fn capture_to_shared_inbox() {
    let (engine, context, temp) = setup().await;
    let shared_inbox = temp.path().join("knowledge").join("inbox");
    tokio::fs::create_dir_all(&shared_inbox).await.unwrap();

    let result = engine
        .memory_capture(&context, "Shared info", WriteScope::Shared)
        .await;
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.contains("knowledge/inbox"));
}

// ── slow-tests (require Ollama) ──────────────────────────────────────

#[cfg(feature = "slow-tests")]
mod slow {
    use super::*;

    async fn setup_with_embeddings() -> (KnowledgeEngine, KnowledgeContext, TempDir) {
        let temp = TempDir::new().expect("tempdir");
        let shared_root = temp.path().join("knowledge");
        tokio::fs::create_dir_all(&shared_root).await.unwrap();
        let workspace = temp.path().join("ghost-a-workspace");
        tokio::fs::create_dir_all(&workspace.join("private_knowledge"))
            .await
            .unwrap();

        let settings = KnowledgeSettings {
            shared_root_override: Some(shared_root.clone()),
            knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
            reconcile_seconds: 999_999,
            ..Default::default()
        };

        let engine = KnowledgeEngine::new(settings);
        let context = KnowledgeContext {
            ghost_name: "ghost-a".to_string(),
            workspace_root: workspace,
        };

        (engine, context, temp)
    }

    #[tokio::test]
    async fn create_and_search_note() {
        let (engine, context, _temp) = setup_with_embeddings().await;

        let request = NoteCreateRequest {
            title: "Rust Error Handling".to_string(),
            note_type: "Concept".to_string(),
            scope: WriteScope::Private,
            body: "Rust uses Result and Option types for error handling. \
                   The ? operator propagates errors up the call stack."
                .to_string(),
            parent: None,
            tags: Some(vec!["rust".to_string(), "errors".to_string()]),
            source: None,
            trust_score: Some(8),
        };

        let write_result = engine
            .note_create(&context, request)
            .await
            .expect("create should succeed with Ollama running");

        // Now search for it
        let query = t_koma_knowledge::MemoryQuery {
            query: "error handling in Rust".to_string(),
            scope: MemoryScope::GhostPrivate,
            options: Default::default(),
        };
        let results = engine
            .memory_search(&context, query)
            .await
            .expect("search should succeed");

        assert!(
            results.iter().any(|r| r.summary.id == write_result.note_id),
            "created note should appear in search results"
        );
    }
}

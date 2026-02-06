//! End-to-end integration test for the reference topic pipeline.
//!
//! This test exercises the full ghost reference workflow using a **single shared
//! clone** of the Dioxus repo + a web page fetch.  The expensive setup
//! (clone → chunk → embed → index) runs once; all assertions share the result.
//!
//! Pipeline steps validated:
//! 1. `topic_approval_summary` — gathers repo metadata via `gh api`
//! 2. `topic_create` — clones a real repo, fetches a web page, indexes everything
//! 3. Per-file indexing — every indexed file must have ≥ 1 chunk + embedding
//! 4. `reference_search` — hybrid BM25 + dense search over indexed content
//! 5. `topic_search` — semantic search over topic descriptions
//! 6. `topic_list` / `recent_topics` — listing and prompt injection
//!
//! Requires:
//! - `gh` CLI authenticated (for GitHub clone)
//! - Ollama running with `qwen3-embedding:8b` model
//! - Network access to github.com and dioxuslabs.com
//!
//! Run with:
//!   cargo test -p t-koma-knowledge --features slow-tests reference_topic_e2e
//!
//! For initial snapshot creation, use:
//!   cargo insta test -p t-koma-knowledge --features slow-tests -- reference_topic_e2e

#![cfg(feature = "slow-tests")]

use insta::assert_yaml_snapshot;
use serde::Serialize;
use sqlx::SqlitePool;
use tempfile::TempDir;

use t_koma_knowledge::models::{KnowledgeContext, ReferenceQuery};
use t_koma_knowledge::{
    KnowledgeEngine, KnowledgeSettings, MemoryResult, TopicCreateRequest, TopicSourceInput,
};

// ── Fixture ─────────────────────────────────────────────────────────

struct DioxusFixture {
    engine: KnowledgeEngine,
    context: KnowledgeContext,
    topic_id: String,
    source_count: usize,
    file_count: usize,
    chunk_count: usize,
    per_file_chunks: Vec<FileChunkInfo>,
    _temp: TempDir,
}

#[derive(Debug, Clone, Serialize)]
struct FileChunkInfo {
    file_path: String,
    chunk_count: usize,
}

impl DioxusFixture {
    /// Create a reference topic from the real Dioxus repo (filtered to docs/examples)
    /// and a web page.  Exercises the full pipeline: clone → chunk → embed → index.
    async fn setup() -> Self {
        // Enable tracing so source-fetch warnings are visible with --nocapture
        let _ = tracing_subscriber::fmt()
            .with_env_filter("t_koma_knowledge=debug,warn")
            .with_test_writer()
            .try_init();

        let temp = TempDir::new().expect("tempdir");
        let shared_root = temp.path().join("knowledge");
        let reference_root = temp.path().join("reference");
        let workspace = temp.path().join("ghost-test-workspace");

        tokio::fs::create_dir_all(&shared_root).await.unwrap();
        tokio::fs::create_dir_all(&reference_root).await.unwrap();
        tokio::fs::create_dir_all(&workspace).await.unwrap();

        let settings = KnowledgeSettings {
            shared_root_override: Some(shared_root.clone()),
            reference_root_override: Some(reference_root),
            knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
            reconcile_seconds: 999_999,
            ..Default::default()
        };

        let engine = KnowledgeEngine::open(settings).await.expect("open engine");
        let context = KnowledgeContext {
            ghost_name: "ghost-researcher".to_string(),
            workspace_root: workspace,
        };

        // Create topic from Dioxus — use filtered paths to keep test fast
        let request = TopicCreateRequest {
            title: "Dioxus - Rust UI Framework".to_string(),
            body: "Dioxus is a portable, performant, and ergonomic framework for building \
                   cross-platform user interfaces in Rust. It uses a virtual DOM with RSX \
                   syntax similar to React's JSX. Key concepts include components as functions \
                   returning Element, state management via use_signal hooks, and a router for \
                   multi-page apps."
                .to_string(),
            sources: vec![
                TopicSourceInput {
                    source_type: "git".to_string(),
                    url: "https://github.com/DioxusLabs/dioxus".to_string(),
                    ref_name: Some("main".to_string()),
                    paths: Some(vec![
                        "README.md".to_string(),
                        "examples/".to_string(),
                    ]),
                    role: None,
                },
                TopicSourceInput {
                    source_type: "web".to_string(),
                    url: "https://dioxuslabs.com/learn/0.6/".to_string(),
                    ref_name: None,
                    paths: None,
                    role: None,
                },
            ],
            tags: Some(vec![
                "rust".to_string(),
                "ui".to_string(),
                "framework".to_string(),
                "dioxus".to_string(),
            ]),
            max_age_days: Some(30),
            trust_score: Some(8),
        };

        let result = engine
            .topic_create(&context, request)
            .await
            .expect("topic_create should succeed");

        assert!(
            result.source_count >= 1,
            "at least one source should succeed (got {})",
            result.source_count
        );
        assert!(
            result.file_count > 0,
            "should have fetched files (got {})",
            result.file_count
        );
        assert!(
            result.chunk_count > 0,
            "should have produced chunks (got {})",
            result.chunk_count
        );

        // Query per-file chunk counts from the DB to validate every file was indexed
        let per_file_chunks =
            query_per_file_chunks(engine.pool(), &result.topic_id).await;

        // CRITICAL: every file in reference_files must have at least 1 chunk
        for info in &per_file_chunks {
            assert!(
                info.chunk_count > 0,
                "file '{}' has 0 chunks — indexing pipeline is broken for this file",
                info.file_path,
            );
        }

        // The number of files in the DB must match what topic_create reported
        assert_eq!(
            per_file_chunks.len(),
            result.file_count,
            "reference_files table should have exactly file_count entries \
             (DB has {}, topic_create reported {})",
            per_file_chunks.len(),
            result.file_count,
        );

        Self {
            engine,
            context,
            topic_id: result.topic_id,
            source_count: result.source_count,
            file_count: result.file_count,
            chunk_count: result.chunk_count,
            per_file_chunks,
            _temp: temp,
        }
    }
}

/// Query the DB for per-file chunk counts within a topic.
async fn query_per_file_chunks(pool: &SqlitePool, topic_id: &str) -> Vec<FileChunkInfo> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"SELECT rf.path, COUNT(c.id)
           FROM reference_files rf
           JOIN chunks c ON c.note_id = rf.note_id
           WHERE rf.topic_id = ?
           GROUP BY rf.path
           ORDER BY rf.path"#,
    )
    .bind(topic_id)
    .fetch_all(pool)
    .await
    .expect("query per-file chunks");

    rows.into_iter()
        .map(|(path, count)| FileChunkInfo {
            file_path: path,
            chunk_count: count as usize,
        })
        .collect()
}

// ── Snapshot helpers ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SearchResultSnapshot {
    query: String,
    total_hits: usize,
    unique_files: usize,
    top_results: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    rank: usize,
    title: String,
    note_type: String,
    snippet_preview: String,
}

fn build_search_snapshot(
    query: &str,
    results: &[MemoryResult],
    n: usize,
) -> SearchResultSnapshot {
    let unique_files: std::collections::HashSet<&str> = results
        .iter()
        .map(|r| r.summary.title.as_str())
        .collect();

    SearchResultSnapshot {
        query: query.to_string(),
        total_hits: results.len(),
        unique_files: unique_files.len(),
        top_results: results
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, r)| SearchHit {
                rank: i + 1,
                title: r.summary.title.clone(),
                note_type: r.summary.note_type.clone(),
                snippet_preview: r.summary.snippet.chars().take(150).collect(),
            })
            .collect(),
    }
}

// ── Test cases ──────────────────────────────────────────────────────

/// Phase 1: Verify topic_approval_summary gathers metadata from GitHub.
/// (Standalone — no clone needed.)
#[tokio::test]
async fn dioxus_approval_summary() {
    let request = TopicCreateRequest {
        title: "Dioxus".to_string(),
        body: "test".to_string(),
        sources: vec![TopicSourceInput {
            source_type: "git".to_string(),
            url: "https://github.com/DioxusLabs/dioxus".to_string(),
            ref_name: None,
            paths: None,
            role: None,
        }],
        tags: None,
        max_age_days: None,
        trust_score: None,
    };

    let temp = TempDir::new().unwrap();
    let shared_root = temp.path().join("knowledge");
    tokio::fs::create_dir_all(&shared_root).await.unwrap();

    let settings = KnowledgeSettings {
        shared_root_override: Some(shared_root.clone()),
        knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
        reconcile_seconds: 999_999,
        ..Default::default()
    };

    let engine = KnowledgeEngine::open(settings).await.unwrap();
    let summary = engine.topic_approval_summary(&request).await.unwrap();

    assert!(
        !summary.is_empty(),
        "approval summary should not be empty"
    );
    assert!(
        summary.contains("dioxus") || summary.contains("Dioxus") || summary.contains("DioxusLabs"),
        "summary should mention the repo: {summary}"
    );

    assert_yaml_snapshot!("dioxus_approval_summary", serde_json::json!({
        "summary_length": summary.len(),
        "contains_repo_name": summary.contains("dioxus") || summary.contains("Dioxus"),
        "summary_preview": summary.chars().take(200).collect::<String>(),
    }));
}

/// Full pipeline test: single setup, all assertions share the same indexed data.
///
/// This exercises clone → chunk → embed → index → search → list in sequence,
/// reusing the Dioxus fixture throughout.
#[tokio::test]
async fn dioxus_full_pipeline() {
    let f = DioxusFixture::setup().await;

    // ── Per-file indexing validation ────────────────────────────────

    // Verify chunk totals match the sum of per-file chunks
    let total_from_files: usize = f.per_file_chunks.iter().map(|fi| fi.chunk_count).sum();
    assert_eq!(
        total_from_files, f.chunk_count,
        "sum of per-file chunks ({}) should equal total chunk_count ({})",
        total_from_files, f.chunk_count,
    );

    // Verify via DB that every reference_files entry has matching chunks
    let orphan_count = sqlx::query_as::<_, (i64,)>(
        r#"SELECT COUNT(*) FROM reference_files rf
           WHERE rf.topic_id = ?
           AND NOT EXISTS (
               SELECT 1 FROM chunks c WHERE c.note_id = rf.note_id
           )"#,
    )
    .bind(&f.topic_id)
    .fetch_one(f.engine.pool())
    .await
    .expect("orphan query")
    .0;

    assert_eq!(
        orphan_count, 0,
        "no reference files should be orphaned (missing chunks)"
    );

    // Verify every file also has vector embeddings
    let files_without_embeddings = sqlx::query_as::<_, (String,)>(
        r#"SELECT rf.path FROM reference_files rf
           JOIN chunks c ON c.note_id = rf.note_id
           LEFT JOIN chunk_vec cv ON cv.rowid = c.id
           WHERE rf.topic_id = ?
           GROUP BY rf.path
           HAVING COUNT(cv.rowid) = 0"#,
    )
    .bind(&f.topic_id)
    .fetch_all(f.engine.pool())
    .await
    .expect("embedding check");

    assert!(
        files_without_embeddings.is_empty(),
        "all files should have embeddings, but these don't: {:?}",
        files_without_embeddings,
    );

    assert_yaml_snapshot!("dioxus_per_file_indexing", serde_json::json!({
        "source_count": f.source_count,
        "file_count": f.file_count,
        "chunk_count": f.chunk_count,
        "per_file_chunks": f.per_file_chunks,
    }));

    // ── Reference search: component lifecycle ──────────────────────

    let question = "component lifecycle hooks use_effect cleanup";
    let search_result = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "dioxus".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference_search should succeed");

    assert!(!search_result.results.is_empty(), "should find results for component lifecycle query");
    assert!(!search_result.topic_body.is_empty(), "should return topic body");
    assert!(!search_result.topic_id.is_empty(), "should return topic_id");

    assert_yaml_snapshot!(
        "dioxus_reference_search_component_lifecycle",
        build_search_snapshot(question, &search_result.results, 3)
    );

    // ── Reference search: RSX syntax ───────────────────────────────

    let question = "RSX syntax JSX-like markup rendering elements";
    let search_result = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "dioxus".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference_search should succeed");

    assert!(!search_result.results.is_empty(), "should find results for RSX syntax query");

    assert_yaml_snapshot!(
        "dioxus_reference_search_rsx_syntax",
        build_search_snapshot(question, &search_result.results, 3)
    );

    // ── Reference search: state management ─────────────────────────

    let question = "state management use_signal reactive hooks";
    let search_result = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "dioxus".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference_search should succeed");

    assert!(!search_result.results.is_empty(), "should find results for state management query");

    assert_yaml_snapshot!(
        "dioxus_reference_search_state_management",
        build_search_snapshot(question, &search_result.results, 3)
    );

    // ── Topic search by description ────────────────────────────────

    let results = f
        .engine
        .topic_search("Rust GUI framework with React-like syntax")
        .await
        .expect("topic_search should succeed");

    assert!(!results.is_empty(), "should find the Dioxus topic");

    let dioxus_found = results.iter().any(|r| r.title.contains("Dioxus"));
    assert!(dioxus_found, "Dioxus topic should appear in search results");

    assert_yaml_snapshot!("dioxus_topic_search_by_description", serde_json::json!({
        "total_results": results.len(),
        "top_result": {
            "topic_id": &results[0].topic_id,
            "title": &results[0].title,
            "status": &results[0].status,
            "is_stale": results[0].is_stale,
            "tags": &results[0].tags,
        },
    }));

    // ── Topic list and recent topics ───────────────────────────────

    let list = f.engine.topic_list(false).await.expect("topic_list");
    assert_eq!(list.len(), 1, "should have exactly one topic");
    assert_eq!(list[0].title, "Dioxus - Rust UI Framework");
    assert!(!list[0].is_stale, "freshly created topic should not be stale");
    assert!(list[0].file_count > 0, "should have files");

    // Validate file_count matches what we indexed
    assert_eq!(
        list[0].file_count, f.file_count,
        "topic_list file_count should match topic_create file_count"
    );

    let recent = f.engine.recent_topics().await.expect("recent_topics");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].1, "Dioxus - Rust UI Framework");
    assert!(
        recent[0].2.contains(&"dioxus".to_string()),
        "tags should include 'dioxus'"
    );

    assert_yaml_snapshot!("dioxus_topic_list_and_recent", serde_json::json!({
        "topic_list": {
            "count": list.len(),
            "title": &list[0].title,
            "status": &list[0].status,
            "is_stale": list[0].is_stale,
            "file_count": list[0].file_count,
            "source_count": list[0].source_count,
            "tags": &list[0].tags,
        },
        "recent_topics": {
            "count": recent.len(),
            "title": &recent[0].1,
            "tag_count": recent[0].2.len(),
        },
        "topic_create_stats": {
            "source_count": f.source_count,
            "file_count": f.file_count,
            "chunk_count": f.chunk_count,
        },
    }));
}

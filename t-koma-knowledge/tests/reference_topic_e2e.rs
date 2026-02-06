//! End-to-end integration test for the reference topic pipeline.
//!
//! This test exercises the full ghost reference workflow:
//! 1. `topic_approval_summary` — gathers repo metadata via `gh api`
//! 2. `topic_create` — clones a real repo, fetches a web page, indexes everything
//! 3. `reference_search` — hybrid BM25 + dense search over indexed content
//! 4. `topic_search` — semantic search over topic descriptions
//! 5. `topic_list` / `recent_topics` — listing and prompt injection
//!
//! Requires:
//! - `gh` CLI authenticated (for GitHub clone)
//! - Ollama running with `qwen3-embedding:8b` model
//! - Network access to github.com and dioxuslabs.com
//!
//! Run with: cargo test -p t-koma-knowledge --features slow-tests reference_topic_e2e

#![cfg(feature = "slow-tests")]

use insta::assert_yaml_snapshot;
use serde::Serialize;
use tempfile::TempDir;

use t_koma_knowledge::models::{KnowledgeContext, ReferenceQuery};
use t_koma_knowledge::{
    KnowledgeEngine, KnowledgeSettings, TopicCreateRequest, TopicSourceInput,
};

// ── Fixture ─────────────────────────────────────────────────────────

struct DioxusFixture {
    engine: KnowledgeEngine,
    context: KnowledgeContext,
    _topic_id: String,
    file_count: usize,
    chunk_count: usize,
    _temp: TempDir,
}

impl DioxusFixture {
    /// Create a reference topic from the real Dioxus repo (filtered to docs/examples)
    /// and a web page. This exercises the full pipeline: clone → chunk → embed → index.
    async fn setup() -> Self {
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
                },
                TopicSourceInput {
                    source_type: "web".to_string(),
                    url: "https://dioxuslabs.com/learn/0.6/".to_string(),
                    ref_name: None,
                    paths: None,
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

        assert!(result.file_count > 0, "should have fetched files");
        assert!(result.chunk_count > 0, "should have produced chunks");

        Self {
            engine,
            context,
            _topic_id: result.topic_id,
            file_count: result.file_count,
            chunk_count: result.chunk_count,
            _temp: temp,
        }
    }
}

// ── Snapshot helpers ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SearchResultSnapshot {
    query: String,
    total_hits: usize,
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
    results: &[t_koma_knowledge::MemoryResult],
    n: usize,
) -> SearchResultSnapshot {
    SearchResultSnapshot {
        query: query.to_string(),
        total_hits: results.len(),
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

    // Summary should contain repo metadata from gh api
    assert!(
        !summary.is_empty(),
        "approval summary should not be empty"
    );
    // Should mention the repo in some form
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

/// Full pipeline: clone → index → search for component lifecycle.
#[tokio::test]
async fn dioxus_reference_search_component_lifecycle() {
    let f = DioxusFixture::setup().await;

    let question = "component lifecycle hooks use_effect cleanup";
    let results = f
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

    assert!(!results.is_empty(), "should find results for component lifecycle query");

    assert_yaml_snapshot!(
        "dioxus_reference_search_component_lifecycle",
        build_search_snapshot(question, &results, 3)
    );
}

/// Search for RSX syntax patterns in the indexed Dioxus source.
#[tokio::test]
async fn dioxus_reference_search_rsx_syntax() {
    let f = DioxusFixture::setup().await;

    let question = "RSX syntax JSX-like markup rendering elements";
    let results = f
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

    assert!(!results.is_empty(), "should find results for RSX syntax query");

    assert_yaml_snapshot!(
        "dioxus_reference_search_rsx_syntax",
        build_search_snapshot(question, &results, 3)
    );
}

/// Search for state management (use_signal) patterns.
#[tokio::test]
async fn dioxus_reference_search_state_management() {
    let f = DioxusFixture::setup().await;

    let question = "state management use_signal reactive hooks";
    let results = f
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

    assert!(!results.is_empty(), "should find results for state management query");

    assert_yaml_snapshot!(
        "dioxus_reference_search_state_management",
        build_search_snapshot(question, &results, 3)
    );
}

/// Verify topic_search finds the Dioxus topic by description.
#[tokio::test]
async fn dioxus_topic_search_by_description() {
    let f = DioxusFixture::setup().await;

    let results = f
        .engine
        .topic_search("Rust GUI framework with React-like syntax")
        .await
        .expect("topic_search should succeed");

    assert!(!results.is_empty(), "should find the Dioxus topic");

    let dioxus_found = results
        .iter()
        .any(|r| r.title.contains("Dioxus"));
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
}

/// Verify topic_list and recent_topics include the created topic.
#[tokio::test]
async fn dioxus_topic_list_and_recent() {
    let f = DioxusFixture::setup().await;

    // topic_list
    let list = f.engine.topic_list(false).await.expect("topic_list");
    assert_eq!(list.len(), 1, "should have exactly one topic");
    assert_eq!(list[0].title, "Dioxus - Rust UI Framework");
    assert!(!list[0].is_stale, "freshly created topic should not be stale");
    assert!(list[0].file_count > 0, "should have files");

    // recent_topics (system prompt injection)
    let recent = f.engine.recent_topics().await.expect("recent_topics");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].1, "Dioxus - Rust UI Framework");
    assert!(recent[0].2.contains(&"dioxus".to_string()), "tags should include 'dioxus'");

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
            "file_count": f.file_count,
            "chunk_count": f.chunk_count,
        },
    }));
}

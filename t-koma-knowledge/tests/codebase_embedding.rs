//! Integration tests that index real Rust source files from the knowledge crate
//! and verify search over embedded codebase content.
//!
//! These tests require a running Ollama instance with the configured embedding
//! model (default: qwen3-embedding:8b). They are gated behind the `slow-tests`
//! feature.
//!
//! Run with: cargo test -p t-koma-knowledge --features slow-tests codebase_embedding

#![cfg(feature = "slow-tests")]

use std::path::{Path, PathBuf};

use insta::assert_yaml_snapshot;
use serde::Serialize;
use tempfile::TempDir;

use t_koma_knowledge::models::{
    KnowledgeContext, MemoryQuery, MemoryResult, MemoryScope, NoteCreateRequest, ReferenceQuery,
    WriteScope,
};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};

// ── Fixture ─────────────────────────────────────────────────────────

/// Source files from our own crate to index as reference content.
const SOURCE_FILES: &[&str] = &[
    "models.rs",
    "graph.rs",
    "parser.rs",
    "chunker.rs",
    "storage.rs",
    "search.rs",
];

/// Build a fully functional engine + context with real embeddings,
/// a reference topic pointing to actual source files, and a ghost workspace.
struct CodebaseFixture {
    engine: KnowledgeEngine,
    context: KnowledgeContext,
    _temp: TempDir,
}

impl CodebaseFixture {
    async fn setup() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let shared_root = temp.path().join("knowledge");
        let reference_root = temp.path().join("reference");
        let topic_dir = reference_root.join("t-koma-knowledge-src");
        let workspace = temp.path().join("ghost-a-workspace");

        tokio::fs::create_dir_all(&shared_root).await.unwrap();
        tokio::fs::create_dir_all(&topic_dir).await.unwrap();
        tokio::fs::create_dir_all(workspace.join("private_knowledge"))
            .await
            .unwrap();

        // Copy real source files into the topic directory
        let crate_src = crate_src_dir();
        for file_name in SOURCE_FILES {
            let src = if *file_name == "search.rs" {
                crate_src.join("engine/search.rs")
            } else {
                crate_src.join(file_name)
            };
            let dst = topic_dir.join(file_name);
            tokio::fs::copy(&src, &dst)
                .await
                .unwrap_or_else(|e| panic!("failed to copy {} -> {}: {}", src.display(), dst.display(), e));
        }

        // Write topic.md with TOML front matter
        let files_toml: Vec<String> = SOURCE_FILES.iter().map(|f| format!("\"{}\"", f)).collect();
        let topic_md = format!(
            r#"+++
id = "topic-knowledge-src"
title = "t-koma-knowledge source"
type = "ReferenceTopic"
created_at = "2025-06-01T00:00:00Z"
trust_score = 10
tags = ["rust", "knowledge", "t-koma"]
files = [{files}]

[created_by]
ghost = "system"
model = "indexer"
+++

# t-koma-knowledge Source Code

This reference topic contains the core source files of the t-koma-knowledge crate,
including models, search pipeline, graph traversal, parsing, chunking, and storage.
"#,
            files = files_toml.join(", ")
        );
        tokio::fs::write(topic_dir.join("topic.md"), &topic_md)
            .await
            .unwrap();

        let settings = KnowledgeSettings {
            shared_root_override: Some(shared_root.clone()),
            reference_root_override: Some(reference_root),
            knowledge_db_path_override: Some(shared_root.join("index.sqlite3")),
            // Use a short reconcile window so we can trigger indexing
            reconcile_seconds: 0,
            ..Default::default()
        };

        let engine = KnowledgeEngine::open(settings).await.expect("open engine");
        let context = KnowledgeContext {
            ghost_name: "ghost-a".to_string(),
            workspace_root: workspace,
        };

        // Trigger reconcile by doing a reference search (reconcile_seconds=0 → always runs)
        let _ = engine
            .reference_search(
                &context,
                ReferenceQuery {
                    topic: "knowledge source".to_string(),
                    question: "warmup".to_string(),
                    options: Default::default(),
                },
            )
            .await;

        Self {
            engine,
            context,
            _temp: temp,
        }
    }
}

/// Locate the `src/` directory of this crate.
fn crate_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

// ── Snapshot helpers ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SnapshotHit {
    title: String,
    note_type: String,
    score: String,
    snippet_preview: String,
}

fn snapshot_results(results: &[MemoryResult]) -> Vec<SnapshotHit> {
    results
        .iter()
        .map(|r| SnapshotHit {
            title: r.summary.title.clone(),
            note_type: r.summary.note_type.clone(),
            score: format!("{:.4}", r.summary.score),
            snippet_preview: r.summary.snippet.chars().take(80).collect(),
        })
        .collect()
}

// ── Test cases ──────────────────────────────────────────────────────

/// Verify that hybrid search (BM25 + dense embeddings) finds relevant source chunks.
#[tokio::test]
async fn hybrid_search_finds_search_pipeline() {
    let f = CodebaseFixture::setup().await;

    let results = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: "hybrid search with BM25 and embeddings".to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search should succeed");

    assert!(!results.is_empty(), "should find at least one result");

    // Verify search.rs appears in results (it contains the BM25+dense pipeline)
    let has_search_file = results
        .iter()
        .any(|r| r.summary.title.contains("search"));
    assert!(
        has_search_file,
        "search.rs should appear in results for BM25+embeddings query"
    );

    assert_yaml_snapshot!("hybrid_search_finds_search_pipeline", snapshot_results(&results));
}

/// Verify that scope isolation works: a ghost-private note is not visible
/// to a reference search (different scope).
#[tokio::test]
async fn scope_isolation_ghost_vs_reference() {
    let f = CodebaseFixture::setup().await;

    // Create a private note with similar content
    let _ = f
        .engine
        .note_create(
            &f.context,
            NoteCreateRequest {
                title: "Private Search Notes".to_string(),
                note_type: "Concept".to_string(),
                scope: WriteScope::Private,
                body: "BM25 search and dense embeddings pipeline notes.".to_string(),
                parent: None,
                tags: Some(vec!["search".to_string()]),
                source: None,
                trust_score: Some(5),
            },
        )
        .await;

    // Reference search should only return reference-scoped results
    let ref_results = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: "BM25 search pipeline".to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    let private_leaked = ref_results
        .iter()
        .any(|r| r.summary.title == "Private Search Notes");
    assert!(
        !private_leaked,
        "private notes must not appear in reference search results"
    );

    // Memory search in ghost scope should find the private note
    let mem_results = f
        .engine
        .memory_search(
            &f.context,
            MemoryQuery {
                query: "BM25 search pipeline".to_string(),
                scope: MemoryScope::GhostPrivate,
                options: Default::default(),
            },
        )
        .await
        .expect("memory search");

    let has_private = mem_results
        .iter()
        .any(|r| r.summary.title == "Private Search Notes");
    assert!(
        has_private,
        "private note should appear in ghost-private memory search"
    );

    assert_yaml_snapshot!(
        "scope_isolation_ghost_vs_reference",
        serde_json::json!({
            "reference_result_count": ref_results.len(),
            "private_note_in_reference": private_leaked,
            "private_note_in_memory": has_private,
        })
    );
}

/// Verify that TOML front matter is correctly parsed and indexed for
/// the reference topic itself.
#[tokio::test]
async fn toml_front_matter_parsing() {
    let f = CodebaseFixture::setup().await;

    let results = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "t-koma-knowledge source".to_string(),
                question: "core source files models search graph".to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    // The topic itself should be findable
    assert!(!results.is_empty(), "should find reference topic results");

    assert_yaml_snapshot!("toml_front_matter_parsing", snapshot_results(&results));
}

/// Verify that tree-sitter code chunking produces meaningful chunks from
/// Rust source files.
#[tokio::test]
async fn tree_sitter_code_chunking() {
    let f = CodebaseFixture::setup().await;

    // Query for a specific function that should be in its own chunk
    let results = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: "sanitize FTS5 query quoting tokens".to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    assert!(!results.is_empty(), "should find FTS5 sanitization code");

    // The snippet should contain actual Rust code
    let has_code = results
        .iter()
        .any(|r| r.summary.snippet.contains("fn ") || r.summary.snippet.contains("pub"));
    assert!(has_code, "snippets should contain Rust code from chunked source files");

    assert_yaml_snapshot!("tree_sitter_code_chunking", snapshot_results(&results));
}

/// Verify that knowledge graph links (tags, parent) are resolved and
/// returned with search results from reference files.
#[tokio::test]
async fn knowledge_graph_link_resolution() {
    let f = CodebaseFixture::setup().await;

    let results = f
        .engine
        .reference_search(
            &f.context,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: "knowledge graph link resolution parent".to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    assert!(!results.is_empty(), "should find graph-related results");

    // Check that at least some results have graph metadata populated
    // (tags come from the topic.md which has tags = ["rust", "knowledge", "t-koma"])
    let snapshot: Vec<_> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "title": r.summary.title,
                "note_type": r.summary.note_type,
                "has_tags": !r.tags.is_empty(),
                "tag_count": r.tags.len(),
                "has_parents": !r.parents.is_empty(),
                "links_out_count": r.links_out.len(),
                "links_in_count": r.links_in.len(),
            })
        })
        .collect();

    assert_yaml_snapshot!("knowledge_graph_link_resolution", snapshot);
}

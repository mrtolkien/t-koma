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
    NoteCreateRequest, NoteQuery, NoteResult, NoteSearchScope, ReferenceQuery,
    WriteScope,
};
use t_koma_knowledge::{KnowledgeEngine, KnowledgeSettings};

// -- Fixture -----------------------------------------------------------------

/// Source code files from our own crate to index as reference content.
const CODE_FILES: &[&str] = &[
    "models.rs",
    "graph.rs",
    "parser.rs",
    "chunker.rs",
    "storage.rs",
    "search.rs",
];

/// Documentation files to index as docs (boosted in search).
const DOC_FILES: &[&str] = &[
    "knowledge_system.md",
    "testing.md",
];

/// Build a fully functional engine with real embeddings,
/// a reference topic pointing to actual source files and documentation,
/// and a ghost workspace.
struct CodebaseFixture {
    engine: KnowledgeEngine,
    ghost_name: String,
    _temp: TempDir,
}

impl CodebaseFixture {
    async fn setup() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let data_root = temp.path().join("data");
        let shared_notes = data_root.join("shared").join("notes");
        let reference_root = data_root.join("shared").join("references");
        let topic_dir = reference_root.join("t-koma-knowledge-src");
        let ghost_notes = data_root.join("ghosts").join("ghost-a").join("notes");

        tokio::fs::create_dir_all(&shared_notes).await.unwrap();
        tokio::fs::create_dir_all(&topic_dir).await.unwrap();
        tokio::fs::create_dir_all(&ghost_notes).await.unwrap();

        let crate_root = crate_root_dir();

        // Copy real source files into the topic directory
        for file_name in CODE_FILES {
            let src = if *file_name == "search.rs" {
                crate_root.join("src/engine/search.rs")
            } else {
                crate_root.join("src").join(file_name)
            };
            let dst = topic_dir.join(file_name);
            tokio::fs::copy(&src, &dst)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to copy {} -> {}: {}",
                        src.display(),
                        dst.display(),
                        e
                    )
                });
        }

        // Copy real doc files into the topic directory
        for file_name in DOC_FILES {
            let src = if *file_name == "knowledge_system.md" {
                crate_root.join("knowledge/prompts/knowledge_system.md")
            } else {
                // testing.md lives in vibe/knowledge/
                crate_root.join("../vibe/knowledge").join(file_name)
            };
            let dst = topic_dir.join(file_name);
            tokio::fs::copy(&src, &dst)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "failed to copy {} -> {}: {}",
                        src.display(),
                        dst.display(),
                        e
                    )
                });
        }

        // Build the combined files list
        let all_files: Vec<String> = CODE_FILES
            .iter()
            .chain(DOC_FILES.iter())
            .map(|f| format!("\"{}\"", f))
            .collect();

        // Write topic.md with TOML front matter including [[sources]] with roles
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

[[sources]]
type = "git"
url = "https://github.com/example/t-koma"
role = "code"
paths = ["models.rs", "graph.rs", "parser.rs", "chunker.rs", "storage.rs", "search.rs"]

[[sources]]
type = "web"
url = "https://example.com/docs"
role = "docs"
paths = ["knowledge_system.md", "testing.md"]
+++

# t-koma-knowledge Source Code & Documentation

This reference topic contains the core source files and documentation of the
t-koma-knowledge crate, including models, search pipeline, graph traversal,
parsing, chunking, storage, and usage guides.
"#,
            files = all_files.join(", ")
        );
        tokio::fs::write(topic_dir.join("topic.md"), &topic_md)
            .await
            .unwrap();

        let settings = KnowledgeSettings {
            data_root_override: Some(data_root),
            // Use a short reconcile window so we can trigger indexing
            reconcile_seconds: 0,
            ..Default::default()
        };

        let engine = KnowledgeEngine::open(settings).await.expect("open engine");
        let ghost_name = "ghost-a".to_string();

        // Trigger reconcile by doing a reference search (reconcile_seconds=0 -> always runs)
        let _ = engine
            .reference_search(
                &ghost_name,
                ReferenceQuery {
                    topic: "knowledge source".to_string(),
                    question: "warmup".to_string(),
                    options: Default::default(),
                },
            )
            .await;

        Self {
            engine,
            ghost_name,
            _temp: temp,
        }
    }
}

/// Locate the crate root directory (CARGO_MANIFEST_DIR).
fn crate_root_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

// -- Snapshot helpers --------------------------------------------------------

/// A self-documenting snapshot entry: shows the query, total hit count,
/// and the top N results with enough snippet to judge relevance.
#[derive(Debug, Serialize)]
struct SearchSnapshot {
    query: String,
    total_hits: usize,
    top_results: Vec<HitSnapshot>,
}

#[derive(Debug, Serialize)]
struct HitSnapshot {
    rank: usize,
    file: String,
    note_type: String,
    snippet: String,
}

/// Build a snapshot showing the query and the top `n` results.
fn build_snapshot(query: &str, results: &[NoteResult], n: usize) -> SearchSnapshot {
    SearchSnapshot {
        query: query.to_string(),
        total_hits: results.len(),
        top_results: results
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, r)| HitSnapshot {
                rank: i + 1,
                file: r.summary.title.clone(),
                note_type: r.summary.note_type.clone(),
                snippet: r.summary.snippet.chars().take(120).collect(),
            })
            .collect(),
    }
}

// -- Test cases --------------------------------------------------------------

/// Verify that hybrid search (BM25 + dense embeddings) finds relevant source chunks.
#[tokio::test]
async fn hybrid_search_finds_search_pipeline() {
    let f = CodebaseFixture::setup().await;

    let question = "hybrid search with BM25 and embeddings";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search should succeed");

    let results = &search_result.results;
    assert!(!results.is_empty(), "should find at least one result");

    let has_search_file = results
        .iter()
        .any(|r| r.summary.title.contains("search"));
    assert!(
        has_search_file,
        "search.rs should appear in results for BM25+embeddings query"
    );

    assert_yaml_snapshot!(
        "hybrid_search_finds_search_pipeline",
        build_snapshot(question, results, 2)
    );
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
            &f.ghost_name,
            NoteCreateRequest {
                title: "Private Search Notes".to_string(),
                note_type: "Concept".to_string(),
                scope: WriteScope::GhostNote,
                body: "BM25 search and dense embeddings pipeline notes.".to_string(),
                parent: None,
                tags: Some(vec!["search".to_string()]),
                source: None,
                trust_score: Some(5),
            },
        )
        .await;

    let question = "BM25 search pipeline";

    // Reference search should only return reference-scoped results
    let ref_search = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");
    let ref_results = &ref_search.results;

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
            &f.ghost_name,
            NoteQuery {
                query: question.to_string(),
                scope: NoteSearchScope::GhostOnly,
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
            "query": question,
            "reference_search": {
                "total_hits": ref_results.len(),
                "contains_private_note": private_leaked,
                "top_file": ref_results.first().map(|r| &r.summary.title),
            },
            "memory_search_ghost_private": {
                "total_hits": mem_results.len(),
                "contains_private_note": has_private,
                "top_title": mem_results.first().map(|r| &r.summary.title),
            },
        })
    );
}

/// Verify that TOML front matter is correctly parsed and indexed for
/// the reference topic itself.
#[tokio::test]
async fn toml_front_matter_parsing() {
    let f = CodebaseFixture::setup().await;

    let question = "core source files models search graph";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "t-koma-knowledge source".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    let results = &search_result.results;
    assert!(!results.is_empty(), "should find reference topic results");

    assert_yaml_snapshot!(
        "toml_front_matter_parsing",
        build_snapshot(question, results, 2)
    );
}

/// Verify that tree-sitter code chunking produces meaningful chunks from
/// Rust source files.
#[tokio::test]
async fn tree_sitter_code_chunking() {
    let f = CodebaseFixture::setup().await;

    let question = "sanitize FTS5 query quoting tokens";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    let results = &search_result.results;
    assert!(!results.is_empty(), "should find FTS5 sanitization code");

    let has_code = results
        .iter()
        .any(|r| r.summary.snippet.contains("fn ") || r.summary.snippet.contains("pub"));
    assert!(
        has_code,
        "snippets should contain Rust code from chunked source files"
    );

    assert_yaml_snapshot!(
        "tree_sitter_code_chunking",
        build_snapshot(question, results, 2)
    );
}

/// Verify that knowledge graph links (tags, parent) are resolved and
/// returned with search results from reference files.
#[tokio::test]
async fn knowledge_graph_link_resolution() {
    let f = CodebaseFixture::setup().await;

    let question = "knowledge graph link resolution parent";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "knowledge source".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference search");

    let results = &search_result.results;
    assert!(!results.is_empty(), "should find graph-related results");

    #[derive(Debug, Serialize)]
    struct GraphSnapshot {
        query: String,
        total_hits: usize,
        top_results: Vec<GraphHit>,
    }

    #[derive(Debug, Serialize)]
    struct GraphHit {
        rank: usize,
        file: String,
        has_parents: bool,
        tag_count: usize,
        links_out: usize,
        links_in: usize,
        snippet: String,
    }

    let snapshot = GraphSnapshot {
        query: question.to_string(),
        total_hits: results.len(),
        top_results: results
            .iter()
            .take(2)
            .enumerate()
            .map(|(i, r)| GraphHit {
                rank: i + 1,
                file: r.summary.title.clone(),
                has_parents: !r.parents.is_empty(),
                tag_count: r.tags.len(),
                links_out: r.links_out.len(),
                links_in: r.links_in.len(),
                snippet: r.summary.snippet.chars().take(120).collect(),
            })
            .collect(),
    };

    assert_yaml_snapshot!("knowledge_graph_link_resolution", snapshot);
}

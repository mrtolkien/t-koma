//! End-to-end integration test for the web crawl pipeline.
//!
//! Crawls the OpenRouter quickstart docs (depth 1) and validates:
//! 1. BFS crawl fetches the seed + same-host linked pages
//! 2. All crawled pages are chunked and embedded
//! 3. Hybrid search returns relevant results for API-related queries
//!
//! Requires:
//! - Ollama running with `qwen3-embedding:8b` model
//! - Network access to openrouter.ai
//!
//! Run with:
//!   cargo test -p t-koma-knowledge --features slow-tests crawl_e2e
//!
//! For initial snapshot creation, use:
//!   cargo insta test -p t-koma-knowledge --features slow-tests -- crawl_e2e

#![cfg(feature = "slow-tests")]

use insta::assert_yaml_snapshot;
use serde::Serialize;
use sqlx::SqlitePool;
use tempfile::TempDir;

use t_koma_knowledge::models::ReferenceQuery;
use t_koma_knowledge::{
    KnowledgeEngine, KnowledgeSettings, NoteResult, TopicCreateRequest, TopicSourceInput,
};

// -- Fixture -----------------------------------------------------------------

struct CrawlFixture {
    engine: KnowledgeEngine,
    ghost_name: String,
    topic_id: String,
    source_count: usize,
    file_count: usize,
    chunk_count: usize,
    per_file: Vec<FileChunkInfo>,
    _temp: TempDir,
}

#[derive(Debug, Clone, Serialize)]
struct FileChunkInfo {
    file_path: String,
    chunk_count: usize,
}

impl CrawlFixture {
    async fn setup() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("t_koma_knowledge=debug,warn")
            .with_test_writer()
            .try_init();

        let temp = TempDir::new().expect("tempdir");
        let data_root = temp.path().join("data");
        let shared_notes = data_root.join("shared").join("notes");
        let shared_references = data_root.join("shared").join("references");

        tokio::fs::create_dir_all(&shared_notes).await.unwrap();
        tokio::fs::create_dir_all(&shared_references).await.unwrap();

        let settings = KnowledgeSettings {
            data_root_override: Some(data_root),
            reconcile_seconds: 999_999,
            ..Default::default()
        };

        let engine = KnowledgeEngine::open(settings).await.expect("open engine");
        let ghost_name = "ghost-crawler".to_string();

        let request = TopicCreateRequest {
            title: "OpenRouter API".to_string(),
            body: "OpenRouter provides a unified API for accessing multiple LLM providers \
                   (OpenAI, Anthropic, Google, Meta, etc.) through a single endpoint. \
                   It supports the OpenAI chat completions format, model routing, \
                   and provider preferences."
                .to_string(),
            sources: vec![TopicSourceInput {
                source_type: "crawl".to_string(),
                url: "https://openrouter.ai/docs/quickstart".to_string(),
                ref_name: None,
                paths: None,
                role: None, // inferred as "docs" for crawl
                max_depth: Some(1),
                max_pages: Some(20),
            }],
            tags: Some(vec![
                "llm".to_string(),
                "api".to_string(),
                "openrouter".to_string(),
            ]),
            max_age_days: Some(30),
            trust_score: Some(8),
        };

        let result = engine
            .topic_create(&ghost_name, request)
            .await
            .expect("topic_create with crawl source should succeed");

        assert_eq!(
            result.source_count, 1,
            "should have exactly one source (crawl)"
        );
        assert!(
            result.file_count > 1,
            "crawl depth 1 should fetch more than just the seed (got {})",
            result.file_count
        );
        assert!(
            result.chunk_count > 0,
            "should have produced chunks (got {})",
            result.chunk_count
        );

        let per_file = query_per_file_chunks(engine.pool(), &result.topic_id).await;

        for info in &per_file {
            assert!(
                info.chunk_count > 0,
                "file '{}' has 0 chunks -- crawl indexing broken",
                info.file_path,
            );
        }

        assert_eq!(
            per_file.len(),
            result.file_count,
            "DB file count ({}) should match topic_create report ({})",
            per_file.len(),
            result.file_count,
        );

        Self {
            engine,
            ghost_name,
            topic_id: result.topic_id,
            source_count: result.source_count,
            file_count: result.file_count,
            chunk_count: result.chunk_count,
            per_file,
            _temp: temp,
        }
    }
}

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

// -- Snapshot helpers --------------------------------------------------------

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

fn build_search_snapshot(query: &str, results: &[NoteResult], n: usize) -> SearchResultSnapshot {
    let unique_files: std::collections::HashSet<&str> =
        results.iter().map(|r| r.summary.title.as_str()).collect();

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

// -- Test cases --------------------------------------------------------------

#[tokio::test]
async fn openrouter_crawl_pipeline() {
    let f = CrawlFixture::setup().await;

    // -- Indexing validation --------------------------------------------------

    let total_from_files: usize = f.per_file.iter().map(|fi| fi.chunk_count).sum();
    assert_eq!(
        total_from_files, f.chunk_count,
        "sum of per-file chunks ({}) should equal total chunk_count ({})",
        total_from_files, f.chunk_count,
    );

    // Verify every file has embeddings
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
        "all crawled pages should have embeddings, but these don't: {:?}",
        files_without_embeddings,
    );

    assert_yaml_snapshot!(
        "openrouter_crawl_indexing",
        serde_json::json!({
            "source_count": f.source_count,
            "file_count": f.file_count,
            "chunk_count": f.chunk_count,
            "per_file_chunks": f.per_file,
        })
    );

    // -- Search: API authentication ------------------------------------------

    let question = "API key authentication headers";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "openrouter".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference_search should succeed");

    assert!(
        !search_result.results.is_empty(),
        "should find results for API auth query"
    );

    assert_yaml_snapshot!(
        "openrouter_search_api_auth",
        build_search_snapshot(question, &search_result.results, 3)
    );

    // -- Search: model routing -----------------------------------------------

    let question = "model selection routing provider preferences";
    let search_result = f
        .engine
        .reference_search(
            &f.ghost_name,
            ReferenceQuery {
                topic: "openrouter".to_string(),
                question: question.to_string(),
                options: Default::default(),
            },
        )
        .await
        .expect("reference_search should succeed");

    assert!(
        !search_result.results.is_empty(),
        "should find results for model routing query"
    );

    assert_yaml_snapshot!(
        "openrouter_search_model_routing",
        build_search_snapshot(question, &search_result.results, 3)
    );

    // -- Topic list ----------------------------------------------------------

    let list = f.engine.topic_list(false).await.expect("topic_list");
    assert_eq!(list.len(), 1, "should have exactly one topic");
    assert_eq!(list[0].title, "OpenRouter API");
    assert!(list[0].file_count > 1, "should have multiple crawled pages");

    assert_yaml_snapshot!(
        "openrouter_crawl_topic_list",
        serde_json::json!({
            "title": &list[0].title,
            "file_count": list[0].file_count,
            "tags": &list[0].tags,
        })
    );
}

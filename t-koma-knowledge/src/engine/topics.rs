//! Engine methods for reference topic management.
//!
//! Topics are regular shared notes. This module handles topic creation
//! (two-phase with approval), searching, and listing via joins against
//! the `reference_files` table.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::KnowledgeResult;
use crate::models::{
    KnowledgeScope, NoteCreateRequest, SourceRole, TopicCreateRequest, TopicCreateResult,
    TopicListEntry, TopicSearchResult, WriteScope, generate_note_id,
};
use crate::sources::{self, TopicSource};

use super::KnowledgeEngine;
use super::search::{dense_search, hydrate_summaries, rrf_fuse, sanitize_fts5_query};

/// Build an approval summary for Phase 1 (metadata gathering).
///
/// Queries GitHub API for repo metadata and builds a human-readable summary
/// for the operator to approve or deny.
pub(crate) async fn build_topic_approval_summary(
    request: &TopicCreateRequest,
) -> KnowledgeResult<String> {
    Ok(sources::build_approval_summary(&request.sources).await)
}

/// Execute Phase 2: create a shared note for the topic, fetch sources, index everything.
///
/// Called after operator approval. Returns topic ID, file count, and chunk count.
pub(crate) async fn topic_create_execute(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    request: TopicCreateRequest,
) -> KnowledgeResult<TopicCreateResult> {
    let settings = engine.settings();
    let pool = engine.pool();
    let embedder = engine.embedder();

    // Create topic directory for reference files
    let reference_root = crate::paths::shared_references_root(settings)?;
    let topic_dir_name = super::notes::sanitize_filename(&request.title);
    let topic_dir = reference_root.join(&topic_dir_name);
    tokio::fs::create_dir_all(&topic_dir).await?;

    // Fetch all sources
    let fetched = sources::fetch_all_sources(&request.sources, &topic_dir).await?;

    // Collect files with their roles and topic sources
    let mut file_roles: Vec<(String, SourceRole)> = Vec::new();
    let mut topic_sources: Vec<TopicSource> = Vec::new();
    for result in &fetched {
        let role = result
            .source
            .role
            .unwrap_or_else(|| SourceRole::infer(&result.source.source_type));
        for f in &result.files {
            file_roles.push((f.clone(), role));
        }
        topic_sources.push(result.source.clone());
    }
    let all_files: Vec<String> = file_roles.iter().map(|(f, _)| f.clone()).collect();

    // Create the topic as a shared note via note_create
    let note_request = NoteCreateRequest {
        title: request.title.clone(),
        archetype: None,
        scope: WriteScope::SharedNote,
        body: request.body.clone(),
        parent: None,
        tags: request.tags.clone(),
        source: None,
        trust_score: request.trust_score,
    };
    let note_result = super::notes::note_create(engine, ghost_name, model, note_request).await?;
    let topic_id = note_result.note_id;

    // Store reference_files mapping with role
    let now = Utc::now();
    for (file_name, role) in &file_roles {
        let file_path = topic_dir.join(file_name);
        let file_content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let note_type = role.to_entry_type();
        let file_note_id = generate_note_id();
        let context_prefix = format!("[{}]", request.title);
        let file_ingested = crate::ingest::ingest_reference_file_with_context(
            settings,
            &file_path,
            &file_content,
            &file_note_id,
            file_name,
            note_type,
            Some(&context_prefix),
        )
        .await?;
        crate::storage::upsert_note(pool, &file_ingested.note).await?;
        crate::storage::replace_tags(pool, &file_note_id, &file_ingested.tags).await?;
        crate::storage::replace_links(pool, &file_note_id, None, &file_ingested.links).await?;
        let file_chunk_ids = crate::storage::replace_chunks(
            pool,
            &file_note_id,
            file_name,
            note_type,
            None,
            &file_ingested.chunks,
        )
        .await?;
        crate::index::embed_chunks(
            settings,
            embedder,
            pool,
            &file_ingested.chunks,
            &file_chunk_ids,
        )
        .await?;

        // Determine source_url from the FetchedSource that produced this file
        let source_url = fetched
            .iter()
            .find(|r| r.files.contains(file_name))
            .map(|r| r.source.url.clone());
        let source_type = fetched
            .iter()
            .find(|r| r.files.contains(file_name))
            .map(|r| r.source.source_type.as_str())
            .unwrap_or("git");

        // Link file to topic with role and provenance metadata
        sqlx::query(
            "INSERT OR REPLACE INTO reference_files (topic_id, note_id, path, role, source_url, source_type, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&topic_id)
        .bind(&file_note_id)
        .bind(file_name)
        .bind(role.as_str())
        .bind(&source_url)
        .bind(source_type)
        .bind(now.to_rfc3339())
        .execute(pool)
        .await?;
    }

    let chunk_count = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM chunks c JOIN reference_files rf ON c.note_id = rf.note_id WHERE rf.topic_id = ?",
    )
    .bind(&topic_id)
    .fetch_one(pool)
    .await
    .map(|(c,)| c as usize)
    .unwrap_or(0);

    Ok(TopicCreateResult {
        topic_id,
        source_count: topic_sources.len(),
        file_count: all_files.len(),
        chunk_count,
    })
}

/// Semantic search over reference topics (shared notes that have reference files).
pub(crate) async fn topic_search(
    engine: &KnowledgeEngine,
    query: &str,
) -> KnowledgeResult<Vec<TopicSearchResult>> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    // Find topic IDs: shared notes that have reference files
    let topic_ids = sqlx::query_as::<_, (String,)>(
        "SELECT DISTINCT n.id FROM notes n \
         JOIN reference_files rf ON rf.topic_id = n.id \
         WHERE n.scope = 'shared_note'",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect::<Vec<_>>();

    if topic_ids.is_empty() {
        return Ok(Vec::new());
    }

    // BM25 search
    let placeholders = topic_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT chunk_id, bm25(chunk_fts) as score FROM chunk_fts JOIN notes ON notes.id = chunk_fts.note_id WHERE chunk_fts MATCH ? AND notes.id IN ({}) ORDER BY score ASC LIMIT ?",
        placeholders
    );
    let safe_query = sanitize_fts5_query(query);
    let mut qb = sqlx::query_as::<_, (i64, f32)>(&sql);
    qb = qb.bind(&safe_query);
    for id in &topic_ids {
        qb = qb.bind(id);
    }
    qb = qb.bind(settings.search.bm25_limit as i64);
    let bm25_hits = qb.fetch_all(pool).await?;

    // Dense search — use SharedNote scope since topics are now shared notes
    let dense_hits = dense_search(
        embedder,
        pool,
        query,
        settings.search.dense_limit,
        Some(&topic_ids),
        KnowledgeScope::SharedNote,
        "",
        None,
    )
    .await?;

    // Fuse and rank
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);

    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::SharedNote, "").await?;

    let mut results = Vec::new();
    for summary in summaries {
        results.push(TopicSearchResult {
            topic_id: summary.id.clone(),
            title: summary.title,
            tags: load_topic_tags(pool, &summary.id).await?,
            score: summary.score,
            snippet: summary.snippet,
        });
    }

    Ok(results)
}

/// List all reference topics (shared notes with reference files).
pub(crate) async fn topic_list(
    engine: &KnowledgeEngine,
    _include_obsolete: bool,
) -> KnowledgeResult<Vec<TopicListEntry>> {
    let pool = engine.pool();

    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT DISTINCT n.id, n.title, n.created_by_ghost \
         FROM notes n \
         JOIN reference_files rf ON rf.topic_id = n.id \
         WHERE n.scope = 'shared_note' \
         ORDER BY n.created_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();
    for (id, title, ghost) in rows {
        let file_count =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM reference_files WHERE topic_id = ?")
                .bind(&id)
                .fetch_one(pool)
                .await
                .map(|(c,)| c as usize)
                .unwrap_or(0);

        let tags = load_topic_tags(pool, &id).await?;

        // Derive collection directories from reference file paths
        let dir_rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT SUBSTR(path, 1, INSTR(path, '/') - 1) \
             FROM reference_files \
             WHERE topic_id = ? AND path LIKE '%/%'",
        )
        .bind(&id)
        .fetch_all(pool)
        .await?;

        let collection_dirs: Vec<String> = dir_rows
            .into_iter()
            .map(|(d,)| d)
            .filter(|d| !d.is_empty())
            .collect();

        entries.push(TopicListEntry {
            topic_id: id,
            title,
            created_by_ghost: ghost,
            file_count,
            collection_dirs,
            tags,
        });
    }

    Ok(entries)
}

/// Load the 10 most recent reference topics for system prompt injection.
pub(crate) async fn recent_topics(
    pool: &SqlitePool,
) -> KnowledgeResult<Vec<(String, String, Vec<String>)>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT n.id, n.title \
         FROM notes n \
         JOIN reference_files rf ON rf.topic_id = n.id \
         WHERE n.scope = 'shared_note' \
         ORDER BY n.created_at DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for (id, title) in rows {
        let tags = load_topic_tags(pool, &id).await?;
        results.push((id, title, tags));
    }

    Ok(results)
}

// ── Internal helpers ────────────────────────────────────────────────

async fn load_topic_tags(pool: &SqlitePool, topic_id: &str) -> KnowledgeResult<Vec<String>> {
    let rows = sqlx::query_as::<_, (String,)>("SELECT tag FROM note_tags WHERE note_id = ?")
        .bind(topic_id)
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(|(t,)| t).collect())
}

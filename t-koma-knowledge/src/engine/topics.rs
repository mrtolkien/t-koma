//! Engine methods for reference topic management.
//!
//! Handles topic creation (two-phase with approval), searching,
//! listing, and metadata updates.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{
    CollectionSummary, KnowledgeScope, SourceRole, TopicCreateRequest, TopicCreateResult,
    TopicListEntry, TopicSearchResult, TopicUpdateRequest, generate_note_id,
};
use crate::sources::{self, TopicSource};

use super::KnowledgeEngine;
use super::notes::{rebuild_front_matter, sanitize_filename};
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

/// Execute Phase 2: fetch sources, write topic.md, index everything.
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

    // Create topic directory
    let reference_root = crate::paths::shared_references_root(settings)?;
    let topic_dir_name = sanitize_filename(&request.title);
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

    // Write topic.md
    let topic_id = generate_note_id();
    let now = Utc::now();
    let trust_score = request.trust_score.unwrap_or(8);

    let front_matter = build_topic_front_matter(
        &topic_id,
        &request.title,
        ghost_name,
        model,
        trust_score,
        request.tags.as_deref(),
        &now,
    );

    let content = format!("+++\n{}\n+++\n\n{}\n", front_matter, request.body);
    let topic_path = topic_dir.join("topic.md");

    let tmp_path = topic_path.with_extension("md.tmp");
    tokio::fs::write(&tmp_path, &content).await?;
    tokio::fs::rename(&tmp_path, &topic_path).await?;

    // Index the topic.md itself
    let ingested = crate::ingest::ingest_markdown(
        settings,
        KnowledgeScope::SharedReference,
        None,
        &topic_path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &topic_id, &ingested.tags).await?;
    crate::storage::replace_links(pool, &topic_id, None, &ingested.links).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &topic_id,
        &request.title,
        "ReferenceTopic",
        None,
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(settings, embedder, pool, &ingested.chunks, &chunk_ids).await?;

    // Store reference_files mapping with role
    for (file_name, role) in &file_roles {
        let file_path = topic_dir.join(file_name);
        let file_content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => continue, // skip unreadable files (binary that slipped through)
        };

        let note_type = role.to_entry_type();
        let file_note_id = generate_note_id();
        // Use topic title as context prefix for chunk enrichment during initial import
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

/// Semantic search over reference topics (not their files).
pub(crate) async fn topic_search(
    engine: &KnowledgeEngine,
    query: &str,
) -> KnowledgeResult<Vec<TopicSearchResult>> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    // Find all ReferenceTopic note IDs
    let topic_ids = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE entry_type = 'ReferenceTopic' AND scope = 'shared_reference' AND owner_ghost IS NULL",
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

    // Dense search
    let dense_hits = dense_search(
        embedder,
        pool,
        query,
        settings.search.dense_limit,
        Some(&topic_ids),
        KnowledgeScope::SharedReference,
        "",
        None,
    )
    .await?;

    // Fuse and rank
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);

    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::SharedReference, "").await?;

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

/// List all reference topics.
pub(crate) async fn topic_list(
    engine: &KnowledgeEngine,
    _include_obsolete: bool,
) -> KnowledgeResult<Vec<TopicListEntry>> {
    let pool = engine.pool();

    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, title, created_by_ghost FROM notes WHERE entry_type = 'ReferenceTopic' AND scope = 'shared_reference' ORDER BY created_at DESC",
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

        // Load collection summaries: ReferenceCollection notes with parent_id = topic id
        let collection_rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, title FROM notes WHERE entry_type = 'ReferenceCollection' AND parent_id = ? ORDER BY title",
        )
        .bind(&id)
        .fetch_all(pool)
        .await?;

        let mut collections = Vec::new();
        for (coll_id, coll_title) in collection_rows {
            let coll_file_count = sqlx::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM reference_files WHERE topic_id = ? AND path LIKE ? || '%'",
            )
            .bind(&id)
            .bind(format!("{}%", coll_title))
            .fetch_one(pool)
            .await
            .map(|(c,)| c as usize)
            .unwrap_or(0);

            // Derive path from the collection note's filesystem path relative to topic
            let coll_path =
                sqlx::query_as::<_, (String,)>("SELECT path FROM notes WHERE id = ? LIMIT 1")
                    .bind(&coll_id)
                    .fetch_optional(pool)
                    .await?
                    .map(|(p,)| {
                        // Extract the subdirectory name from the path (parent of _index.md)
                        let path = std::path::Path::new(&p);
                        path.parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .unwrap_or_default();

            collections.push(CollectionSummary {
                title: coll_title,
                path: coll_path,
                file_count: coll_file_count,
            });
        }

        entries.push(TopicListEntry {
            topic_id: id,
            title,
            created_by_ghost: ghost,
            file_count,
            collections,
            tags,
        });
    }

    Ok(entries)
}

/// Update topic metadata without re-fetching sources.
pub(crate) async fn topic_update(
    engine: &KnowledgeEngine,
    _ghost_name: &str,
    request: TopicUpdateRequest,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    // Fetch the topic note directly from the reference scope
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT path, entry_type FROM notes WHERE id = ? AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(&request.topic_id)
    .fetch_optional(pool)
    .await?;

    let (path_str, entry_type) =
        row.ok_or_else(|| KnowledgeError::UnknownNote(request.topic_id.clone()))?;

    if entry_type != "ReferenceTopic" {
        return Err(KnowledgeError::AccessDenied(format!(
            "note '{}' is not a ReferenceTopic",
            request.topic_id
        )));
    }

    let doc_path = std::path::PathBuf::from(&path_str);

    // Read and parse existing file
    let raw = tokio::fs::read_to_string(&doc_path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let mut front = parsed.front;

    // Apply patches
    if let Some(tags) = &request.tags {
        front.tags = Some(tags.clone());
    }

    let body = request.body.as_deref().unwrap_or(&parsed.body);
    let front_toml = rebuild_front_matter(&front);
    let content = format!("+++\n{}\n+++\n\n{}\n", front_toml, body);

    // Atomic write
    let tmp_path = doc_path.with_extension("md.tmp");
    tokio::fs::write(&tmp_path, &content).await?;
    tokio::fs::rename(&tmp_path, &doc_path).await?;

    // Re-index the topic note
    let ingested = crate::ingest::ingest_markdown(
        engine.settings(),
        KnowledgeScope::SharedReference,
        None,
        &doc_path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &request.topic_id, &ingested.tags).await?;
    crate::storage::replace_links(pool, &request.topic_id, None, &ingested.links).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &request.topic_id,
        &front.title,
        "ReferenceTopic",
        None,
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(
        engine.settings(),
        engine.embedder(),
        pool,
        &ingested.chunks,
        &chunk_ids,
    )
    .await?;

    Ok(())
}

/// Load the 10 most recent reference topics for system prompt injection.
pub(crate) async fn recent_topics(
    pool: &SqlitePool,
) -> KnowledgeResult<Vec<(String, String, Vec<String>)>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE entry_type = 'ReferenceTopic' AND scope = 'shared_reference' ORDER BY created_at DESC LIMIT 10",
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

/// Build TOML front matter for a reference topic.
///
/// v2 topics are lean: just id, title, type, created_at, trust_score, tags, created_by.
/// Per-file provenance (source_url, fetched_at, max_age_days) is stored in the DB.
pub(crate) fn build_topic_front_matter(
    id: &str,
    title: &str,
    ghost_name: &str,
    model: &str,
    trust_score: i64,
    tags: Option<&[String]>,
    now: &chrono::DateTime<Utc>,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("id = \"{}\"", id));
    lines.push(format!("title = \"{}\"", title.replace('"', "\\\"")));
    lines.push("type = \"ReferenceTopic\"".to_string());
    lines.push(format!("created_at = \"{}\"", now.to_rfc3339()));
    lines.push(format!("trust_score = {}", trust_score));
    if let Some(tag_list) = tags {
        let formatted: Vec<String> = tag_list.iter().map(|t| format!("\"{}\"", t)).collect();
        lines.push(format!("tags = [{}]", formatted.join(", ")));
    }

    lines.push(String::new());
    lines.push("[created_by]".to_string());
    lines.push(format!("ghost = \"{}\"", ghost_name));
    lines.push(format!("model = \"{}\"", model));

    lines.join("\n")
}

//! Engine methods for reference topic management.
//!
//! Handles topic creation (two-phase with approval), searching,
//! listing, and metadata updates.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{
    KnowledgeContext, KnowledgeScope, TopicCreateRequest, TopicCreateResult, TopicListEntry,
    TopicSearchResult, TopicUpdateRequest, generate_note_id,
};
use crate::parser::TopicSource;
use crate::sources;

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
    context: &KnowledgeContext,
    request: TopicCreateRequest,
) -> KnowledgeResult<TopicCreateResult> {
    let settings = engine.settings();
    let pool = engine.pool();
    let embedder = engine.embedder();

    // Create topic directory
    let reference_root = crate::paths::reference_root(settings)?;
    let topic_dir_name = sanitize_filename(&request.title);
    let topic_dir = reference_root.join(&topic_dir_name);
    tokio::fs::create_dir_all(&topic_dir).await?;

    // Fetch all sources
    let fetched = sources::fetch_all_sources(&request.sources, &topic_dir).await?;

    // Collect files and topic sources
    let mut all_files: Vec<String> = Vec::new();
    let mut topic_sources: Vec<TopicSource> = Vec::new();
    for result in &fetched {
        all_files.extend(result.files.iter().cloned());
        topic_sources.push(result.source.clone());
    }

    // Write topic.md
    let topic_id = generate_note_id();
    let now = Utc::now();
    let trust_score = request.trust_score.unwrap_or(8);
    let max_age_days = request.max_age_days.unwrap_or(30);

    let front_matter = build_topic_front_matter(
        &topic_id,
        &request.title,
        &context.ghost_name,
        trust_score,
        request.tags.as_deref(),
        &topic_sources,
        &all_files,
        max_age_days,
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
        KnowledgeScope::Reference,
        None,
        &topic_path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &topic_id, &ingested.tags).await?;
    crate::storage::replace_links(pool, &topic_id, None, &ingested.links).await?;
    let chunk_ids =
        crate::storage::replace_chunks(pool, &topic_id, &request.title, "ReferenceTopic", &ingested.chunks)
            .await?;
    crate::index::embed_chunks(settings, embedder, pool, &ingested.chunks, &chunk_ids).await?;

    // Store reference_files mapping
    for file_name in &all_files {
        let file_path = topic_dir.join(file_name);
        let file_content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => continue, // skip unreadable files (binary that slipped through)
        };

        let file_note_id = generate_note_id();
        let file_ingested = crate::ingest::ingest_reference_file(
            settings,
            &file_path,
            &file_content,
            &file_note_id,
            file_name,
        )
        .await?;
        crate::storage::upsert_note(pool, &file_ingested.note).await?;
        crate::storage::replace_tags(pool, &file_note_id, &file_ingested.tags).await?;
        crate::storage::replace_links(pool, &file_note_id, None, &file_ingested.links).await?;
        let file_chunk_ids = crate::storage::replace_chunks(
            pool,
            &file_note_id,
            file_name,
            "ReferenceFile",
            &file_ingested.chunks,
        )
        .await?;
        crate::index::embed_chunks(settings, embedder, pool, &file_ingested.chunks, &file_chunk_ids)
            .await?;

        // Link file to topic
        sqlx::query(
            "INSERT OR REPLACE INTO reference_files (topic_id, note_id, path) VALUES (?, ?, ?)",
        )
        .bind(&topic_id)
        .bind(&file_note_id)
        .bind(file_name)
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
        "SELECT id FROM notes WHERE note_type = 'ReferenceTopic' AND scope = 'reference' AND owner_ghost IS NULL",
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
        KnowledgeScope::Reference,
        "",
    )
    .await?;

    // Fuse and rank
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);

    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::Reference, "").await?;

    // Convert to TopicSearchResult with staleness info
    let now = Utc::now();
    let mut results = Vec::new();
    for summary in summaries {
        let topic_meta = load_topic_meta(pool, &summary.id).await?;
        let status = topic_meta.status.clone();
        let is_stale = compute_staleness(&status, topic_meta.fetched_at, topic_meta.max_age_days, &now);

        results.push(TopicSearchResult {
            topic_id: summary.id.clone(),
            title: summary.title,
            status,
            is_stale,
            fetched_at: topic_meta.fetched_at,
            tags: load_topic_tags(pool, &summary.id).await?,
            score: summary.score,
            snippet: summary.snippet,
        });
    }

    Ok(results)
}

/// List all reference topics with staleness information.
pub(crate) async fn topic_list(
    engine: &KnowledgeEngine,
    include_obsolete: bool,
) -> KnowledgeResult<Vec<TopicListEntry>> {
    let pool = engine.pool();
    let now = Utc::now();

    let rows = if include_obsolete {
        sqlx::query_as::<_, (String, String, String, i64, String)>(
            "SELECT id, title, created_by_ghost, trust_score, path FROM notes WHERE note_type = 'ReferenceTopic' AND scope = 'reference' ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?
    } else {
        // Exclude obsolete — we check in Rust since status is in the file, not a column
        sqlx::query_as::<_, (String, String, String, i64, String)>(
            "SELECT id, title, created_by_ghost, trust_score, path FROM notes WHERE note_type = 'ReferenceTopic' AND scope = 'reference' ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await?
    };

    let mut entries = Vec::new();
    for (id, title, ghost, _trust, _path) in rows {
        let meta = load_topic_meta(pool, &id).await?;

        if !include_obsolete && meta.status == "obsolete" {
            continue;
        }

        let is_stale = compute_staleness(&meta.status, meta.fetched_at, meta.max_age_days, &now);

        let file_count = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM reference_files WHERE topic_id = ?",
        )
        .bind(&id)
        .fetch_one(pool)
        .await
        .map(|(c,)| c as usize)
        .unwrap_or(0);

        let tags = load_topic_tags(pool, &id).await?;

        let source_count = meta.source_count;

        entries.push(TopicListEntry {
            topic_id: id,
            title,
            status: meta.status,
            is_stale,
            fetched_at: meta.fetched_at,
            max_age_days: meta.max_age_days,
            created_by_ghost: ghost,
            source_count,
            file_count,
            tags,
        });
    }

    Ok(entries)
}

/// Update topic metadata without re-fetching sources.
pub(crate) async fn topic_update(
    engine: &KnowledgeEngine,
    _context: &KnowledgeContext,
    request: TopicUpdateRequest,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    // Fetch the topic note directly from the reference scope
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT path, note_type FROM notes WHERE id = ? AND scope = 'reference' LIMIT 1",
    )
    .bind(&request.topic_id)
    .fetch_optional(pool)
    .await?;

    let (path_str, note_type) = row.ok_or_else(|| {
        KnowledgeError::UnknownNote(request.topic_id.clone())
    })?;

    if note_type != "ReferenceTopic" {
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
    if let Some(status) = &request.status {
        front.status = Some(status.clone());
    }
    if let Some(max_age) = request.max_age_days {
        front.max_age_days = Some(max_age);
    }
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
        KnowledgeScope::Reference,
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
pub(crate) async fn recent_topics(pool: &SqlitePool) -> KnowledgeResult<Vec<(String, String, Vec<String>)>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE note_type = 'ReferenceTopic' AND scope = 'reference' ORDER BY created_at DESC LIMIT 10",
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

struct TopicMeta {
    status: String,
    fetched_at: Option<chrono::DateTime<Utc>>,
    max_age_days: i64,
    source_count: usize,
}

/// Load topic metadata from the file's front matter.
///
/// We read the topic.md file and parse its front matter to get status,
/// fetched_at, max_age_days, and source count.
async fn load_topic_meta(pool: &SqlitePool, topic_id: &str) -> KnowledgeResult<TopicMeta> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT path FROM notes WHERE id = ? LIMIT 1",
    )
    .bind(topic_id)
    .fetch_optional(pool)
    .await?;

    let path = match row {
        Some((p,)) => p,
        None => {
            return Ok(TopicMeta {
                status: "active".to_string(),
                fetched_at: None,
                max_age_days: 0,
                source_count: 0,
            });
        }
    };

    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(_) => {
            return Ok(TopicMeta {
                status: "active".to_string(),
                fetched_at: None,
                max_age_days: 0,
                source_count: 0,
            });
        }
    };

    match crate::parser::parse_note(&content) {
        Ok(parsed) => Ok(TopicMeta {
            status: parsed.front.status.unwrap_or_else(|| "active".to_string()),
            fetched_at: parsed.front.fetched_at,
            max_age_days: parsed.front.max_age_days.unwrap_or(0),
            source_count: parsed.front.sources.as_ref().map(|s| s.len()).unwrap_or(0),
        }),
        Err(_) => Ok(TopicMeta {
            status: "active".to_string(),
            fetched_at: None,
            max_age_days: 0,
            source_count: 0,
        }),
    }
}

async fn load_topic_tags(pool: &SqlitePool, topic_id: &str) -> KnowledgeResult<Vec<String>> {
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT tag FROM note_tags WHERE note_id = ?",
    )
    .bind(topic_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(t,)| t).collect())
}

fn compute_staleness(
    status: &str,
    fetched_at: Option<chrono::DateTime<Utc>>,
    max_age_days: i64,
    now: &chrono::DateTime<Utc>,
) -> bool {
    if status == "obsolete" {
        return true;
    }
    if max_age_days == 0 {
        return false;
    }
    match fetched_at {
        Some(fetched) => (*now - fetched).num_days() > max_age_days,
        None => false,
    }
}

/// Build TOML front matter for a reference topic.
#[allow(clippy::too_many_arguments)]
fn build_topic_front_matter(
    id: &str,
    title: &str,
    ghost_name: &str,
    trust_score: i64,
    tags: Option<&[String]>,
    sources: &[TopicSource],
    files: &[String],
    max_age_days: i64,
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
    lines.push("status = \"active\"".to_string());
    lines.push(format!("fetched_at = \"{}\"", now.to_rfc3339()));
    lines.push(format!("max_age_days = {}", max_age_days));
    if !files.is_empty() {
        let formatted: Vec<String> = files.iter().map(|f| format!("\"{}\"", f)).collect();
        lines.push(format!("files = [{}]", formatted.join(", ")));
    }

    lines.push(String::new());
    lines.push("[created_by]".to_string());
    lines.push(format!("ghost = \"{}\"", ghost_name));
    lines.push("model = \"tool\"".to_string());

    for src in sources {
        lines.push(String::new());
        lines.push("[[sources]]".to_string());
        lines.push(format!("type = \"{}\"", src.source_type));
        lines.push(format!("url = \"{}\"", src.url));
        if let Some(ref_name) = &src.ref_name {
            lines.push(format!("ref = \"{}\"", ref_name));
        }
        if let Some(commit) = &src.commit {
            lines.push(format!("commit = \"{}\"", commit));
        }
        if let Some(paths) = &src.paths {
            let formatted: Vec<String> = paths.iter().map(|p| format!("\"{}\"", p)).collect();
            lines.push(format!("paths = [{}]", formatted.join(", ")));
        }
    }

    lines.join("\n")
}

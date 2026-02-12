use sqlx::SqlitePool;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::graph::{load_links_in, load_links_out, load_parent, load_tags};
use crate::models::{
    KnowledgeScope, NoteDocument, NoteResult, ReferenceFileStatus, ReferenceQuery,
    ReferenceSearchResult, SearchOptions,
};

use super::KnowledgeEngine;
use super::search::{
    dense_search, hydrate_summaries, hydrate_summaries_boosted, rrf_fuse, sanitize_fts5_query,
};

/// Search within a reference topic's files, returning full topic context.
pub(crate) async fn reference_search(
    engine: &KnowledgeEngine,
    query: &ReferenceQuery,
) -> KnowledgeResult<ReferenceSearchResult> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    let topics = search_reference_topics(settings, embedder, pool, query).await?;
    let top_topic = topics.first().map(|result| result.summary.id.clone());

    if let Some(topic_id) = top_topic {
        let doc_boost = query.options.doc_boost.unwrap_or(settings.search.doc_boost);

        let results =
            search_reference_files(pool, settings, embedder, &topic_id, query, doc_boost).await?;

        // Fetch the topic note body for LLM context (topics are shared notes)
        let topic_doc =
            super::get::fetch_note(pool, &topic_id, KnowledgeScope::SharedNote, "").await?;
        let (topic_title, topic_body) = match topic_doc {
            Some(doc) => (doc.title, extract_topic_body(&doc.body)),
            None => (String::new(), String::new()),
        };

        return Ok(ReferenceSearchResult {
            topic_body,
            topic_title,
            topic_id,
            results,
        });
    }

    Ok(ReferenceSearchResult {
        topic_body: String::new(),
        topic_title: String::new(),
        topic_id: String::new(),
        results: Vec::new(),
    })
}

/// Extract just the body content from a topic.md file (strip front matter).
fn extract_topic_body(raw: &str) -> String {
    // The body stored in NoteDocument already has front matter stripped by fetch_note
    // (it reads the file then parse is done elsewhere). However fetch_note reads the
    // raw file. We need to strip the front matter.
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("+++") {
        return raw.to_string();
    }
    // Find the closing +++
    let after_first = &trimmed[3..];
    if let Some(pos) = after_first.find("\n+++") {
        let body_start = 3 + pos + 4; // skip past closing +++
        trimmed[body_start..].trim().to_string()
    } else {
        raw.to_string()
    }
}

/// Search files within a single topic, with doc_boost and status filtering.
async fn search_reference_files(
    pool: &SqlitePool,
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    topic_id: &str,
    query: &ReferenceQuery,
    doc_boost: f32,
) -> KnowledgeResult<Vec<NoteResult>> {
    // Fetch file note_ids, excluding obsolete files
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT note_id, status FROM reference_files WHERE topic_id = ? AND status != 'obsolete'",
    )
    .bind(topic_id)
    .fetch_all(pool)
    .await?;

    let note_ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
    let problematic_ids: Vec<String> = rows
        .iter()
        .filter(|(_, status)| status == "problematic")
        .map(|(id, _)| id.clone())
        .collect();

    if note_ids.is_empty() {
        return Ok(Vec::new());
    }

    let filter_ids = note_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

    let sql = format!(
        "SELECT chunk_id, bm25(chunk_fts) as score FROM chunk_fts JOIN notes ON notes.id = chunk_fts.note_id WHERE chunk_fts MATCH ? AND notes.id IN ({}) ORDER BY score ASC LIMIT ?",
        filter_ids
    );

    let safe_question = sanitize_fts5_query(&query.question);
    let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
    query_builder = query_builder.bind(&safe_question);
    for id in &note_ids {
        query_builder = query_builder.bind(id);
    }
    query_builder = query_builder.bind(settings.search.bm25_limit as i64);
    let bm25_hits = query_builder.fetch_all(pool).await?;

    let dense_hits = dense_search(
        embedder,
        pool,
        &query.question,
        settings.search.dense_limit,
        Some(&note_ids),
        KnowledgeScope::SharedReference,
        "",
        None,
    )
    .await?;
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);
    let summaries = hydrate_summaries_boosted(
        pool,
        &ranked,
        KnowledgeScope::SharedReference,
        "",
        doc_boost,
        &problematic_ids,
    )
    .await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = load_parent(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
        let links_out = load_links_out(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedReference,
            "",
        )
        .await?;
        let links_in = load_links_in(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedReference,
            "",
        )
        .await?;
        let tags = load_tags(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
        results.push(NoteResult {
            summary,
            parents,
            links_out,
            links_in,
            tags,
        });
    }

    Ok(results)
}

async fn search_reference_topics(
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &ReferenceQuery,
) -> KnowledgeResult<Vec<NoteResult>> {
    // Topics are shared notes that have reference files
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

    let placeholders = topic_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT chunk_id, bm25(chunk_fts) as score FROM chunk_fts JOIN notes ON notes.id = chunk_fts.note_id WHERE chunk_fts MATCH ? AND notes.id IN ({}) ORDER BY score ASC LIMIT ?",
        placeholders
    );
    let safe_topic = sanitize_fts5_query(&query.topic);
    let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
    query_builder = query_builder.bind(&safe_topic);
    for id in &topic_ids {
        query_builder = query_builder.bind(id);
    }
    query_builder = query_builder.bind(settings.search.bm25_limit as i64);
    let bm25_hits = query_builder.fetch_all(pool).await?;

    let dense_hits = dense_search(
        embedder,
        pool,
        &query.topic,
        settings.search.dense_limit,
        Some(&topic_ids),
        KnowledgeScope::SharedNote,
        "",
        None,
    )
    .await?;
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);
    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::SharedNote, "").await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = load_parent(pool, &summary.id, KnowledgeScope::SharedNote, "").await?;
        let links_out = load_links_out(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedNote,
            "",
        )
        .await?;
        let links_in = load_links_in(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedNote,
            "",
        )
        .await?;
        let tags = load_tags(pool, &summary.id, KnowledgeScope::SharedNote, "").await?;
        results.push(NoteResult {
            summary,
            parents,
            links_out,
            links_in,
            tags,
        });
    }

    Ok(results)
}

/// Search ALL non-obsolete reference files across all topics.
///
/// Unlike `search_reference_files` which is scoped to a single topic,
/// this searches the entire reference corpus. Used by the unified
/// `knowledge_search` when no specific topic is provided.
pub(crate) async fn search_all_reference_files(
    engine: &KnowledgeEngine,
    query_str: &str,
    options: &SearchOptions,
) -> KnowledgeResult<Vec<NoteResult>> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    // Fetch all non-obsolete reference file note_ids (excluding topic/collection notes)
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT note_id, status FROM reference_files WHERE status != 'obsolete'",
    )
    .fetch_all(pool)
    .await?;

    let note_ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
    let problematic_ids: Vec<String> = rows
        .iter()
        .filter(|(_, status)| status == "problematic")
        .map(|(id, _)| id.clone())
        .collect();

    if note_ids.is_empty() {
        return Ok(Vec::new());
    }

    let doc_boost = options.doc_boost.unwrap_or(settings.search.doc_boost);
    let bm25_limit = options.bm25_limit.unwrap_or(settings.search.bm25_limit);
    let dense_limit = options.dense_limit.unwrap_or(settings.search.dense_limit);
    let max_results = options.max_results.unwrap_or(settings.search.max_results);

    // BM25 search scoped to reference file notes
    let filter_ids = note_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT chunk_id, bm25(chunk_fts) as score FROM chunk_fts \
         JOIN notes ON notes.id = chunk_fts.note_id \
         WHERE chunk_fts MATCH ? AND notes.id IN ({}) \
         ORDER BY score ASC LIMIT ?",
        filter_ids
    );
    let safe_query = sanitize_fts5_query(query_str);
    let mut qb = sqlx::query_as::<_, (i64, f32)>(&sql);
    qb = qb.bind(&safe_query);
    for id in &note_ids {
        qb = qb.bind(id);
    }
    qb = qb.bind(bm25_limit as i64);
    let bm25_hits = qb.fetch_all(pool).await?;

    // Dense search
    let dense_hits = dense_search(
        embedder,
        pool,
        query_str,
        dense_limit,
        Some(&note_ids),
        KnowledgeScope::SharedReference,
        "",
        None,
    )
    .await?;

    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(max_results);

    let summaries = hydrate_summaries_boosted(
        pool,
        &ranked,
        KnowledgeScope::SharedReference,
        "",
        doc_boost,
        &problematic_ids,
    )
    .await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = load_parent(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
        let links_out = load_links_out(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedReference,
            "",
        )
        .await?;
        let links_in = load_links_in(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::SharedReference,
            "",
        )
        .await?;
        let tags = load_tags(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
        results.push(NoteResult {
            summary,
            parents,
            links_out,
            links_in,
            tags,
        });
    }

    Ok(results)
}

/// Set the status of a reference file (DB-only, no topic file modification).
pub(crate) async fn reference_file_set_status(
    engine: &KnowledgeEngine,
    note_id: &str,
    status: ReferenceFileStatus,
    _reason: Option<&str>,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    let updated = sqlx::query("UPDATE reference_files SET status = ? WHERE note_id = ?")
        .bind(status.as_str())
        .bind(note_id)
        .execute(pool)
        .await?;

    if updated.rows_affected() == 0 {
        return Err(KnowledgeError::UnknownNote(note_id.to_string()));
    }

    Ok(())
}

/// Get a reference file by note_id, or by topic + file_path.
pub(crate) async fn reference_get(
    engine: &KnowledgeEngine,
    note_id: Option<&str>,
    topic: Option<&str>,
    file_path: Option<&str>,
    max_chars: Option<usize>,
) -> KnowledgeResult<NoteDocument> {
    let pool = engine.pool();

    let resolved_note_id = if let Some(id) = note_id {
        id.to_string()
    } else if let (Some(topic_name), Some(path)) = (topic, file_path) {
        // Resolve topic → topic_id via search, then look up note_id by path
        let topic_id = resolve_topic_id(engine, topic_name).await?;
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT note_id FROM reference_files WHERE topic_id = ? AND path = ? LIMIT 1",
        )
        .bind(&topic_id)
        .bind(path)
        .fetch_optional(pool)
        .await?;

        match row {
            Some((id,)) => id,
            None => {
                return Err(KnowledgeError::UnknownNote(format!(
                    "file '{}' not found in topic '{}'",
                    path, topic_name
                )));
            }
        }
    } else {
        return Err(KnowledgeError::MissingField(
            "note_id or (topic + file_path)",
        ));
    };

    // Try SharedReference first (file notes), then SharedNote (topic notes)
    let doc =
        match super::get::fetch_note(pool, &resolved_note_id, KnowledgeScope::SharedReference, "")
            .await?
        {
            Some(d) => Some(d),
            None => {
                super::get::fetch_note(pool, &resolved_note_id, KnowledgeScope::SharedNote, "")
                    .await?
            }
        };
    match doc {
        Some(mut d) => {
            if let Some(limit) = max_chars
                && d.body.len() > limit
            {
                d.body = d.body.chars().take(limit).collect();
            }
            Ok(d)
        }
        None => Err(KnowledgeError::UnknownNote(resolved_note_id)),
    }
}

/// Summary of a recently saved reference file.
#[derive(Debug, Clone)]
pub struct RecentRefSummary {
    pub topic_title: String,
    pub path: String,
    pub source_url: Option<String>,
    pub fetched_at: String,
}

/// Get reference files saved since a given RFC3339 timestamp.
pub(crate) async fn recent_reference_files(
    engine: &KnowledgeEngine,
    since_rfc3339: &str,
) -> KnowledgeResult<Vec<RecentRefSummary>> {
    let pool = engine.pool();

    let rows = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        "SELECT n.title, rf.path, rf.source_url, rf.fetched_at
         FROM reference_files rf
         JOIN notes n ON n.id = rf.topic_id
         WHERE rf.fetched_at > ?
         ORDER BY rf.fetched_at DESC",
    )
    .bind(since_rfc3339)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(topic_title, path, source_url, fetched_at)| RecentRefSummary {
                topic_title,
                path,
                source_url,
                fetched_at,
            },
        )
        .collect())
}

/// Resolve a topic name to its topic_id by searching shared notes.
async fn resolve_topic_id(engine: &KnowledgeEngine, topic_name: &str) -> KnowledgeResult<String> {
    let pool = engine.pool();

    // Try exact title match first
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE title = ? AND scope = 'shared_note' LIMIT 1",
    )
    .bind(topic_name)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    // Fall back to case-insensitive LIKE match
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE title LIKE ? AND scope = 'shared_note' LIMIT 1",
    )
    .bind(format!("%{}%", topic_name))
    .fetch_optional(pool)
    .await?;

    match row {
        Some((id,)) => Ok(id),
        None => Err(KnowledgeError::UnknownNote(format!(
            "topic '{}' not found",
            topic_name
        ))),
    }
}

/// Move a reference file from one topic to another without touching the content.
///
/// Reads the raw file from disk, gets source metadata from the DB, saves to the
/// target topic via `reference_save`, then deletes the original. The content is
/// never exposed to the caller — it stays server-side.
pub(crate) async fn reference_file_move(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    note_id: &str,
    target_topic: &str,
    target_filename: Option<&str>,
    target_collection: Option<&str>,
) -> KnowledgeResult<crate::models::ReferenceSaveResult> {
    let pool = engine.pool();

    // 1. Get the note's file path and scope
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT path, scope FROM notes WHERE id = ? LIMIT 1")
            .bind(note_id)
            .fetch_optional(pool)
            .await?;

    let (disk_path, scope) = row.ok_or_else(|| KnowledgeError::UnknownNote(note_id.to_string()))?;

    if !scope.contains("reference") {
        return Err(KnowledgeError::AccessDenied(format!(
            "note '{}' is not a reference (scope={})",
            note_id, scope
        )));
    }

    // 2. Read file content from disk
    let content = tokio::fs::read_to_string(&disk_path).await.map_err(|e| {
        KnowledgeError::Io(std::io::Error::new(
            e.kind(),
            format!("reading reference file '{}': {}", disk_path, e),
        ))
    })?;

    // 3. Get source_url and role from reference_files table
    let meta = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT source_url, role FROM reference_files WHERE note_id = ? LIMIT 1",
    )
    .bind(note_id)
    .fetch_optional(pool)
    .await?;

    let (source_url, role_str) = meta.unwrap_or((None, "docs".to_string()));
    let role: crate::models::SourceRole =
        role_str.parse().unwrap_or(crate::models::SourceRole::Docs);

    // 4. Determine target filename — preserve original file extension
    let original_path = std::path::Path::new(&disk_path);
    let original_ext = original_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("md");
    let original_filename = original_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unnamed.md");

    let filename = match target_filename {
        Some(name) => {
            // Preserve original extension if the target doesn't specify one
            let target_path = std::path::Path::new(name);
            if target_path.extension().is_some() {
                name.to_string()
            } else {
                format!("{}.{}", name, original_ext)
            }
        }
        None => original_filename.to_string(),
    };

    // Build the save path (collection/filename or just filename)
    let save_path = match target_collection {
        Some(coll) => format!("{}/{}", coll, filename),
        None => filename,
    };

    // 5. Save to target topic
    let request = crate::models::ReferenceSaveRequest {
        topic: target_topic.to_string(),
        path: save_path,
        content,
        source_url,
        role: Some(role),
        title: None,
    };

    let result = super::save::reference_save(engine, ghost_name, model, request).await?;

    // 6. Delete the original
    reference_file_delete(engine, note_id).await?;

    Ok(result)
}

/// Delete a reference file by note ID.
///
/// Unlike `note_delete` which searches note/diary scopes only, this does a
/// direct lookup by ID in the `notes` table (scope-agnostic) and verifies
/// the entry is a reference type before deleting.
pub(crate) async fn reference_file_delete(
    engine: &KnowledgeEngine,
    note_id: &str,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    // Direct lookup — no scope filter, just find the row by ID
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT path, scope FROM notes WHERE id = ? LIMIT 1")
            .bind(note_id)
            .fetch_optional(pool)
            .await?;

    let (path, scope) = row.ok_or_else(|| KnowledgeError::UnknownNote(note_id.to_string()))?;

    if !scope.contains("reference") {
        return Err(KnowledgeError::AccessDenied(format!(
            "note '{}' is not a reference (scope={}), use note_write to delete notes",
            note_id, scope
        )));
    }

    // Delete file from disk
    let path = std::path::PathBuf::from(&path);
    if path.exists() {
        tokio::fs::remove_file(&path).await?;
    }

    // Delete from DB: chunks (FTS + vec), tags, links, reference_files, then notes
    let existing_chunk_ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .fetch_all(pool)
        .await?;

    sqlx::query("DELETE FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM chunk_fts WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    if !existing_chunk_ids.is_empty() {
        let placeholders = existing_chunk_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM chunk_vec WHERE rowid IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for (chunk_id,) in &existing_chunk_ids {
            q = q.bind(chunk_id);
        }
        q.execute(pool).await?;
    }
    sqlx::query("DELETE FROM note_tags WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM note_links WHERE source_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE note_links SET target_id = NULL WHERE target_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM reference_files WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;

    Ok(())
}

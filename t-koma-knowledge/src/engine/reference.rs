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
        let doc_boost = query
            .options
            .doc_boost
            .unwrap_or(settings.search.doc_boost);

        let results =
            search_reference_files(pool, settings, embedder, &topic_id, query, doc_boost).await?;

        // Fetch the topic note body for LLM context
        let topic_doc =
            super::get::fetch_note(pool, &topic_id, KnowledgeScope::SharedReference, "").await?;
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

    let filter_ids = note_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");

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
    let topic_ids = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE note_type = 'ReferenceTopic' AND scope = 'shared_reference' AND owner_ghost IS NULL",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id,)| id)
    .collect::<Vec<_>>();

    if topic_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = topic_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
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
        KnowledgeScope::SharedReference,
        "",
    )
    .await?;
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);
    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::SharedReference, "").await?;

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
        let parents =
            load_parent(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
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
        let tags =
            load_tags(pool, &summary.id, KnowledgeScope::SharedReference, "").await?;
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

/// Set the status of a reference file and optionally record a reason in the topic body.
pub(crate) async fn reference_file_set_status(
    engine: &KnowledgeEngine,
    note_id: &str,
    status: ReferenceFileStatus,
    reason: Option<&str>,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    // Update the status in reference_files
    let updated = sqlx::query("UPDATE reference_files SET status = ? WHERE note_id = ?")
        .bind(status.as_str())
        .bind(note_id)
        .execute(pool)
        .await?;

    if updated.rows_affected() == 0 {
        return Err(KnowledgeError::UnknownNote(note_id.to_string()));
    }

    // If marking as problematic or obsolete, append a warning to the topic body
    if matches!(
        status,
        ReferenceFileStatus::Problematic | ReferenceFileStatus::Obsolete
    ) && let Some(reason_text) = reason
    {
        // Find the topic_id for this file
        let topic_row = sqlx::query_as::<_, (String, String)>(
            "SELECT topic_id, path FROM reference_files WHERE note_id = ? LIMIT 1",
        )
        .bind(note_id)
        .fetch_optional(pool)
        .await?;

        if let Some((topic_id, file_path)) = topic_row {
            append_topic_warning(engine, &topic_id, &file_path, status, reason_text).await?;
        }
    }

    Ok(())
}

/// Append a warning line to a topic's body about a file status change.
async fn append_topic_warning(
    engine: &KnowledgeEngine,
    topic_id: &str,
    file_path: &str,
    status: ReferenceFileStatus,
    reason: &str,
) -> KnowledgeResult<()> {
    let pool = engine.pool();

    let row = sqlx::query_as::<_, (String,)>(
        "SELECT path FROM notes WHERE id = ? AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(topic_id)
    .fetch_optional(pool)
    .await?;

    let doc_path = match row {
        Some((p,)) => std::path::PathBuf::from(p),
        None => return Ok(()),
    };

    let raw = tokio::fs::read_to_string(&doc_path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let front = parsed.front;

    let warning = format!(
        "\n> **{}** `{}`: {}\n",
        status.as_str().to_uppercase(),
        file_path,
        reason
    );
    let new_body = format!("{}{}", parsed.body, warning);

    let front_toml = super::notes::rebuild_front_matter(&front);
    let content = format!("+++\n{}\n+++\n\n{}\n", front_toml, new_body);

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

    let doc =
        super::get::fetch_note(pool, &resolved_note_id, KnowledgeScope::SharedReference, "").await?;
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

/// Resolve a topic name to its topic_id by searching.
async fn resolve_topic_id(engine: &KnowledgeEngine, topic_name: &str) -> KnowledgeResult<String> {
    let pool = engine.pool();

    // Try exact title match first
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE title = ? AND note_type = 'ReferenceTopic' AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(topic_name)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = row {
        return Ok(id);
    }

    // Fall back to case-insensitive LIKE match
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM notes WHERE title LIKE ? AND note_type = 'ReferenceTopic' AND scope = 'shared_reference' LIMIT 1",
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

use sqlx::SqlitePool;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::graph::{load_links_in, load_links_out, load_parent, load_tags};
use crate::models::{KnowledgeScope, MemoryResult, ReferenceQuery};

use super::KnowledgeEngine;
use super::search::{dense_search, hydrate_summaries, rrf_fuse, sanitize_fts5_query};

pub(crate) async fn reference_search(
    engine: &KnowledgeEngine,
    query: &ReferenceQuery,
) -> KnowledgeResult<Vec<MemoryResult>> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    let topics = search_reference_topics(settings, embedder, pool, query).await?;
    let top_topic = topics.first().map(|result| result.summary.id.clone());

    if let Some(topic_id) = top_topic {
        return search_reference_files(pool, settings, embedder, &topic_id, query).await;
    }

    Ok(Vec::new())
}

async fn search_reference_files(
    pool: &SqlitePool,
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    topic_id: &str,
    query: &ReferenceQuery,
) -> KnowledgeResult<Vec<MemoryResult>> {
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT note_id FROM reference_files WHERE topic_id = ?",
    )
    .bind(topic_id)
    .fetch_all(pool)
    .await?;

    let note_ids: Vec<String> = rows.into_iter().map(|(id,)| id).collect();
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
        KnowledgeScope::Reference,
        "",
    )
    .await?;
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);
    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::Reference, "").await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = load_parent(pool, &summary.id, KnowledgeScope::Reference, "").await?;
        let links_out = load_links_out(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::Reference,
            "",
        )
        .await?;
        let links_in = load_links_in(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::Reference,
            "",
        )
        .await?;
        let tags = load_tags(pool, &summary.id, KnowledgeScope::Reference, "").await?;
        results.push(MemoryResult {
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
) -> KnowledgeResult<Vec<MemoryResult>> {
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
        KnowledgeScope::Reference,
        "",
    )
    .await?;
    let fused = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = fused.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(settings.search.max_results);
    let summaries = hydrate_summaries(pool, &ranked, KnowledgeScope::Reference, "").await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = load_parent(pool, &summary.id, KnowledgeScope::Reference, "").await?;
        let links_out = load_links_out(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::Reference,
            "",
        )
        .await?;
        let links_in = load_links_in(
            pool,
            &summary.id,
            settings.search.graph_max,
            KnowledgeScope::Reference,
            "",
        )
        .await?;
        let tags = load_tags(pool, &summary.id, KnowledgeScope::Reference, "").await?;
        results.push(MemoryResult {
            summary,
            parents,
            links_out,
            links_in,
            tags,
        });
    }

    Ok(results)
}

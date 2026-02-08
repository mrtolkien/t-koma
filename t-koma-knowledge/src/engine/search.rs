use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::graph::{load_links_in, load_links_out, load_parent, load_tags};
use crate::models::{
    DiaryQuery, DiarySearchResult, KnowledgeScope, NoteQuery, NoteResult, NoteSummary,
    OwnershipScope, SearchOptions,
};

pub(crate) async fn search_store(
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &NoteQuery,
    scope: KnowledgeScope,
    ghost_name: &str,
    archetype: Option<&str>,
) -> KnowledgeResult<Vec<NoteResult>> {
    let options = merge_options(settings, &query.options);
    let bm25_hits = bm25_search(
        pool,
        &query.query,
        options.bm25_limit,
        scope,
        ghost_name,
        archetype,
    )
    .await?;
    let dense_hits = dense_search(
        embedder,
        pool,
        &query.query,
        options.dense_limit,
        None,
        scope,
        ghost_name,
        archetype,
    )
    .await?;

    let rrf = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = rrf.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(options.max_results);

    let summaries = hydrate_summaries(pool, &ranked, scope, ghost_name).await?;

    let mut results = Vec::new();
    for summary in summaries {
        let parents = if options.graph_depth > 1 {
            crate::graph::expand_parents(pool, &summary.id, options.graph_depth, scope, ghost_name)
                .await?
        } else {
            load_parent(pool, &summary.id, scope, ghost_name).await?
        };
        let links_out = if options.graph_depth > 1 {
            crate::graph::expand_links_out(
                pool,
                &summary.id,
                options.graph_depth,
                options.graph_max,
                scope,
                ghost_name,
            )
            .await?
        } else {
            load_links_out(pool, &summary.id, options.graph_max, scope, ghost_name).await?
        };
        let links_in = if options.graph_depth > 1 {
            crate::graph::expand_links_in(
                pool,
                &summary.id,
                options.graph_depth,
                options.graph_max,
                scope,
                ghost_name,
            )
            .await?
        } else {
            load_links_in(pool, &summary.id, options.graph_max, scope, ghost_name).await?
        };
        let tags = load_tags(pool, &summary.id, scope, ghost_name).await?;
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

/// Search diary entries using the same hybrid BM25 + dense pipeline, scoped to GhostDiary.
pub(crate) async fn search_diary(
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &DiaryQuery,
    ghost_name: &str,
) -> KnowledgeResult<Vec<DiarySearchResult>> {
    let scope = KnowledgeScope::GhostDiary;
    let options = merge_options(settings, &query.options);
    let bm25_hits = bm25_search(
        pool,
        &query.query,
        options.bm25_limit,
        scope,
        ghost_name,
        None,
    )
    .await?;
    let dense_hits = dense_search(
        embedder,
        pool,
        &query.query,
        options.dense_limit,
        None,
        scope,
        ghost_name,
        None,
    )
    .await?;

    let rrf = rrf_fuse(settings.search.rrf_k, &bm25_hits, &dense_hits);
    let mut ranked: Vec<(i64, f32)> = rrf.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(options.max_results);

    let summaries = hydrate_summaries(pool, &ranked, scope, ghost_name).await?;

    Ok(summaries
        .into_iter()
        .map(|s| DiarySearchResult {
            date: s.title,
            score: s.score,
            snippet: s.snippet,
            note_id: s.id,
        })
        .collect())
}

/// Sanitize user input for FTS5 MATCH by quoting each word.
///
/// FTS5 has special operators (AND, OR, NOT, NEAR, quotes, etc.). Passing
/// raw user input can cause SQLite parse errors. We split on whitespace
/// and wrap each non-empty token in double quotes, joining with spaces
/// so FTS5 treats them as an implicit AND of literal terms.
pub(crate) fn sanitize_fts5_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if tokens.is_empty() {
        return "\"\"".to_string();
    }
    tokens.join(" ")
}

pub(crate) async fn bm25_search(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
    archetype: Option<&str>,
) -> KnowledgeResult<Vec<(i64, f32)>> {
    let safe_query = sanitize_fts5_query(query);
    let scope_value = scope.as_str();
    let archetype_clause = if archetype.is_some() {
        " AND notes.archetype = ?"
    } else {
        ""
    };

    let sql = if scope.is_shared() {
        format!(
            "SELECT chunk_id, bm25(chunk_fts) as score \
             FROM chunk_fts \
             JOIN notes ON notes.id = chunk_fts.note_id \
             WHERE chunk_fts MATCH ? AND notes.scope = ? AND notes.owner_ghost IS NULL{} \
             ORDER BY score ASC LIMIT ?",
            archetype_clause
        )
    } else {
        format!(
            "SELECT chunk_id, bm25(chunk_fts) as score \
             FROM chunk_fts \
             JOIN notes ON notes.id = chunk_fts.note_id \
             WHERE chunk_fts MATCH ? AND notes.scope = ? AND notes.owner_ghost = ?{} \
             ORDER BY score ASC LIMIT ?",
            archetype_clause
        )
    };

    let mut qb = sqlx::query_as::<_, (i64, f32)>(&sql);
    qb = qb.bind(&safe_query).bind(scope_value);
    if !scope.is_shared() {
        qb = qb.bind(ghost_name);
    }
    if let Some(arch) = archetype {
        qb = qb.bind(arch);
    }
    qb = qb.bind(limit as i64);

    Ok(qb.fetch_all(pool).await?)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn dense_search(
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    note_filter: Option<&[String]>,
    scope: KnowledgeScope,
    ghost_name: &str,
    archetype: Option<&str>,
) -> KnowledgeResult<Vec<(i64, f32)>> {
    let embeddings = embedder.embed_batch(&[query.to_string()]).await?;
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }
    let payload = serde_json::to_string(&embeddings[0])
        .map_err(|e| KnowledgeError::Embedding(format!("embedding serialize failed: {e}")))?;

    // KNN must run in a CTE with `k = ?` because vec0 cannot see LIMIT
    // through JOINs. We overfetch in the CTE, then filter+limit in the outer query.
    let knn_k = limit * 4; // overfetch to allow for scope/owner filtering

    let archetype_clause = if archetype.is_some() {
        " AND n.archetype = ?"
    } else {
        ""
    };

    let rows = if let Some(note_ids) = note_filter {
        let placeholders = note_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = if scope.is_shared() {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.id IN ({}) AND n.scope = ? AND n.owner_ghost IS NULL{} \
                 ORDER BY knn.distance ASC LIMIT ?",
                placeholders, archetype_clause
            )
        } else {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.id IN ({}) AND n.scope = ? AND n.owner_ghost = ?{} \
                 ORDER BY knn.distance ASC LIMIT ?",
                placeholders, archetype_clause
            )
        };
        let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
        query_builder = query_builder.bind(&payload).bind(knn_k as i64);
        for id in note_ids {
            query_builder = query_builder.bind(id);
        }
        query_builder = query_builder.bind(scope.as_str());
        if !scope.is_shared() {
            query_builder = query_builder.bind(ghost_name);
        }
        if let Some(arch) = archetype {
            query_builder = query_builder.bind(arch);
        }
        query_builder = query_builder.bind(limit as i64);
        query_builder.fetch_all(pool).await?
    } else {
        let sql = if scope.is_shared() {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.scope = ? AND n.owner_ghost IS NULL{} \
                 ORDER BY knn.distance ASC LIMIT ?",
                archetype_clause
            )
        } else {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.scope = ? AND n.owner_ghost = ?{} \
                 ORDER BY knn.distance ASC LIMIT ?",
                archetype_clause
            )
        };
        let mut qb = sqlx::query_as::<_, (i64, f32)>(&sql);
        qb = qb.bind(&payload).bind(knn_k as i64).bind(scope.as_str());
        if !scope.is_shared() {
            qb = qb.bind(ghost_name);
        }
        if let Some(arch) = archetype {
            qb = qb.bind(arch);
        }
        qb = qb.bind(limit as i64);
        qb.fetch_all(pool).await?
    };

    Ok(rows)
}

pub(crate) fn rrf_fuse(k: usize, a: &[(i64, f32)], b: &[(i64, f32)]) -> HashMap<i64, f32> {
    let mut scores: HashMap<i64, f32> = HashMap::new();

    for (idx, (chunk_id, _)) in a.iter().enumerate() {
        let rank = idx + 1;
        let score = 1.0 / (k as f32 + rank as f32);
        *scores.entry(*chunk_id).or_insert(0.0) += score;
    }

    for (idx, (chunk_id, _)) in b.iter().enumerate() {
        let rank = idx + 1;
        let score = 1.0 / (k as f32 + rank as f32);
        *scores.entry(*chunk_id).or_insert(0.0) += score;
    }

    scores
}

pub(crate) async fn hydrate_summaries(
    pool: &SqlitePool,
    ranked: &[(i64, f32)],
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    hydrate_summaries_boosted(pool, ranked, scope, ghost_name, 1.0, &[]).await
}

/// Hydrate summaries with doc_boost and problematic file penalties.
///
/// - `doc_boost`: multiplier applied to `ReferenceDocs` notes (1.0 = no boost)
/// - `problematic_ids`: note IDs with `problematic` status (get 0.5x penalty)
pub(crate) async fn hydrate_summaries_boosted(
    pool: &SqlitePool,
    ranked: &[(i64, f32)],
    scope: KnowledgeScope,
    ghost_name: &str,
    doc_boost: f32,
    problematic_ids: &[String],
) -> KnowledgeResult<Vec<NoteSummary>> {
    if ranked.is_empty() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();

    for (chunk_id, score) in ranked {
        let row = if scope.is_shared() {
            sqlx::query_as::<_, (String, String, String, Option<String>, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.entry_type, n.archetype, n.path, n.trust_score, n.scope, c.content
                   FROM chunks c
                   JOIN notes n ON n.id = c.note_id
                   WHERE c.id = ? AND n.scope = ? AND n.owner_ghost IS NULL
                   LIMIT 1"#,
            )
            .bind(chunk_id)
            .bind(scope.as_str())
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String, Option<String>, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.entry_type, n.archetype, n.path, n.trust_score, n.scope, c.content
                   FROM chunks c
                   JOIN notes n ON n.id = c.note_id
                   WHERE c.id = ? AND n.scope = ? AND n.owner_ghost = ?
                   LIMIT 1"#,
            )
            .bind(chunk_id)
            .bind(scope.as_str())
            .bind(ghost_name)
            .fetch_optional(pool)
            .await?
        };

        if let Some((id, title, entry_type, archetype, path, trust_score, scope, content)) = row {
            let snippet = content.chars().take(200).collect::<String>();
            let trust_boost = 1.0 + (trust_score as f32 / 20.0);
            let type_boost = match entry_type.as_str() {
                "ReferenceDocs" => doc_boost,
                _ => 1.0,
            };
            let status_factor = if problematic_ids.contains(&id) {
                0.5
            } else {
                1.0
            };
            summaries.push(NoteSummary {
                id,
                title,
                entry_type,
                archetype,
                path: path.into(),
                scope: scope.parse().unwrap_or(KnowledgeScope::SharedNote),
                trust_score,
                score: *score * trust_boost * type_boost * status_factor,
                snippet,
            });
        }
    }

    Ok(summaries)
}

/// Resolve ownership scope to note-only scopes (no diary, no references).
pub(crate) fn resolve_note_only_scopes(scope: &OwnershipScope) -> Vec<KnowledgeScope> {
    match scope {
        OwnershipScope::All => vec![KnowledgeScope::SharedNote, KnowledgeScope::GhostNote],
        OwnershipScope::Shared => vec![KnowledgeScope::SharedNote],
        OwnershipScope::Private => vec![KnowledgeScope::GhostNote],
    }
}

pub(crate) fn resolve_scopes(scope: &OwnershipScope) -> Vec<KnowledgeScope> {
    match scope {
        OwnershipScope::All => vec![
            KnowledgeScope::SharedNote,
            KnowledgeScope::GhostNote,
            KnowledgeScope::GhostDiary,
        ],
        OwnershipScope::Shared => vec![KnowledgeScope::SharedNote],
        OwnershipScope::Private => vec![KnowledgeScope::GhostNote, KnowledgeScope::GhostDiary],
    }
}

pub(crate) fn merge_options(
    settings: &KnowledgeSettings,
    overrides: &SearchOptions,
) -> ResolvedSearchOptions {
    ResolvedSearchOptions {
        max_results: overrides.max_results.unwrap_or(settings.search.max_results),
        graph_depth: overrides.graph_depth.unwrap_or(settings.search.graph_depth),
        graph_max: overrides.graph_max.unwrap_or(settings.search.graph_max),
        bm25_limit: overrides.bm25_limit.unwrap_or(settings.search.bm25_limit),
        dense_limit: overrides.dense_limit.unwrap_or(settings.search.dense_limit),
    }
}

pub(crate) struct ResolvedSearchOptions {
    pub(crate) max_results: usize,
    pub(crate) graph_depth: u8,
    pub(crate) graph_max: usize,
    pub(crate) bm25_limit: usize,
    pub(crate) dense_limit: usize,
}

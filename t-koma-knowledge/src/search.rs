use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::config::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::graph::{load_links_in, load_links_out, load_parent, load_tags};
use crate::index::{reconcile_ghost, reconcile_shared};
use crate::models::{
    KnowledgeContext, KnowledgeScope, MemoryQuery, MemoryResult, MemoryScope, NoteDocument,
    NoteSummary, ReferenceQuery, SearchOptions,
};
use crate::paths::{knowledge_db_path, shared_inbox_path, ghost_inbox_path};
use crate::storage::KnowledgeStore;

#[derive(Debug, Clone)]
pub struct KnowledgeEngine {
    settings: KnowledgeSettings,
    embedder: EmbeddingClient,
}

impl KnowledgeEngine {
    pub fn new(settings: KnowledgeSettings) -> Self {
        let embedder = EmbeddingClient::new(&settings);
        Self { settings, embedder }
    }

    pub async fn memory_search(
        &self,
        context: &KnowledgeContext,
        query: MemoryQuery,
    ) -> KnowledgeResult<Vec<MemoryResult>> {
        let mut results = Vec::new();

        let store = self.store().await?;
        let scopes = resolve_scopes(&query.scope);
        for scope in scopes {
            self.maybe_reconcile(context, scope, &store).await?;
            let partial = search_store(
                &self.settings,
                &self.embedder,
                store.pool(),
                &query,
                scope,
                &context.ghost_name,
            )
            .await?;
            results.extend(partial);
        }

        let max_results = query
            .options
            .max_results
            .unwrap_or(self.settings.search.max_results);
        results.sort_by(|a, b| b.summary.score.partial_cmp(&a.summary.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);

        Ok(results)
    }

    pub async fn memory_get(
        &self,
        context: &KnowledgeContext,
        note_id_or_title: &str,
        scope: MemoryScope,
    ) -> KnowledgeResult<NoteDocument> {
        let store = self.store().await?;
        let scopes = resolve_scopes(&scope);
        for scope in scopes {
            let doc = fetch_note(
                store.pool(),
                note_id_or_title,
                scope,
                &context.ghost_name,
            )
            .await?;
            if let Some(doc) = doc {
                return Ok(doc);
            }
        }

        Err(KnowledgeError::UnknownNote(note_id_or_title.to_string()))
    }

    pub async fn memory_capture(
        &self,
        context: &KnowledgeContext,
        payload: &str,
        scope: MemoryScope,
    ) -> KnowledgeResult<String> {
        let target_path = match scope {
            MemoryScope::SharedOnly => shared_inbox_path(&self.settings)?,
            _ => ghost_inbox_path(&context.workspace_root),
        };
        tokio::fs::create_dir_all(&target_path).await?;

        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let file_name = format!("inbox-{}.md", timestamp);
        let path = target_path.join(file_name);
        tokio::fs::write(&path, payload).await?;

        Ok(path.to_string_lossy().to_string())
    }

    pub async fn reference_search(
        &self,
        context: &KnowledgeContext,
        query: ReferenceQuery,
    ) -> KnowledgeResult<Vec<MemoryResult>> {
        let store = self.store().await?;
        self.maybe_reconcile(context, KnowledgeScope::Reference, &store).await?;

        let topics = search_reference_topics(&self.settings, &self.embedder, store.pool(), &query).await?;
        let top_topic = topics.first().map(|result| result.summary.id.clone());

        if let Some(topic_id) = top_topic {
            return search_reference_files(store.pool(), &self.settings, &self.embedder, &topic_id, &query).await;
        }

        Ok(Vec::new())
    }

    async fn store(&self) -> KnowledgeResult<KnowledgeStore> {
        let path = knowledge_db_path(&self.settings)?;
        KnowledgeStore::open(&path, self.settings.embedding_dim).await
    }

    async fn maybe_reconcile(
        &self,
        context: &KnowledgeContext,
        scope: KnowledgeScope,
        store: &KnowledgeStore,
    ) -> KnowledgeResult<()> {
        let key = match scope {
            KnowledgeScope::Shared | KnowledgeScope::Reference => "last_reconcile_shared".to_string(),
            _ => format!("last_reconcile_ghost:{}", context.ghost_name),
        };
        let last: Option<(String,)> = sqlx::query_as(
            "SELECT value FROM meta WHERE key = ? LIMIT 1",
        )
        .bind(&key)
        .fetch_optional(store.pool())
        .await?;
        let now = Utc::now();
        let should_run = match last {
            Some((value,)) => DateTime::parse_from_rfc3339(&value)
                .map(|dt| (now - dt.with_timezone(&Utc)).num_seconds() as u64 > self.settings.reconcile_seconds)
                .unwrap_or(true),
            None => true,
        };

        if should_run {
            match scope {
                KnowledgeScope::Shared | KnowledgeScope::Reference => {
                    reconcile_shared(&self.settings, store.pool(), &self.embedder).await?;
                }
                _ => {
                    reconcile_ghost(
                        &self.settings,
                        store.pool(),
                        &self.embedder,
                        &context.workspace_root,
                        &context.ghost_name,
                    )
                    .await?;
                }
            }
            sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(now.to_rfc3339())
                .execute(store.pool())
                .await?;
        }

        Ok(())
    }
}

async fn search_store(
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &MemoryQuery,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<MemoryResult>> {
    let options = merge_options(settings, &query.options);
    let bm25_hits = bm25_search(pool, &query.query, options.bm25_limit, scope, ghost_name).await?;
    let dense_hits = dense_search(
        embedder,
        pool,
        &query.query,
        options.dense_limit,
        None,
        scope,
        ghost_name,
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
            crate::graph::expand_parents(pool, &summary.id, options.graph_depth, scope, ghost_name).await?
        } else {
            load_parent(pool, &summary.id, scope, ghost_name).await?
        };
        let links_out = if options.graph_depth > 1 {
            crate::graph::expand_links_out(pool, &summary.id, options.graph_depth, options.graph_max, scope, ghost_name).await?
        } else {
            load_links_out(pool, &summary.id, options.graph_max, scope, ghost_name).await?
        };
        let links_in = if options.graph_depth > 1 {
            crate::graph::expand_links_in(pool, &summary.id, options.graph_depth, options.graph_max, scope, ghost_name).await?
        } else {
            load_links_in(pool, &summary.id, options.graph_max, scope, ghost_name).await?
        };
        let tags = load_tags(pool, &summary.id, scope, ghost_name).await?;
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

async fn bm25_search(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<(i64, f32)>> {
    let scope_value = scope_string(scope);
    let rows = if is_shared_scope(scope) {
        sqlx::query_as::<_, (i64, f32)>(
            r#"SELECT chunk_id, bm25(chunk_fts) as score
               FROM chunk_fts
               JOIN notes ON notes.id = chunk_fts.note_id
               WHERE chunk_fts MATCH ? AND notes.scope = ? AND notes.owner_ghost IS NULL
               ORDER BY score ASC
               LIMIT ?"#,
        )
        .bind(query)
        .bind(scope_value)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (i64, f32)>(
            r#"SELECT chunk_id, bm25(chunk_fts) as score
               FROM chunk_fts
               JOIN notes ON notes.id = chunk_fts.note_id
               WHERE chunk_fts MATCH ? AND notes.scope = ? AND notes.owner_ghost = ?
               ORDER BY score ASC
               LIMIT ?"#,
        )
        .bind(query)
        .bind(scope_value)
        .bind(ghost_name)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}

async fn dense_search(
    embedder: &EmbeddingClient,
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    note_filter: Option<&[String]>,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<(i64, f32)>> {
    let embeddings = embedder.embed_batch(&[query.to_string()]).await?;
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }
    let payload = serde_json::to_string(&embeddings[0]).map_err(|e| {
        KnowledgeError::InvalidFrontMatter(format!("embedding serialize failed: {e}"))
    })?;

    let rows = if let Some(note_ids) = note_filter {
        let placeholders = note_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = if is_shared_scope(scope) {
            format!(
                "SELECT c.id, v.distance FROM chunk_vec v JOIN chunks c ON c.id = v.rowid JOIN notes n ON n.id = c.note_id WHERE v.embedding MATCH ? AND n.id IN ({}) AND n.scope = ? AND n.owner_ghost IS NULL ORDER BY v.distance ASC LIMIT ?",
                placeholders
            )
        } else {
            format!(
                "SELECT c.id, v.distance FROM chunk_vec v JOIN chunks c ON c.id = v.rowid JOIN notes n ON n.id = c.note_id WHERE v.embedding MATCH ? AND n.id IN ({}) AND n.scope = ? AND n.owner_ghost = ? ORDER BY v.distance ASC LIMIT ?",
                placeholders
            )
        };
        let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
        query_builder = query_builder.bind(payload);
        for id in note_ids {
            query_builder = query_builder.bind(id);
        }
        query_builder = query_builder.bind(scope_string(scope));
        if !is_shared_scope(scope) {
            query_builder = query_builder.bind(ghost_name);
        }
        query_builder = query_builder.bind(limit as i64);
        query_builder.fetch_all(pool).await?
    } else if is_shared_scope(scope) {
        sqlx::query_as::<_, (i64, f32)>(
            r#"SELECT c.id, v.distance
               FROM chunk_vec v
               JOIN chunks c ON c.id = v.rowid
               JOIN notes n ON n.id = c.note_id
               WHERE v.embedding MATCH ? AND n.scope = ? AND n.owner_ghost IS NULL
               ORDER BY v.distance ASC
               LIMIT ?"#,
        )
        .bind(payload)
        .bind(scope_string(scope))
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (i64, f32)>(
            r#"SELECT c.id, v.distance
               FROM chunk_vec v
               JOIN chunks c ON c.id = v.rowid
               JOIN notes n ON n.id = c.note_id
               WHERE v.embedding MATCH ? AND n.scope = ? AND n.owner_ghost = ?
               ORDER BY v.distance ASC
               LIMIT ?"#,
        )
        .bind(payload)
        .bind(scope_string(scope))
        .bind(ghost_name)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}

fn rrf_fuse(k: usize, a: &[(i64, f32)], b: &[(i64, f32)]) -> HashMap<i64, f32> {
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

async fn hydrate_summaries(
    pool: &SqlitePool,
    ranked: &[(i64, f32)],
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    if ranked.is_empty() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();

    for (chunk_id, score) in ranked {
        let row = if is_shared_scope(scope) {
            sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.note_type, n.path, n.trust_score, n.scope, c.content
                   FROM chunks c
                   JOIN notes n ON n.id = c.note_id
                   WHERE c.id = ? AND n.scope = ? AND n.owner_ghost IS NULL
                   LIMIT 1"#,
            )
            .bind(chunk_id)
            .bind(scope_string(scope))
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.note_type, n.path, n.trust_score, n.scope, c.content
                   FROM chunks c
                   JOIN notes n ON n.id = c.note_id
                   WHERE c.id = ? AND n.scope = ? AND n.owner_ghost = ?
                   LIMIT 1"#,
            )
            .bind(chunk_id)
            .bind(scope_string(scope))
            .bind(ghost_name)
            .fetch_optional(pool)
            .await?
        };

        if let Some((id, title, note_type, path, trust_score, scope, content)) = row {
            let snippet = content.chars().take(200).collect::<String>();
            let trust_boost = 1.0 + (trust_score as f32 / 20.0);
            summaries.push(NoteSummary {
                id,
                title,
                note_type,
                path: path.into(),
                scope: scope_from_str(&scope),
                trust_score,
                score: *score * trust_boost,
                snippet,
            });
        }
    }

    Ok(summaries)
}

async fn fetch_note(
    pool: &SqlitePool,
    note_id_or_title: &str,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Option<NoteDocument>> {
    let row = if is_shared_scope(scope) {
        sqlx::query_as::<_, (
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
    )>(
            r#"SELECT id, title, note_type, path, scope, trust_score, created_at, created_by_ghost,
                      created_by_model, last_validated_at, last_validated_by_ghost, last_validated_by_model,
                      version, parent_id, comments_json
               FROM notes
               WHERE (id = ? OR title = ?) AND scope = ? AND owner_ghost IS NULL
               LIMIT 1"#,
        )
        .bind(note_id_or_title)
        .bind(note_id_or_title)
        .bind(scope_string(scope))
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, (
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
    )>(
            r#"SELECT id, title, note_type, path, scope, trust_score, created_at, created_by_ghost,
                      created_by_model, last_validated_at, last_validated_by_ghost, last_validated_by_model,
                      version, parent_id, comments_json
               FROM notes
               WHERE (id = ? OR title = ?) AND scope = ? AND owner_ghost = ?
               LIMIT 1"#,
        )
        .bind(note_id_or_title)
        .bind(note_id_or_title)
        .bind(scope_string(scope))
        .bind(ghost_name)
        .fetch_optional(pool)
        .await?
    };

    if let Some((
        id,
        title,
        note_type,
        path,
        scope,
        trust_score,
        created_at,
        created_by_ghost,
        created_by_model,
        last_validated_at,
        last_validated_by_ghost,
        last_validated_by_model,
        version,
        parent_id,
        comments_json,
    )) = row
    {
        let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        return Ok(Some(NoteDocument {
            id,
            title,
            note_type,
            path: path.into(),
            scope: scope_from_str(&scope),
            trust_score,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            created_by_ghost,
            created_by_model,
            last_validated_at: last_validated_at
                .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_validated_by_ghost,
            last_validated_by_model,
            version,
            parent_id,
            comments_json,
            body,
        }));
    }

    Ok(None)
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

    let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
    query_builder = query_builder.bind(&query.question);
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
        let links_out = load_links_out(pool, &summary.id, settings.search.graph_max, KnowledgeScope::Reference, "").await?;
        let links_in = load_links_in(pool, &summary.id, settings.search.graph_max, KnowledgeScope::Reference, "").await?;
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

    let placeholders = topic_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT chunk_id, bm25(chunk_fts) as score FROM chunk_fts JOIN notes ON notes.id = chunk_fts.note_id WHERE chunk_fts MATCH ? AND notes.id IN ({}) ORDER BY score ASC LIMIT ?",
        placeholders
    );
    let mut query_builder = sqlx::query_as::<_, (i64, f32)>(&sql);
    query_builder = query_builder.bind(&query.topic);
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
        let links_out =
            load_links_out(pool, &summary.id, settings.search.graph_max, KnowledgeScope::Reference, "").await?;
        let links_in =
            load_links_in(pool, &summary.id, settings.search.graph_max, KnowledgeScope::Reference, "").await?;
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

fn resolve_scopes(scope: &MemoryScope) -> Vec<KnowledgeScope> {
    match scope {
        MemoryScope::All => vec![
            KnowledgeScope::Shared,
            KnowledgeScope::GhostPrivate,
            KnowledgeScope::GhostProjects,
            KnowledgeScope::GhostDiary,
        ],
        MemoryScope::SharedOnly => vec![KnowledgeScope::Shared],
        MemoryScope::GhostOnly => vec![
            KnowledgeScope::GhostPrivate,
            KnowledgeScope::GhostProjects,
            KnowledgeScope::GhostDiary,
        ],
        MemoryScope::GhostPrivate => vec![KnowledgeScope::GhostPrivate],
        MemoryScope::GhostProjects => vec![KnowledgeScope::GhostProjects],
        MemoryScope::GhostDiary => vec![KnowledgeScope::GhostDiary],
    }
}

fn merge_options(settings: &KnowledgeSettings, overrides: &SearchOptions) -> ResolvedSearchOptions {
    ResolvedSearchOptions {
        max_results: overrides.max_results.unwrap_or(settings.search.max_results),
        graph_depth: overrides.graph_depth.unwrap_or(settings.search.graph_depth),
        graph_max: overrides.graph_max.unwrap_or(settings.search.graph_max),
        bm25_limit: overrides.bm25_limit.unwrap_or(settings.search.bm25_limit),
        dense_limit: overrides.dense_limit.unwrap_or(settings.search.dense_limit),
    }
}

struct ResolvedSearchOptions {
    max_results: usize,
    graph_depth: u8,
    graph_max: usize,
    bm25_limit: usize,
    dense_limit: usize,
}

fn scope_string(scope: KnowledgeScope) -> String {
    match scope {
        KnowledgeScope::Shared => "shared",
        KnowledgeScope::GhostPrivate => "ghost_private",
        KnowledgeScope::GhostProjects => "ghost_projects",
        KnowledgeScope::GhostDiary => "ghost_diary",
        KnowledgeScope::Reference => "reference",
    }
    .to_string()
}

fn is_shared_scope(scope: KnowledgeScope) -> bool {
    matches!(scope, KnowledgeScope::Shared | KnowledgeScope::Reference)
}

fn scope_from_str(scope: &str) -> KnowledgeScope {
    match scope {
        "shared" => KnowledgeScope::Shared,
        "ghost_private" => KnowledgeScope::GhostPrivate,
        "ghost_projects" => KnowledgeScope::GhostProjects,
        "ghost_diary" => KnowledgeScope::GhostDiary,
        "reference" => KnowledgeScope::Reference,
        _ => KnowledgeScope::Shared,
    }
}

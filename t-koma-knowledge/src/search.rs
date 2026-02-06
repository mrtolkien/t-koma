use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::config::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::graph::{load_links_in, load_links_out, load_parent, load_tags};
use crate::index::{reconcile_ghost, reconcile_shared};
use crate::models::{
    KnowledgeContext, KnowledgeScope, MemoryQuery, MemoryResult, MemoryScope, NoteCreateRequest,
    NoteDocument, NoteSummary, NoteUpdateRequest, NoteWriteResult, ReferenceQuery, SearchOptions,
    generate_note_id,
};
use crate::parser::CommentEntry;
use crate::paths::{
    ghost_diary_root, ghost_inbox_path, ghost_private_root, ghost_projects_root,
    knowledge_db_path, shared_inbox_path, shared_knowledge_root,
};
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

    /// Create a structured note with validated front matter.
    pub async fn note_create(
        &self,
        context: &KnowledgeContext,
        request: NoteCreateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        let note_id = generate_note_id();
        let now = Utc::now();
        let (target_dir, scope, owner_ghost) =
            resolve_write_target(context, &self.settings, &request.scope)?;
        tokio::fs::create_dir_all(&target_dir).await?;

        let trust_score = request.trust_score.unwrap_or(5);
        let front_matter = build_front_matter(
            &note_id,
            &request.title,
            &request.note_type,
            &context.ghost_name,
            trust_score,
            request.parent.as_deref(),
            request.tags.as_deref(),
            request.source.as_deref(),
            &now,
        );

        let content = format!("+++\n{}\n+++\n\n{}\n", front_matter, request.body);
        let file_name = sanitize_filename(&request.title);
        let path = target_dir.join(format!("{}.md", file_name));

        // Atomic write: write to tmp then rename
        let tmp_path = path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, &content).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        // Index inline
        let ingested = crate::ingest::ingest_markdown(
            &self.settings,
            scope,
            owner_ghost,
            &path,
            &content,
        )
        .await?;
        let store = self.store().await?;
        crate::storage::upsert_note(store.pool(), &ingested.note).await?;
        crate::storage::replace_tags(store.pool(), &note_id, &ingested.tags).await?;
        crate::storage::replace_links(
            store.pool(),
            &note_id,
            ingested.note.owner_ghost.as_deref(),
            &ingested.links,
        )
        .await?;
        let chunk_ids = crate::storage::replace_chunks(
            store.pool(),
            &note_id,
            &ingested.note.title,
            &ingested.note.note_type,
            &ingested.chunks,
        )
        .await?;
        crate::index::embed_chunks(&self.settings, &self.embedder, store.pool(), &ingested.chunks, &chunk_ids).await?;

        Ok(NoteWriteResult {
            note_id,
            path,
        })
    }

    /// Update an existing note (title, body, tags, trust, parent).
    pub async fn note_update(
        &self,
        context: &KnowledgeContext,
        request: NoteUpdateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        let store = self.store().await?;

        // Fetch existing note and verify access
        let doc = self
            .memory_get(context, &request.note_id, MemoryScope::All)
            .await?;
        verify_write_access(context, &doc)?;

        // Read existing file
        let raw = tokio::fs::read_to_string(&doc.path).await?;
        let parsed = crate::parser::parse_note(&raw)?;
        let mut front = parsed.front;

        // Apply patches
        if let Some(title) = &request.title {
            front.title = title.clone();
        }
        if let Some(tags) = &request.tags {
            front.tags = Some(tags.clone());
        }
        if let Some(trust) = request.trust_score {
            front.trust_score = trust;
        }
        if let Some(parent) = &request.parent {
            front.parent = Some(parent.clone());
        }
        front.version = Some(front.version.unwrap_or(1) + 1);

        let body = request.body.as_deref().unwrap_or(&parsed.body);
        let front_toml = rebuild_front_matter(&front);
        let content = format!("+++\n{}\n+++\n\n{}\n", front_toml, body);

        // Atomic write
        let tmp_path = doc.path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, &content).await?;
        tokio::fs::rename(&tmp_path, &doc.path).await?;

        // Re-index
        let scope = doc.scope;
        let owner_ghost = if scope.is_shared() {
            None
        } else {
            Some(context.ghost_name.clone())
        };
        let ingested = crate::ingest::ingest_markdown(
            &self.settings,
            scope,
            owner_ghost,
            &doc.path,
            &content,
        )
        .await?;
        crate::storage::upsert_note(store.pool(), &ingested.note).await?;
        crate::storage::replace_tags(store.pool(), &request.note_id, &ingested.tags).await?;
        crate::storage::replace_links(
            store.pool(),
            &request.note_id,
            ingested.note.owner_ghost.as_deref(),
            &ingested.links,
        )
        .await?;
        let chunk_ids = crate::storage::replace_chunks(
            store.pool(),
            &request.note_id,
            &ingested.note.title,
            &ingested.note.note_type,
            &ingested.chunks,
        )
        .await?;
        crate::index::embed_chunks(&self.settings, &self.embedder, store.pool(), &ingested.chunks, &chunk_ids).await?;

        Ok(NoteWriteResult {
            note_id: request.note_id,
            path: doc.path,
        })
    }

    /// Record validation metadata and optionally adjust trust score.
    pub async fn note_validate(
        &self,
        context: &KnowledgeContext,
        note_id: &str,
        trust_score: Option<i64>,
    ) -> KnowledgeResult<NoteWriteResult> {
        let doc = self
            .memory_get(context, note_id, MemoryScope::All)
            .await?;
        verify_write_access(context, &doc)?;

        let raw = tokio::fs::read_to_string(&doc.path).await?;
        let parsed = crate::parser::parse_note(&raw)?;
        let mut front = parsed.front;

        let now = Utc::now();
        front.last_validated_at = Some(now);
        front.last_validated_by = Some(crate::parser::CreatedBy {
            ghost: context.ghost_name.clone(),
            model: "tool".to_string(),
        });
        if let Some(score) = trust_score {
            front.trust_score = score;
        }

        let front_toml = rebuild_front_matter(&front);
        let content = format!("+++\n{}\n+++\n\n{}\n", front_toml, parsed.body);

        let tmp_path = doc.path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, &content).await?;
        tokio::fs::rename(&tmp_path, &doc.path).await?;

        // Update DB record
        let store = self.store().await?;
        let scope = doc.scope;
        let owner_ghost = if scope.is_shared() {
            None
        } else {
            Some(context.ghost_name.clone())
        };
        let ingested = crate::ingest::ingest_markdown(
            &self.settings,
            scope,
            owner_ghost,
            &doc.path,
            &content,
        )
        .await?;
        crate::storage::upsert_note(store.pool(), &ingested.note).await?;

        Ok(NoteWriteResult {
            note_id: note_id.to_string(),
            path: doc.path,
        })
    }

    /// Append a comment entry to a note's front matter.
    pub async fn note_comment(
        &self,
        context: &KnowledgeContext,
        note_id: &str,
        text: &str,
    ) -> KnowledgeResult<NoteWriteResult> {
        let doc = self
            .memory_get(context, note_id, MemoryScope::All)
            .await?;
        verify_write_access(context, &doc)?;

        let raw = tokio::fs::read_to_string(&doc.path).await?;
        let parsed = crate::parser::parse_note(&raw)?;
        let mut front = parsed.front;

        let comment = CommentEntry {
            ghost: context.ghost_name.clone(),
            model: "tool".to_string(),
            at: Utc::now(),
            text: text.to_string(),
        };
        let comments = front.comments.get_or_insert_with(Vec::new);
        comments.push(comment);

        let front_toml = rebuild_front_matter(&front);
        let content = format!("+++\n{}\n+++\n\n{}\n", front_toml, parsed.body);

        let tmp_path = doc.path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, &content).await?;
        tokio::fs::rename(&tmp_path, &doc.path).await?;

        // Update DB
        let store = self.store().await?;
        let scope = doc.scope;
        let owner_ghost = if scope.is_shared() {
            None
        } else {
            Some(context.ghost_name.clone())
        };
        let ingested = crate::ingest::ingest_markdown(
            &self.settings,
            scope,
            owner_ghost,
            &doc.path,
            &content,
        )
        .await?;
        crate::storage::upsert_note(store.pool(), &ingested.note).await?;

        Ok(NoteWriteResult {
            note_id: note_id.to_string(),
            path: doc.path,
        })
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

/// Sanitize user input for FTS5 MATCH by quoting each word.
///
/// FTS5 has special operators (AND, OR, NOT, NEAR, quotes, etc.). Passing
/// raw user input can cause SQLite parse errors. We split on whitespace
/// and wrap each non-empty token in double quotes, joining with spaces
/// so FTS5 treats them as an implicit AND of literal terms.
fn sanitize_fts5_query(raw: &str) -> String {
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

async fn bm25_search(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<(i64, f32)>> {
    let safe_query = sanitize_fts5_query(query);
    let scope_value = scope.as_str();
    let rows = if scope.is_shared() {
        sqlx::query_as::<_, (i64, f32)>(
            r#"SELECT chunk_id, bm25(chunk_fts) as score
               FROM chunk_fts
               JOIN notes ON notes.id = chunk_fts.note_id
               WHERE chunk_fts MATCH ? AND notes.scope = ? AND notes.owner_ghost IS NULL
               ORDER BY score ASC
               LIMIT ?"#,
        )
        .bind(&safe_query)
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
        .bind(&safe_query)
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
        KnowledgeError::Embedding(format!("embedding serialize failed: {e}"))
    })?;

    // KNN must run in a CTE with `k = ?` because vec0 cannot see LIMIT
    // through JOINs. We overfetch in the CTE, then filter+limit in the outer query.
    let knn_k = limit * 4; // overfetch to allow for scope/owner filtering

    let rows = if let Some(note_ids) = note_filter {
        let placeholders = note_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = if scope.is_shared() {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.id IN ({}) AND n.scope = ? AND n.owner_ghost IS NULL \
                 ORDER BY knn.distance ASC LIMIT ?",
                placeholders
            )
        } else {
            format!(
                "WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?) \
                 SELECT c.id, knn.distance FROM knn \
                 JOIN chunks c ON c.id = knn.rowid \
                 JOIN notes n ON n.id = c.note_id \
                 WHERE n.id IN ({}) AND n.scope = ? AND n.owner_ghost = ? \
                 ORDER BY knn.distance ASC LIMIT ?",
                placeholders
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
        query_builder = query_builder.bind(limit as i64);
        query_builder.fetch_all(pool).await?
    } else if scope.is_shared() {
        sqlx::query_as::<_, (i64, f32)>(
            r#"WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?)
               SELECT c.id, knn.distance
               FROM knn
               JOIN chunks c ON c.id = knn.rowid
               JOIN notes n ON n.id = c.note_id
               WHERE n.scope = ? AND n.owner_ghost IS NULL
               ORDER BY knn.distance ASC
               LIMIT ?"#,
        )
        .bind(&payload)
        .bind(knn_k as i64)
        .bind(scope.as_str())
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (i64, f32)>(
            r#"WITH knn AS (SELECT rowid, distance FROM chunk_vec WHERE embedding MATCH ? AND k = ?)
               SELECT c.id, knn.distance
               FROM knn
               JOIN chunks c ON c.id = knn.rowid
               JOIN notes n ON n.id = c.note_id
               WHERE n.scope = ? AND n.owner_ghost = ?
               ORDER BY knn.distance ASC
               LIMIT ?"#,
        )
        .bind(&payload)
        .bind(knn_k as i64)
        .bind(scope.as_str())
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
        let row = if scope.is_shared() {
            sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.note_type, n.path, n.trust_score, n.scope, c.content
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
            sqlx::query_as::<_, (String, String, String, String, i64, String, String)>(
                r#"SELECT n.id, n.title, n.note_type, n.path, n.trust_score, n.scope, c.content
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

        if let Some((id, title, note_type, path, trust_score, scope, content)) = row {
            let snippet = content.chars().take(200).collect::<String>();
            let trust_boost = 1.0 + (trust_score as f32 / 20.0);
            summaries.push(NoteSummary {
                id,
                title,
                note_type,
                path: path.into(),
                scope: scope.parse().unwrap_or(KnowledgeScope::Shared),
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
    let row = if scope.is_shared() {
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
        .bind(scope.as_str())
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
        .bind(scope.as_str())
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
            scope: scope.parse().unwrap_or(KnowledgeScope::Shared),
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

/// Determine the filesystem directory, internal scope, and owner_ghost for writing.
fn resolve_write_target(
    context: &KnowledgeContext,
    settings: &KnowledgeSettings,
    scope: &MemoryScope,
) -> KnowledgeResult<(std::path::PathBuf, KnowledgeScope, Option<String>)> {
    match scope {
        MemoryScope::SharedOnly => {
            let dir = shared_knowledge_root(settings)?;
            Ok((dir, KnowledgeScope::Shared, None))
        }
        MemoryScope::GhostPrivate | MemoryScope::GhostOnly => {
            let dir = ghost_private_root(&context.workspace_root);
            Ok((dir, KnowledgeScope::GhostPrivate, Some(context.ghost_name.clone())))
        }
        MemoryScope::GhostProjects => {
            let dir = ghost_projects_root(&context.workspace_root);
            Ok((dir, KnowledgeScope::GhostProjects, Some(context.ghost_name.clone())))
        }
        MemoryScope::GhostDiary => {
            let dir = ghost_diary_root(&context.workspace_root);
            Ok((dir, KnowledgeScope::GhostDiary, Some(context.ghost_name.clone())))
        }
        // Default: private knowledge
        MemoryScope::All => {
            let dir = ghost_private_root(&context.workspace_root);
            Ok((dir, KnowledgeScope::GhostPrivate, Some(context.ghost_name.clone())))
        }
    }
}

/// Verify the calling ghost has write access to a note.
fn verify_write_access(context: &KnowledgeContext, doc: &NoteDocument) -> KnowledgeResult<()> {
    if doc.scope.is_shared() {
        // Shared notes are writable by any ghost
        return Ok(());
    }
    // Private notes: only the owner ghost can write
    if doc.created_by_ghost != context.ghost_name {
        return Err(KnowledgeError::AccessDenied(format!(
            "ghost '{}' cannot modify note owned by '{}'",
            context.ghost_name, doc.created_by_ghost,
        )));
    }
    Ok(())
}

/// Build TOML front matter for a new note.
#[allow(clippy::too_many_arguments)]
fn build_front_matter(
    id: &str,
    title: &str,
    note_type: &str,
    ghost_name: &str,
    trust_score: i64,
    parent: Option<&str>,
    tags: Option<&[String]>,
    source: Option<&[String]>,
    now: &DateTime<Utc>,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("id = \"{}\"", id));
    lines.push(format!("title = \"{}\"", title.replace('"', "\\\"")));
    lines.push(format!("type = \"{}\"", note_type));
    lines.push(format!("created_at = \"{}\"", now.to_rfc3339()));
    lines.push(format!("trust_score = {}", trust_score));
    if let Some(parent_id) = parent {
        lines.push(format!("parent = \"{}\"", parent_id));
    }
    if let Some(tag_list) = tags {
        let formatted: Vec<String> = tag_list.iter().map(|t| format!("\"{}\"", t)).collect();
        lines.push(format!("tags = [{}]", formatted.join(", ")));
    }
    if let Some(source_list) = source {
        let formatted: Vec<String> = source_list.iter().map(|s| format!("\"{}\"", s)).collect();
        lines.push(format!("source = [{}]", formatted.join(", ")));
    }
    lines.push(String::new());
    lines.push("[created_by]".to_string());
    lines.push(format!("ghost = \"{}\"", ghost_name));
    lines.push("model = \"tool\"".to_string());
    lines.join("\n")
}

/// Rebuild front matter from a parsed FrontMatter struct.
fn rebuild_front_matter(front: &crate::parser::FrontMatter) -> String {
    let mut lines = Vec::new();
    lines.push(format!("id = \"{}\"", front.id));
    lines.push(format!("title = \"{}\"", front.title.replace('"', "\\\"")));
    lines.push(format!("type = \"{}\"", front.note_type));
    lines.push(format!("created_at = \"{}\"", front.created_at.to_rfc3339()));
    lines.push(format!("trust_score = {}", front.trust_score));
    if let Some(version) = front.version {
        lines.push(format!("version = {}", version));
    }
    if let Some(parent) = &front.parent {
        lines.push(format!("parent = \"{}\"", parent));
    }
    if let Some(tags) = &front.tags {
        let formatted: Vec<String> = tags.iter().map(|t| format!("\"{}\"", t)).collect();
        lines.push(format!("tags = [{}]", formatted.join(", ")));
    }
    if let Some(sources) = &front.source {
        lines.push(String::new());
        for src in sources {
            lines.push("[[source]]".to_string());
            lines.push(format!("path = \"{}\"", src.path));
            if let Some(checksum) = &src.checksum {
                lines.push(format!("checksum = \"{}\"", checksum));
            }
        }
    }
    if let Some(validated_at) = front.last_validated_at {
        lines.push(format!("last_validated_at = \"{}\"", validated_at.to_rfc3339()));
    }
    if let Some(validated_by) = &front.last_validated_by {
        lines.push(String::new());
        lines.push("[last_validated_by]".to_string());
        lines.push(format!("ghost = \"{}\"", validated_by.ghost));
        lines.push(format!("model = \"{}\"", validated_by.model));
    }
    if let Some(files) = &front.files {
        let formatted: Vec<String> = files.iter().map(|f| format!("\"{}\"", f)).collect();
        lines.push(format!("files = [{}]", formatted.join(", ")));
    }
    lines.push(String::new());
    lines.push("[created_by]".to_string());
    lines.push(format!("ghost = \"{}\"", front.created_by.ghost));
    lines.push(format!("model = \"{}\"", front.created_by.model));
    if let Some(comments) = &front.comments {
        for comment in comments {
            lines.push(String::new());
            lines.push("[[comments]]".to_string());
            lines.push(format!("ghost = \"{}\"", comment.ghost));
            lines.push(format!("model = \"{}\"", comment.model));
            lines.push(format!("at = \"{}\"", comment.at.to_rfc3339()));
            lines.push(format!("text = \"{}\"", comment.text.replace('"', "\\\"")));
        }
    }
    lines.join("\n")
}

/// Sanitize a title for use as a filename.
fn sanitize_filename(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}


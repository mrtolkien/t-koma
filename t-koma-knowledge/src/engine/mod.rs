use chrono::{DateTime, NaiveDate, Utc};
use sqlx::SqlitePool;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeError;
use crate::errors::KnowledgeResult;
use crate::index::{
    check_embedding_provider_change, reconcile_ghost, reconcile_shared, reindex_embeddings,
};
use crate::models::{
    DiaryQuery, DiarySearchResult, IndexStats, IndexStatsEntry, KnowledgeGetQuery, KnowledgeScope,
    KnowledgeSearchQuery, KnowledgeSearchResult, MatchedTopic, NoteCreateRequest, NoteDocument,
    NoteQuery, NoteResult, NoteSummary, NoteUpdateRequest, NoteWriteResult, OwnershipScope,
    ReferenceFileStatus, ReferenceQuery, ReferenceSaveRequest, ReferenceSaveResult,
    ReferenceSearchOutput, ReferenceSearchResult, SearchCategory, TopicCreateRequest,
    TopicCreateResult, TopicListEntry, TopicSearchResult, WriteScope,
};
use crate::paths::knowledge_db_path;
use crate::storage::KnowledgeStore;

pub(crate) mod get;
pub(crate) mod notes;
pub(crate) mod reference;
pub(crate) mod save;
pub(crate) mod search;
pub(crate) mod topics;

pub use reference::RecentRefSummary;

#[derive(Debug, Clone)]
pub struct KnowledgeEngine {
    settings: KnowledgeSettings,
    embedder: EmbeddingClient,
    store: KnowledgeStore,
}

impl KnowledgeEngine {
    /// Open a persistent KnowledgeEngine that reuses a single DB pool.
    pub async fn open(settings: KnowledgeSettings) -> KnowledgeResult<Self> {
        let path = knowledge_db_path(&settings)?;
        let store = KnowledgeStore::open(&path, settings.embedding_dim).await?;
        let embedder = EmbeddingClient::new(&settings);
        Ok(Self {
            settings,
            embedder,
            store,
        })
    }

    /// Access the underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        self.store.pool()
    }

    /// Access the knowledge settings.
    pub fn settings(&self) -> &KnowledgeSettings {
        &self.settings
    }

    /// Access the embedding client (crate-internal).
    pub(crate) fn embedder(&self) -> &EmbeddingClient {
        &self.embedder
    }

    pub async fn memory_search(
        &self,
        ghost_name: &str,
        query: NoteQuery,
    ) -> KnowledgeResult<Vec<NoteResult>> {
        let mut results = Vec::new();

        let scopes = search::resolve_scopes(&query.scope);
        for scope in scopes {
            self.maybe_reconcile(ghost_name, scope).await?;
            let partial = search::search_store(
                &self.settings,
                &self.embedder,
                self.store.pool(),
                &query,
                scope,
                ghost_name,
                None,
            )
            .await?;
            results.extend(partial);
        }

        let max_results = query
            .options
            .max_results
            .unwrap_or(self.settings.search.max_results);
        results.sort_by(|a, b| {
            b.summary
                .score
                .partial_cmp(&a.summary.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);

        Ok(results)
    }

    /// Search diary entries for a ghost using hybrid BM25 + dense search.
    pub async fn search_diary(
        &self,
        ghost_name: &str,
        query: DiaryQuery,
    ) -> KnowledgeResult<Vec<DiarySearchResult>> {
        self.maybe_reconcile(ghost_name, KnowledgeScope::GhostDiary)
            .await?;
        search::search_diary(
            &self.settings,
            &self.embedder,
            self.store.pool(),
            &query,
            ghost_name,
        )
        .await
    }

    pub async fn memory_get(
        &self,
        ghost_name: &str,
        note_id_or_title: &str,
        scope: OwnershipScope,
    ) -> KnowledgeResult<NoteDocument> {
        get::memory_get(self, ghost_name, note_id_or_title, scope).await
    }

    pub async fn memory_capture(
        &self,
        ghost_name: &str,
        payload: &str,
        scope: WriteScope,
        source: Option<&str>,
    ) -> KnowledgeResult<String> {
        get::memory_capture(self, ghost_name, payload, scope, source).await
    }

    /// Create a structured note with validated front matter.
    pub async fn note_create(
        &self,
        ghost_name: &str,
        model: &str,
        request: NoteCreateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_create(self, ghost_name, model, request).await
    }

    /// Update an existing note (title, body, tags, trust, parent).
    pub async fn note_update(
        &self,
        ghost_name: &str,
        request: NoteUpdateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_update(self, ghost_name, request).await
    }

    /// Record validation metadata and optionally adjust trust score.
    pub async fn note_validate(
        &self,
        ghost_name: &str,
        model: &str,
        note_id: &str,
        trust_score: Option<i64>,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_validate(self, ghost_name, model, note_id, trust_score).await
    }

    /// Append a comment entry to a note's front matter.
    pub async fn note_comment(
        &self,
        ghost_name: &str,
        model: &str,
        note_id: &str,
        text: &str,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_comment(self, ghost_name, model, note_id, text).await
    }

    /// Delete a note and all associated DB records (chunks, tags, links).
    pub async fn note_delete(&self, ghost_name: &str, note_id: &str) -> KnowledgeResult<()> {
        notes::note_delete(self, ghost_name, note_id).await
    }

    pub async fn reference_search(
        &self,
        ghost_name: &str,
        query: ReferenceQuery,
    ) -> KnowledgeResult<ReferenceSearchResult> {
        self.maybe_reconcile(ghost_name, KnowledgeScope::SharedReference)
            .await?;
        reference::reference_search(self, &query).await
    }

    /// Set the status of a reference file (active, problematic, obsolete).
    pub async fn reference_file_set_status(
        &self,
        note_id: &str,
        status: ReferenceFileStatus,
        reason: Option<&str>,
    ) -> KnowledgeResult<()> {
        reference::reference_file_set_status(self, note_id, status, reason).await
    }

    /// Delete a reference file by note ID (scope-agnostic).
    pub async fn reference_file_delete(&self, note_id: &str) -> KnowledgeResult<()> {
        reference::reference_file_delete(self, note_id).await
    }

    /// Move a reference file from one topic to another without exposing content.
    ///
    /// Reads the file from disk, saves to the target topic, deletes the original.
    /// Content never passes through the caller — stays server-side.
    pub async fn reference_file_move(
        &self,
        ghost_name: &str,
        model: &str,
        note_id: &str,
        target_topic: &str,
        target_filename: Option<&str>,
        target_collection: Option<&str>,
    ) -> KnowledgeResult<ReferenceSaveResult> {
        reference::reference_file_move(
            self,
            ghost_name,
            model,
            note_id,
            target_topic,
            target_filename,
            target_collection,
        )
        .await
    }

    /// Get a reference file by note_id or by topic + file_path.
    pub async fn reference_get(
        &self,
        note_id: Option<&str>,
        topic: Option<&str>,
        file_path: Option<&str>,
        max_chars: Option<usize>,
    ) -> KnowledgeResult<NoteDocument> {
        reference::reference_get(self, note_id, topic, file_path, max_chars).await
    }

    /// Get reference files saved since a given RFC3339 timestamp.
    pub async fn recent_reference_files(
        &self,
        since_rfc3339: &str,
    ) -> KnowledgeResult<Vec<reference::RecentRefSummary>> {
        reference::recent_reference_files(self, since_rfc3339).await
    }

    /// Resolve an existing reference topic by fuzzy name matching.
    ///
    /// Returns `(id, title)` or errors with `UnknownNote` if no topic found.
    pub async fn resolve_topic(&self, name: &str) -> KnowledgeResult<(String, String)> {
        save::find_existing_topic(self.pool(), name)
            .await?
            .ok_or_else(|| KnowledgeError::UnknownNote(format!("topic '{}'", name)))
    }

    /// Save content to a reference topic, creating topic and collection if needed.
    pub async fn reference_save(
        &self,
        ghost_name: &str,
        model: &str,
        request: ReferenceSaveRequest,
    ) -> KnowledgeResult<ReferenceSaveResult> {
        save::reference_save(self, ghost_name, model, request).await
    }

    /// Build an approval summary for a topic creation request (Phase 1).
    pub async fn topic_approval_summary(
        &self,
        request: &TopicCreateRequest,
    ) -> KnowledgeResult<String> {
        topics::build_topic_approval_summary(request).await
    }

    /// Execute topic creation after operator approval (Phase 2).
    pub async fn topic_create(
        &self,
        ghost_name: &str,
        model: &str,
        request: TopicCreateRequest,
    ) -> KnowledgeResult<TopicCreateResult> {
        topics::topic_create_execute(self, ghost_name, model, request).await
    }

    /// Semantic search over reference topics.
    pub async fn topic_search(&self, query: &str) -> KnowledgeResult<Vec<TopicSearchResult>> {
        topics::topic_search(self, query).await
    }

    /// List all reference topics with staleness info.
    pub async fn topic_list(&self, include_obsolete: bool) -> KnowledgeResult<Vec<TopicListEntry>> {
        topics::topic_list(self, include_obsolete).await
    }

    /// Get recent reference topics for system prompt injection.
    pub async fn recent_topics(&self) -> KnowledgeResult<Vec<(String, String, Vec<String>)>> {
        topics::recent_topics(self.pool()).await
    }

    // ── Unified knowledge query methods ─────────────────────────────

    /// Unified search across notes, diary, references, and topics.
    ///
    /// Searches active categories in parallel, then merges results using a
    /// min-per-category budget algorithm: each non-empty category gets at
    /// least 1 result, remaining budget is filled by global score ranking.
    pub async fn knowledge_search(
        &self,
        ghost_name: &str,
        query: KnowledgeSearchQuery,
    ) -> KnowledgeResult<KnowledgeSearchResult> {
        let categories = query.categories.clone().unwrap_or_else(SearchCategory::all);
        let max_results = query
            .options
            .max_results
            .unwrap_or(self.settings.search.max_results);

        // Reconcile scopes that will be queried
        let needs_shared = categories.iter().any(|c| {
            matches!(
                c,
                SearchCategory::Notes | SearchCategory::References | SearchCategory::Topics
            )
        }) && query.scope != OwnershipScope::Private;
        let needs_ghost = categories
            .iter()
            .any(|c| matches!(c, SearchCategory::Notes | SearchCategory::Diary))
            && query.scope != OwnershipScope::Shared;

        if needs_shared {
            self.maybe_reconcile(ghost_name, KnowledgeScope::SharedNote)
                .await?;
        }
        if needs_ghost {
            self.maybe_reconcile(ghost_name, KnowledgeScope::GhostNote)
                .await?;
        }

        // ── Per-category search ─────────────────────────────────────

        let mut notes = Vec::new();
        if categories.contains(&SearchCategory::Notes) {
            let scopes = search::resolve_note_only_scopes(&query.scope);
            let note_query = NoteQuery {
                query: query.query.clone(),
                scope: query.scope,
                options: query.options.clone(),
            };
            for scope in scopes {
                let partial = search::search_store(
                    &self.settings,
                    &self.embedder,
                    self.store.pool(),
                    &note_query,
                    scope,
                    ghost_name,
                    query.archetype.as_deref(),
                )
                .await?;
                notes.extend(partial);
            }
            notes.sort_by(|a, b| {
                b.summary
                    .score
                    .partial_cmp(&a.summary.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let mut diary = Vec::new();
        if categories.contains(&SearchCategory::Diary) && query.scope != OwnershipScope::Shared {
            let diary_query = DiaryQuery {
                query: query.query.clone(),
                options: query.options.clone(),
            };
            diary = search::search_diary(
                &self.settings,
                &self.embedder,
                self.store.pool(),
                &diary_query,
                ghost_name,
            )
            .await?;
        }

        let mut ref_output = ReferenceSearchOutput {
            matched_topic: None,
            results: Vec::new(),
        };
        if categories.contains(&SearchCategory::References) {
            if let Some(topic_name) = &query.topic {
                // Scoped to a specific topic — use existing reference_search
                let ref_query = ReferenceQuery {
                    topic: topic_name.clone(),
                    question: query.query.clone(),
                    options: query.options.clone(),
                };
                if let Ok(result) = reference::reference_search(self, &ref_query).await {
                    ref_output.matched_topic = Some(MatchedTopic {
                        topic_id: result.topic_id,
                        title: result.topic_title,
                        body: result.topic_body,
                    });
                    ref_output.results = result.results;
                }
            } else {
                // Broad search across all reference files
                ref_output.results =
                    reference::search_all_reference_files(self, &query.query, &query.options)
                        .await?;
            }
        }

        let mut topic_results = Vec::new();
        if categories.contains(&SearchCategory::Topics) {
            topic_results = topics::topic_search(self, &query.query).await?;
        }

        // ── Min-per-category budget algorithm ───────────────────────
        // Reserve top-1 from each non-empty category, then fill remaining
        // budget from a globally sorted pool.
        //
        // Category boosts applied to pool scores for priority ordering:
        //   Notes      1.5x  — curated, high-quality knowledge
        //   Diary      1.0–1.5x — recency-weighted (1.5x today → 1.0x at 1 year)
        //   References 1.0x  — baseline (already has internal doc_boost)
        //   Topics     1.0x  — baseline

        const NOTE_BOOST: f32 = 1.5;

        let diary_recency_boost = |date_str: &str| -> f32 {
            let Some(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok() else {
                return 1.0;
            };
            let days_old = (Utc::now().date_naive() - date).num_days().max(0) as f32;
            (1.5 - 0.5 * days_old / 365.0).max(1.0)
        };

        #[derive(Debug)]
        struct ScoredItem {
            score: f32,
            category: SearchCategory,
            index: usize,
        }

        let mut reserved_indices: Vec<(SearchCategory, usize)> = Vec::new();
        let mut pool_items: Vec<ScoredItem> = Vec::new();

        // Notes (boosted: curated knowledge ranks highest)
        for (i, r) in notes.iter().enumerate() {
            let item = ScoredItem {
                score: r.summary.score * NOTE_BOOST,
                category: SearchCategory::Notes,
                index: i,
            };
            if i == 0 {
                reserved_indices.push((SearchCategory::Notes, i));
            } else {
                pool_items.push(item);
            }
        }
        // Diary (recency-boosted: recent entries rank higher)
        for (i, r) in diary.iter().enumerate() {
            let item = ScoredItem {
                score: r.score * diary_recency_boost(&r.date),
                category: SearchCategory::Diary,
                index: i,
            };
            if i == 0 {
                reserved_indices.push((SearchCategory::Diary, i));
            } else {
                pool_items.push(item);
            }
        }
        // References (baseline: internal doc_boost already applied)
        for (i, r) in ref_output.results.iter().enumerate() {
            let item = ScoredItem {
                score: r.summary.score,
                category: SearchCategory::References,
                index: i,
            };
            if i == 0 {
                reserved_indices.push((SearchCategory::References, i));
            } else {
                pool_items.push(item);
            }
        }
        // Topics (baseline)
        for (i, r) in topic_results.iter().enumerate() {
            let item = ScoredItem {
                score: r.score,
                category: SearchCategory::Topics,
                index: i,
            };
            if i == 0 {
                reserved_indices.push((SearchCategory::Topics, i));
            } else {
                pool_items.push(item);
            }
        }

        let reserved_count = reserved_indices.len();
        let remaining_budget = max_results.saturating_sub(reserved_count);

        pool_items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pool_items.truncate(remaining_budget);

        // Build index sets per category to know what to include
        let mut include_notes = std::collections::HashSet::new();
        let mut include_diary = std::collections::HashSet::new();
        let mut include_refs = std::collections::HashSet::new();
        let mut include_topics = std::collections::HashSet::new();

        for (cat, idx) in &reserved_indices {
            match cat {
                SearchCategory::Notes => {
                    include_notes.insert(*idx);
                }
                SearchCategory::Diary => {
                    include_diary.insert(*idx);
                }
                SearchCategory::References => {
                    include_refs.insert(*idx);
                }
                SearchCategory::Topics => {
                    include_topics.insert(*idx);
                }
            }
        }
        for item in &pool_items {
            match item.category {
                SearchCategory::Notes => {
                    include_notes.insert(item.index);
                }
                SearchCategory::Diary => {
                    include_diary.insert(item.index);
                }
                SearchCategory::References => {
                    include_refs.insert(item.index);
                }
                SearchCategory::Topics => {
                    include_topics.insert(item.index);
                }
            }
        }

        // Filter to included indices
        let final_notes: Vec<NoteResult> = notes
            .into_iter()
            .enumerate()
            .filter(|(i, _)| include_notes.contains(i))
            .map(|(_, r)| r)
            .collect();
        let final_diary: Vec<DiarySearchResult> = diary
            .into_iter()
            .enumerate()
            .filter(|(i, _)| include_diary.contains(i))
            .map(|(_, r)| r)
            .collect();
        let final_refs: Vec<NoteResult> = ref_output
            .results
            .into_iter()
            .enumerate()
            .filter(|(i, _)| include_refs.contains(i))
            .map(|(_, r)| r)
            .collect();
        let final_topics: Vec<TopicSearchResult> = topic_results
            .into_iter()
            .enumerate()
            .filter(|(i, _)| include_topics.contains(i))
            .map(|(_, r)| r)
            .collect();

        Ok(KnowledgeSearchResult {
            notes: final_notes,
            diary: final_diary,
            references: ReferenceSearchOutput {
                matched_topic: ref_output.matched_topic,
                results: final_refs,
            },
            topics: final_topics,
        })
    }

    /// Unified retrieval by ID or by topic + path.
    ///
    /// - `id` only → search all scopes (SharedNote, GhostNote, GhostDiary,
    ///   SharedReference) until found
    /// - `topic` + `path` → delegate to reference_get
    pub async fn knowledge_get(
        &self,
        ghost_name: &str,
        query: KnowledgeGetQuery,
    ) -> KnowledgeResult<NoteDocument> {
        if let (Some(topic), Some(path)) = (&query.topic, &query.path) {
            // Delegate to reference file retrieval
            return self
                .reference_get(None, Some(topic), Some(path), query.max_chars)
                .await;
        }

        let id = query
            .id
            .as_deref()
            .ok_or(KnowledgeError::MissingField("id or (topic + path)"))?;

        // Try each scope until we find the note
        let scopes = [
            KnowledgeScope::SharedNote,
            KnowledgeScope::GhostNote,
            KnowledgeScope::GhostDiary,
            KnowledgeScope::SharedReference,
        ];

        for scope in scopes {
            if let Some(mut doc) = get::fetch_note(self.pool(), id, scope, ghost_name).await? {
                if let Some(limit) = query.max_chars
                    && doc.body.len() > limit
                {
                    doc.body = doc.body.chars().take(limit).collect();
                }
                return Ok(doc);
            }
        }

        Err(KnowledgeError::UnknownNote(id.to_string()))
    }

    /// List recently updated notes across all scopes (no embeddings needed).
    ///
    /// If `ghost_name` is non-empty, reconciles both shared and ghost scopes
    /// before querying.  Otherwise only shared scope is reconciled.
    pub async fn list_recent_notes(
        &self,
        ghost_name: &str,
        limit: usize,
    ) -> KnowledgeResult<Vec<NoteSummary>> {
        self.maybe_reconcile(ghost_name, KnowledgeScope::SharedNote)
            .await?;
        if !ghost_name.is_empty() {
            self.maybe_reconcile(ghost_name, KnowledgeScope::GhostNote)
                .await?;
        }

        let rows =
            sqlx::query_as::<_, (String, String, String, Option<String>, String, i64, String)>(
                "SELECT n.id, n.title, n.entry_type, n.archetype, n.path, n.trust_score, n.scope
             FROM notes n
             ORDER BY n.updated_at DESC
             LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(self.pool())
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, title, entry_type, archetype, path, trust_score, scope)| NoteSummary {
                    id,
                    title,
                    entry_type,
                    archetype,
                    path: path.into(),
                    scope: scope.parse().unwrap_or(KnowledgeScope::SharedNote),
                    trust_score,
                    score: 0.0,
                    snippet: String::new(),
                },
            )
            .collect())
    }

    /// Retrieve index statistics: note/chunk/embedding counts and latest entries.
    pub async fn index_stats(&self) -> KnowledgeResult<IndexStats> {
        let pool = self.pool();

        let (total_notes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM notes")
            .fetch_one(pool)
            .await?;

        let (total_chunks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM chunks")
            .fetch_one(pool)
            .await?;

        // chunk_vec may not exist if embeddings are disabled
        let total_embeddings: i64 = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM chunk_vec")
            .fetch_one(pool)
            .await
            .map(|(c,)| c)
            .unwrap_or(0);

        let recent = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT title, entry_type, scope, updated_at FROM notes ORDER BY updated_at DESC LIMIT 10",
        )
        .fetch_all(pool)
        .await?;

        Ok(IndexStats {
            total_notes,
            total_chunks,
            total_embeddings,
            embedding_model: self.settings.embedding_model.clone(),
            embedding_dim: self.settings.embedding_dim.unwrap_or(0) as u32,
            recent_entries: recent
                .into_iter()
                .map(|(title, entry_type, scope, updated_at)| IndexStatsEntry {
                    title,
                    entry_type,
                    scope,
                    updated_at,
                })
                .collect(),
        })
    }

    /// Check if the embedding provider/model changed and needs reindexing.
    ///
    /// Returns `true` if embeddings were invalidated and a reindex is needed.
    pub async fn check_embedding_change(&self) -> KnowledgeResult<bool> {
        check_embedding_provider_change(&self.settings, self.store.pool()).await
    }

    /// Re-embed chunks that are missing embedding vectors.
    ///
    /// Processes up to `max_chunks` in batches. Returns count of re-embedded chunks.
    pub async fn reindex_embeddings(&self, max_chunks: usize) -> KnowledgeResult<usize> {
        reindex_embeddings(
            &self.settings,
            self.store.pool(),
            &self.embedder,
            max_chunks,
        )
        .await
    }

    /// Count chunks that still need embedding (for progress tracking).
    pub async fn chunks_needing_embedding(&self) -> KnowledgeResult<i64> {
        crate::storage::count_chunks_needing_embedding(self.store.pool()).await
    }

    async fn maybe_reconcile(
        &self,
        ghost_name: &str,
        scope: KnowledgeScope,
    ) -> KnowledgeResult<()> {
        let pool = self.store.pool();
        let key = match scope {
            KnowledgeScope::SharedNote | KnowledgeScope::SharedReference => {
                "last_reconcile_shared".to_string()
            }
            _ => format!("last_reconcile_ghost:{}", ghost_name),
        };
        let last: Option<(String,)> =
            sqlx::query_as("SELECT value FROM meta WHERE key = ? LIMIT 1")
                .bind(&key)
                .fetch_optional(pool)
                .await?;
        let now = Utc::now();
        let should_run = match last {
            Some((value,)) => DateTime::parse_from_rfc3339(&value)
                .map(|dt| {
                    (now - dt.with_timezone(&Utc)).num_seconds() as u64
                        > self.settings.reconcile_seconds
                })
                .unwrap_or(true),
            None => true,
        };

        if should_run {
            match scope {
                KnowledgeScope::SharedNote | KnowledgeScope::SharedReference => {
                    reconcile_shared(&self.settings, pool, &self.embedder).await?;
                }
                _ => {
                    reconcile_ghost(&self.settings, pool, &self.embedder, ghost_name).await?;
                }
            }
            sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(now.to_rfc3339())
                .execute(pool)
                .await?;
        }

        Ok(())
    }
}

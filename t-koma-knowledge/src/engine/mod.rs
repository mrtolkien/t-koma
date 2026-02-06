use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::index::{reconcile_ghost, reconcile_shared};
use crate::models::{
    KnowledgeContext, KnowledgeScope, MemoryQuery, MemoryResult, MemoryScope, NoteCreateRequest,
    NoteDocument, NoteUpdateRequest, NoteWriteResult, ReferenceQuery, WriteScope,
};
use crate::paths::knowledge_db_path;
use crate::storage::KnowledgeStore;

pub(crate) mod get;
pub(crate) mod notes;
pub(crate) mod reference;
pub(crate) mod search;

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
        context: &KnowledgeContext,
        query: MemoryQuery,
    ) -> KnowledgeResult<Vec<MemoryResult>> {
        let mut results = Vec::new();

        let scopes = search::resolve_scopes(&query.scope);
        for scope in scopes {
            self.maybe_reconcile(context, scope).await?;
            let partial = search::search_store(
                &self.settings,
                &self.embedder,
                self.store.pool(),
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
        results.sort_by(|a, b| {
            b.summary
                .score
                .partial_cmp(&a.summary.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);

        Ok(results)
    }

    pub async fn memory_get(
        &self,
        context: &KnowledgeContext,
        note_id_or_title: &str,
        scope: MemoryScope,
    ) -> KnowledgeResult<NoteDocument> {
        get::memory_get(self, context, note_id_or_title, scope).await
    }

    pub async fn memory_capture(
        &self,
        context: &KnowledgeContext,
        payload: &str,
        scope: WriteScope,
    ) -> KnowledgeResult<String> {
        get::memory_capture(self, context, payload, scope).await
    }

    /// Create a structured note with validated front matter.
    pub async fn note_create(
        &self,
        context: &KnowledgeContext,
        request: NoteCreateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_create(self, context, request).await
    }

    /// Update an existing note (title, body, tags, trust, parent).
    pub async fn note_update(
        &self,
        context: &KnowledgeContext,
        request: NoteUpdateRequest,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_update(self, context, request).await
    }

    /// Record validation metadata and optionally adjust trust score.
    pub async fn note_validate(
        &self,
        context: &KnowledgeContext,
        note_id: &str,
        trust_score: Option<i64>,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_validate(self, context, note_id, trust_score).await
    }

    /// Append a comment entry to a note's front matter.
    pub async fn note_comment(
        &self,
        context: &KnowledgeContext,
        note_id: &str,
        text: &str,
    ) -> KnowledgeResult<NoteWriteResult> {
        notes::note_comment(self, context, note_id, text).await
    }

    pub async fn reference_search(
        &self,
        context: &KnowledgeContext,
        query: ReferenceQuery,
    ) -> KnowledgeResult<Vec<MemoryResult>> {
        self.maybe_reconcile(context, KnowledgeScope::Reference)
            .await?;
        reference::reference_search(self, &query).await
    }

    async fn maybe_reconcile(
        &self,
        context: &KnowledgeContext,
        scope: KnowledgeScope,
    ) -> KnowledgeResult<()> {
        let pool = self.store.pool();
        let key = match scope {
            KnowledgeScope::Shared | KnowledgeScope::Reference => {
                "last_reconcile_shared".to_string()
            }
            _ => format!("last_reconcile_ghost:{}", context.ghost_name),
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
                KnowledgeScope::Shared | KnowledgeScope::Reference => {
                    reconcile_shared(&self.settings, pool, &self.embedder).await?;
                }
                _ => {
                    reconcile_ghost(
                        &self.settings,
                        pool,
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
                .execute(pool)
                .await?;
        }

        Ok(())
    }
}

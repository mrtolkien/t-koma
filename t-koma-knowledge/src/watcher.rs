use std::path::PathBuf;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::warn;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::index::{reconcile_ghost, reconcile_shared};
use crate::models::KnowledgeScope;
use crate::paths::{knowledge_db_path, shared_notes_root, shared_references_root};
use crate::storage::KnowledgeStore;

pub async fn run_shared_watcher(settings: KnowledgeSettings) -> KnowledgeResult<()> {
    let store =
        KnowledgeStore::open(&knowledge_db_path(&settings)?, settings.embedding_dim).await?;
    let embedder = EmbeddingClient::new(&settings);
    let root = shared_notes_root(&settings)?;
    let reference = shared_references_root(&settings)?;
    run_watcher(
        settings,
        store,
        embedder,
        vec![root, reference],
        KnowledgeScope::SharedNote,
        None,
    )
    .await
}

pub async fn run_ghost_watcher(
    settings: KnowledgeSettings,
    ghost_name: String,
) -> KnowledgeResult<()> {
    let store =
        KnowledgeStore::open(&knowledge_db_path(&settings)?, settings.embedding_dim).await?;
    let embedder = EmbeddingClient::new(&settings);
    let notes = crate::paths::ghost_notes_root(&settings, &ghost_name)?;
    let diary = crate::paths::ghost_diary_root(&settings, &ghost_name)?;
    let references = crate::paths::ghost_references_root(&settings, &ghost_name)?;
    run_watcher(
        settings,
        store,
        embedder,
        vec![notes, diary, references],
        KnowledgeScope::GhostNote,
        Some(ghost_name),
    )
    .await
}

async fn run_watcher(
    settings: KnowledgeSettings,
    store: KnowledgeStore,
    embedder: EmbeddingClient,
    roots: Vec<PathBuf>,
    scope: KnowledgeScope,
    ghost_name: Option<String>,
) -> KnowledgeResult<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<()>();

    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })?;

    for root in &roots {
        if root.exists() {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }
    }

    let mut pending = false;
    loop {
        tokio::select! {
            _ = rx.recv() => {
                pending = true;
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                if pending {
                    let result = match scope {
                        KnowledgeScope::SharedNote | KnowledgeScope::SharedReference => {
                            reconcile_shared(&settings, store.pool(), &embedder).await
                        }
                        _ => {
                            let ghost = ghost_name.as_deref().unwrap_or("unknown");
                            reconcile_ghost(
                                &settings,
                                store.pool(),
                                &embedder,
                                ghost,
                            )
                            .await
                        }
                    };
                    if let Err(err) = result {
                        warn!("knowledge watcher reconcile failed: {err}");
                    }
                    pending = false;
                }
            }
        }
    }
}

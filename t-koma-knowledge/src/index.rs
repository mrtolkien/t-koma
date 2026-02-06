use std::path::Path;

use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::config::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::ingest::{ingest_markdown, ingest_reference_file, ingest_reference_topic};
use crate::models::KnowledgeScope;
use crate::paths::{ghost_diary_root, ghost_private_root, ghost_projects_root, reference_root, shared_knowledge_root};
use crate::storage::{
    ensure_vec_table_dim, replace_chunks, replace_links, replace_tags, upsert_note, upsert_vec,
};

pub async fn reconcile_shared(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
) -> KnowledgeResult<()> {
    let root = shared_knowledge_root(settings)?;
    index_markdown_tree(settings, store, embedder, &root, KnowledgeScope::Shared, None).await?;
    index_reference_topics(settings, store, embedder).await?;
    Ok(())
}

pub async fn reconcile_ghost(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    workspace_root: &Path,
    ghost_name: &str,
) -> KnowledgeResult<()> {
    let private_root = ghost_private_root(workspace_root);
    let projects_root = ghost_projects_root(workspace_root);
    let diary_root = ghost_diary_root(workspace_root);
    let owner = Some(ghost_name.to_string());

    index_markdown_tree(settings, store, embedder, &private_root, KnowledgeScope::GhostPrivate, owner.clone()).await?;
    index_markdown_tree(settings, store, embedder, &projects_root, KnowledgeScope::GhostProjects, owner.clone()).await?;
    index_markdown_tree(settings, store, embedder, &diary_root, KnowledgeScope::GhostDiary, owner).await?;

    Ok(())
}

async fn index_reference_topics(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
) -> KnowledgeResult<()> {
    let root = reference_root(settings)?;
    if !root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(&root).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }

        let raw = tokio::fs::read_to_string(path).await?;
        let (note, files) = ingest_reference_topic(settings, path, &raw).await?;

        upsert_note(store, &note.note).await?;
        replace_tags(store, &note.note.id, &note.tags).await?;
        replace_links(store, &note.note.id, None, &note.links).await?;
        let chunk_ids = replace_chunks(
            store,
            &note.note.id,
            &note.note.title,
            &note.note.note_type,
            &note.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &note.chunks, &chunk_ids).await?;

        if !files.is_empty() {
            let files_json = serde_json::to_string(&files).unwrap_or_default();
            sqlx::query("INSERT OR REPLACE INTO reference_topics (topic_id, files_json) VALUES (?, ?)")
                .bind(&note.note.id)
                .bind(files_json)
                .execute(store)
                .await?;
            index_reference_files(settings, store, embedder, &note.note.id, path.parent().unwrap_or(&root), &files).await?;
        }
    }

    Ok(())
}

async fn index_reference_files(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    topic_id: &str,
    topic_dir: &Path,
    files: &[String],
) -> KnowledgeResult<()> {
    for file in files {
        let path = topic_dir.join(file);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let note_id = format!("ref:{}:{}", topic_id, file);
        let title = path.file_name().and_then(|v| v.to_str()).unwrap_or(file);
        let ingested = ingest_reference_file(settings, &path, &raw, &note_id, title).await?;

        if is_unchanged(store, &path, &ingested.note.content_hash).await? {
            continue;
        }

        upsert_note(store, &ingested.note).await?;
        let chunk_ids = replace_chunks(
            store,
            &ingested.note.id,
            &ingested.note.title,
            &ingested.note.note_type,
            &ingested.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;

        sqlx::query(
            "INSERT OR REPLACE INTO reference_files (topic_id, note_id, path) VALUES (?, ?, ?)",
        )
        .bind(topic_id)
        .bind(&ingested.note.id)
        .bind(ingested.note.path.to_string_lossy().to_string())
        .execute(store)
        .await?;
    }

    Ok(())
}

async fn index_markdown_tree(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    root: &Path,
    scope: KnowledgeScope,
    owner_ghost: Option<String>,
) -> KnowledgeResult<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        if is_archived_path(path) {
            continue;
        }

        let raw = tokio::fs::read_to_string(path).await?;
        let ingested = ingest_markdown(settings, scope, owner_ghost.clone(), path, &raw).await?;

        if is_unchanged(store, path, &ingested.note.content_hash).await? {
            continue;
        }

        upsert_note(store, &ingested.note).await?;
        replace_tags(store, &ingested.note.id, &ingested.tags).await?;
        replace_links(store, &ingested.note.id, owner_ghost.as_deref(), &ingested.links).await?;
        let chunk_ids = replace_chunks(
            store,
            &ingested.note.id,
            &ingested.note.title,
            &ingested.note.note_type,
            &ingested.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;
    }

    Ok(())
}

async fn embed_chunks(
    settings: &KnowledgeSettings,
    embedder: &EmbeddingClient,
    store: &SqlitePool,
    chunks: &[crate::storage::ChunkRecord],
    chunk_ids: &[i64],
) -> KnowledgeResult<()> {
    if chunks.is_empty() {
        return Ok(());
    }

    let batch_size = settings.embedding_batch.max(1);
    let mut offset = 0;
    while offset < chunks.len() {
        let end = (offset + batch_size).min(chunks.len());
        let inputs = chunks[offset..end]
            .iter()
            .map(|chunk| chunk.content.clone())
            .collect::<Vec<_>>();

        let embeddings = embedder.embed_batch(&inputs).await?;
        if embeddings.is_empty() {
            return Ok(());
        }

        let dim = embeddings[0].len();
        if let Some(expected) = settings.embedding_dim
            && expected != dim
        {
            return Err(crate::errors::KnowledgeError::EmbeddingDimMismatch {
                expected,
                actual: dim,
            });
        }
        ensure_vec_table_dim(store, dim).await?;

        for (idx, embedding) in embeddings.into_iter().enumerate() {
            let chunk_id = *chunk_ids.get(offset + idx).unwrap_or(&0);
            if chunk_id == 0 {
                continue;
            }
            upsert_vec(store, chunk_id, &embedding).await?;
        }

        offset = end;
    }

    Ok(())
}

async fn is_unchanged(pool: &SqlitePool, path: &Path, content_hash: &str) -> KnowledgeResult<bool> {
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT content_hash FROM notes WHERE path = ? LIMIT 1",
    )
    .bind(path.to_string_lossy().to_string())
    .fetch_optional(pool)
    .await?;

    Ok(existing
        .map(|(hash,)| hash == content_hash)
        .unwrap_or(false))
}

fn is_archived_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".archive")
}

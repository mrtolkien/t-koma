use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::ingest::{ingest_diary_entry, ingest_markdown, ingest_reference_file_with_context};
use crate::models::{KnowledgeScope, SourceRole};
use crate::paths::{ghost_diary_root, ghost_notes_root, shared_notes_root, shared_references_root};
use crate::storage::{
    ensure_vec_table_dim, replace_chunks, replace_links, replace_tags, upsert_note, upsert_vec,
};

pub async fn reconcile_shared(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
) -> KnowledgeResult<()> {
    let root = shared_notes_root(settings)?;
    index_markdown_tree(
        settings,
        store,
        embedder,
        &root,
        KnowledgeScope::SharedNote,
        None,
    )
    .await?;
    index_reference_topics(settings, store, embedder).await?;
    Ok(())
}

pub async fn reconcile_ghost(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    ghost_name: &str,
) -> KnowledgeResult<()> {
    let notes_root = ghost_notes_root(settings, ghost_name)?;
    let diary_root = ghost_diary_root(settings, ghost_name)?;
    let owner = Some(ghost_name.to_string());

    index_markdown_tree(
        settings,
        store,
        embedder,
        &notes_root,
        KnowledgeScope::GhostNote,
        owner,
    )
    .await?;
    index_diary_tree(settings, store, embedder, &diary_root, ghost_name).await?;

    Ok(())
}

/// DB-driven reference topic indexing.
///
/// Instead of walking the filesystem for `topic.md` files, queries the DB
/// for known topics (shared notes that have reference files) and indexes
/// their reference file directories.
async fn index_reference_topics(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
) -> KnowledgeResult<()> {
    let root = shared_references_root(settings)?;
    if !root.exists() {
        return Ok(());
    }

    // Get known topics from reference_files table joined with notes
    let topics = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT rf.topic_id, n.title \
         FROM reference_files rf \
         JOIN notes n ON n.id = rf.topic_id \
         WHERE n.scope = 'shared_note'",
    )
    .fetch_all(store)
    .await?;

    for (topic_id, title) in topics {
        let topic_dir_name = crate::engine::notes::sanitize_filename(&title);
        let topic_dir = root.join(&topic_dir_name);
        if !topic_dir.exists() {
            continue;
        }

        index_reference_files(settings, store, embedder, &topic_id, &title, &topic_dir).await?;
    }

    Ok(())
}

/// Index reference files by walking the topic directory.
///
/// Discovers files on the filesystem (skipping hidden files),
/// looks up existing roles from the DB, and indexes with context enrichment.
async fn index_reference_files(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    topic_id: &str,
    topic_title: &str,
    topic_dir: &Path,
) -> KnowledgeResult<()> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in WalkDir::new(topic_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let filename = entry.file_name().to_str().unwrap_or("");
        // Skip hidden files and legacy topic.md/_index.md files
        if filename.starts_with('.') || filename == "topic.md" || filename == "_index.md" {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(topic_dir) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            files.push((rel_str, entry.path().to_path_buf()));
        }
    }

    for (rel_path, abs_path) in &files {
        let raw = match tokio::fs::read_to_string(abs_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Look up existing role from DB; default to Code for new files
        let role = sqlx::query_as::<_, (String,)>(
            "SELECT role FROM reference_files WHERE topic_id = ? AND path = ? LIMIT 1",
        )
        .bind(topic_id)
        .bind(rel_path)
        .fetch_optional(store)
        .await?
        .and_then(|(r,)| SourceRole::from_str(&r).ok())
        .unwrap_or(SourceRole::Code);

        let note_id = format!("ref:{}:{}", topic_id, rel_path);
        let title = abs_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(rel_path);
        let entry_type = role.to_entry_type();

        // Context prefix: [topic/subdir] for nested files, [topic] for root-level
        let context_prefix = if let Some(pos) = rel_path.find('/') {
            let subdir = &rel_path[..pos];
            format!("[{}/{}]", topic_title, subdir)
        } else {
            format!("[{}]", topic_title)
        };

        let ingested = ingest_reference_file_with_context(
            settings,
            abs_path,
            &raw,
            &note_id,
            title,
            entry_type,
            Some(&context_prefix),
        )
        .await?;

        if is_unchanged(store, abs_path, &ingested.note.content_hash).await? {
            continue;
        }

        upsert_note(store, &ingested.note).await?;
        let chunk_ids = replace_chunks(
            store,
            &ingested.note.id,
            &ingested.note.title,
            &ingested.note.entry_type,
            ingested.note.archetype.as_deref(),
            &ingested.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;

        // Upsert into reference_files (preserves existing metadata like source_url)
        sqlx::query(
            "INSERT INTO reference_files (topic_id, note_id, path, role) VALUES (?, ?, ?, ?) \
             ON CONFLICT(topic_id, note_id) DO UPDATE SET path = excluded.path, role = excluded.role",
        )
        .bind(topic_id)
        .bind(&ingested.note.id)
        .bind(rel_path)
        .bind(role.as_str())
        .execute(store)
        .await?;
    }

    Ok(())
}

/// Index diary entries — plain markdown files named `YYYY-MM-DD.md` (no front matter).
async fn index_diary_tree(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    root: &Path,
    ghost_name: &str,
) -> KnowledgeResult<()> {
    if !root.exists() {
        return Ok(());
    }

    let date_re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        // Only process files whose stem matches YYYY-MM-DD
        match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) if date_re.is_match(s) => {}
            _ => continue,
        }

        let raw = tokio::fs::read_to_string(path).await?;
        let ingested = ingest_diary_entry(settings, ghost_name, path, &raw).await?;

        if is_unchanged(store, path, &ingested.note.content_hash).await? {
            continue;
        }

        upsert_note(store, &ingested.note).await?;
        replace_links(store, &ingested.note.id, Some(ghost_name), &ingested.links).await?;
        let chunk_ids = replace_chunks(
            store,
            &ingested.note.id,
            &ingested.note.title,
            &ingested.note.entry_type,
            ingested.note.archetype.as_deref(),
            &ingested.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;
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

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
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
        replace_links(
            store,
            &ingested.note.id,
            owner_ghost.as_deref(),
            &ingested.links,
        )
        .await?;
        let chunk_ids = replace_chunks(
            store,
            &ingested.note.id,
            &ingested.note.title,
            &ingested.note.entry_type,
            ingested.note.archetype.as_deref(),
            &ingested.chunks,
        )
        .await?;
        embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;
    }

    Ok(())
}

pub async fn embed_chunks(
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
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT content_hash FROM notes WHERE path = ? LIMIT 1")
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

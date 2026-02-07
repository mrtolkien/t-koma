use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::ingest::{
    ingest_diary_entry, ingest_markdown, ingest_reference_collection,
    ingest_reference_file_with_context, ingest_reference_topic,
};
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
    index_markdown_tree(settings, store, embedder, &root, KnowledgeScope::SharedNote, None).await?;
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

    index_markdown_tree(settings, store, embedder, &notes_root, KnowledgeScope::GhostNote, owner).await?;
    index_diary_tree(settings, store, embedder, &diary_root, ghost_name).await?;

    // TODO: ghost reference indexing — GhostReference scope exists in the enum
    // but index_reference_topics() and reference_search() hardcode SharedReference.
    // Add ghost-scoped reference indexing here when needed.

    Ok(())
}

async fn index_reference_topics(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
) -> KnowledgeResult<()> {
    let root = shared_references_root(settings)?;
    if !root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(&root).into_iter().filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // Only process topic.md files — other .md files in reference dirs
        // are fetched content (no front matter) handled by index_reference_files.
        if path.file_name().and_then(|v| v.to_str()) != Some("topic.md") {
            continue;
        }

        let raw = tokio::fs::read_to_string(path).await?;
        let topic = ingest_reference_topic(settings, path, &raw).await?;

        let note = &topic.note;
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

        let topic_dir = path.parent().unwrap_or(&root);

        // Index _index.md files in subdirectories as ReferenceCollection notes
        let collection_contexts = index_collections(
            settings, store, embedder, &note.note.id, &note.note.title, topic_dir,
        )
        .await?;

        // Walk filesystem for reference files and index them
        index_reference_files(
            settings, store, embedder, &note.note.id, &note.note.title,
            topic_dir, &collection_contexts,
        )
        .await?;
    }

    Ok(())
}

/// Context info for a collection, used for chunk enrichment.
struct CollectionContext {
    /// Directory name of the collection (relative to topic dir).
    dir_name: String,
    /// Context prefix to prepend to chunks: "[Title: Description]"
    prefix: String,
}

/// Index `_index.md` files found in subdirectories of a topic.
///
/// Returns a list of `CollectionContext` entries mapping subdirectory names
/// to their enrichment prefixes.
async fn index_collections(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    topic_id: &str,
    _topic_title: &str,
    topic_dir: &Path,
) -> KnowledgeResult<Vec<CollectionContext>> {
    let mut contexts = Vec::new();

    if !topic_dir.exists() {
        return Ok(contexts);
    }

    let mut entries = tokio::fs::read_dir(topic_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let subdir = entry.path();
        let index_path = subdir.join("_index.md");
        if !index_path.exists() {
            continue;
        }

        let raw = tokio::fs::read_to_string(&index_path).await?;
        let ingested = ingest_reference_collection(settings, &index_path, &raw).await?;

        // Set parent_id to the topic note
        let mut note = ingested.note.clone();
        note.parent_id = Some(topic_id.to_string());

        if !is_unchanged(store, &index_path, &note.content_hash).await? {
            upsert_note(store, &note).await?;
            replace_tags(store, &note.id, &ingested.tags).await?;
            replace_links(store, &note.id, None, &ingested.links).await?;
            let chunk_ids = replace_chunks(
                store,
                &note.id,
                &note.title,
                &note.note_type,
                &ingested.chunks,
            )
            .await?;
            embed_chunks(settings, embedder, store, &ingested.chunks, &chunk_ids).await?;
        }

        // Build context prefix from collection title and body
        let dir_name = subdir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Extract description from the parsed body (first ~200 chars)
        let description = {
            let body = raw
                .split("\n+++\n")
                .nth(1)
                .unwrap_or("")
                .trim();
            if body.is_empty() {
                String::new()
            } else {
                body.chars().take(200).collect::<String>()
            }
        };

        let prefix = if description.is_empty() {
            format!("[{}]", ingested.note.title)
        } else {
            format!("[{}: {}]", ingested.note.title, description)
        };

        contexts.push(CollectionContext { dir_name, prefix });
    }

    Ok(contexts)
}

/// Index reference files by walking the topic directory.
///
/// Discovers files on the filesystem (skipping `topic.md` and `_index.md`),
/// looks up existing roles from the DB, and indexes with context enrichment.
async fn index_reference_files(
    settings: &KnowledgeSettings,
    store: &SqlitePool,
    embedder: &EmbeddingClient,
    topic_id: &str,
    topic_title: &str,
    topic_dir: &Path,
    collection_contexts: &[CollectionContext],
) -> KnowledgeResult<()> {
    // Collect all content files under the topic dir (skip topic.md and _index.md)
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in WalkDir::new(topic_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let filename = entry.file_name().to_str().unwrap_or("");
        if filename == "topic.md" || filename == "_index.md" || filename.starts_with('.') {
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
            Err(_) => continue, // skip binary/unreadable files
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
        let note_type = role.to_note_type();

        let context_prefix = determine_context_prefix(rel_path, topic_title, collection_contexts);

        let ingested = ingest_reference_file_with_context(
            settings, abs_path, &raw, &note_id, title, note_type, Some(&context_prefix),
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
            &ingested.note.note_type,
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

/// Determine the context prefix for a reference file based on its path.
///
/// If the file is inside a collection subdirectory, use that collection's prefix.
/// Otherwise, use the topic title.
fn determine_context_prefix(
    file_path: &str,
    topic_title: &str,
    collection_contexts: &[CollectionContext],
) -> String {
    // Check if the file path starts with a collection directory
    for ctx in collection_contexts {
        if file_path.starts_with(&format!("{}/", ctx.dir_name))
            || file_path.starts_with(&format!("{}\\", ctx.dir_name))
        {
            return ctx.prefix.clone();
        }
    }
    // Root-level file: use topic title
    format!("[{}]", topic_title)
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

    for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
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
            &ingested.note.note_type,
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


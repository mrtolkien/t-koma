use std::path::Path;

use sqlx::SqlitePool;
use walkdir::WalkDir;

use crate::KnowledgeSettings;
use crate::embeddings::EmbeddingClient;
use crate::errors::KnowledgeResult;
use crate::ingest::{ingest_diary_entry, ingest_markdown, ingest_reference_file, ingest_reference_topic};
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

        // Build file → role map from parsed [[sources]] blocks
        let file_roles = build_file_role_map(&topic.sources, &topic.files);

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

        if !topic.files.is_empty() {
            let files_json = serde_json::to_string(&topic.files).unwrap_or_default();
            sqlx::query("INSERT OR REPLACE INTO reference_topics (topic_id, files_json) VALUES (?, ?)")
                .bind(&note.note.id)
                .bind(files_json)
                .execute(store)
                .await?;
            index_reference_files(settings, store, embedder, &note.note.id, path.parent().unwrap_or(&root), &file_roles).await?;
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
    file_roles: &[(String, SourceRole)],
) -> KnowledgeResult<()> {
    for (file, role) in file_roles {
        let path = topic_dir.join(file);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let note_id = format!("ref:{}:{}", topic_id, file);
        let title = path.file_name().and_then(|v| v.to_str()).unwrap_or(file);
        let note_type = role.to_note_type();
        let ingested = ingest_reference_file(settings, &path, &raw, &note_id, title, note_type).await?;

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
            "INSERT OR REPLACE INTO reference_files (topic_id, note_id, path, role) VALUES (?, ?, ?, ?)",
        )
        .bind(topic_id)
        .bind(&ingested.note.id)
        .bind(ingested.note.path.to_string_lossy().to_string())
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

/// Build a (file_name, role) mapping from parsed `[[sources]]` and the flat files list.
///
/// The `files` list in the front matter is flat (no role), but we can infer role
/// from the `[[sources]]` blocks: each source has a list of `paths` (which correspond
/// to fetched files) and an optional `role`. Files not matched to any source default
/// to `SourceRole::Code`.
fn build_file_role_map(
    sources: &[crate::parser::TopicSource],
    files: &[String],
) -> Vec<(String, SourceRole)> {
    // Build a mapping from source to its inferred role.
    // A source's paths list contains path prefixes/patterns, so we check if a
    // file starts with any of the source's paths. For web sources (which produce
    // a single file), the file name typically matches the URL-derived filename.
    let source_roles: Vec<(Option<&[String]>, SourceRole)> = sources
        .iter()
        .map(|src| {
            let role = src
                .role
                .unwrap_or_else(|| SourceRole::infer(&src.source_type));
            (src.paths.as_deref(), role)
        })
        .collect();

    files
        .iter()
        .map(|file| {
            // Try to match the file to a source via its paths filter
            let role = source_roles
                .iter()
                .find(|(paths, _)| match paths {
                    Some(path_list) => path_list
                        .iter()
                        .any(|p| file.starts_with(p.trim_end_matches('/'))),
                    None => true, // source with no paths filter matches all files
                })
                .map(|(_, role)| *role)
                .unwrap_or(SourceRole::Code);
            (file.clone(), role)
        })
        .collect()
}

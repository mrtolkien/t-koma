//! Engine method for saving content to reference topics.
//!
//! `reference_save` is the primary write path for incremental knowledge
//! accumulation. The topic must already exist as a shared note (created via
//! `note_write`), except for the special `_web-cache` topic which is
//! auto-created on first use.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{
    NoteCreateRequest, ReferenceSaveRequest, ReferenceSaveResult, SourceRole, WriteScope,
    generate_note_id,
};

use super::KnowledgeEngine;
use super::notes::sanitize_filename;

/// Save content to a reference topic. The topic note must already exist as a
/// shared note, except `_web-cache` which is auto-created.
pub(crate) async fn reference_save(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    request: ReferenceSaveRequest,
) -> KnowledgeResult<ReferenceSaveResult> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    // 1. Resolve topic — must exist as a shared note (or auto-create _web-cache)
    let (topic_id, topic_title) =
        resolve_or_error(engine, ghost_name, model, &request.topic).await?;

    // 2. Determine filesystem paths
    let reference_root = crate::paths::shared_references_root(settings)?;
    let topic_dir_name = sanitize_filename(&topic_title);
    let topic_dir = reference_root.join(&topic_dir_name);
    tokio::fs::create_dir_all(&topic_dir).await?;

    let file_path = topic_dir.join(&request.path);

    // Create parent directories for nested paths
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // 3. Determine context prefix from path structure
    let context_prefix = if let Some(subdir) = extract_subdir(&request.path) {
        format!("[{}/{}]", topic_title, subdir)
    } else {
        format!("[{}]", topic_title)
    };

    // 4. Write content file
    let tmp_path = file_path.with_extension("tmp");
    tokio::fs::write(&tmp_path, &request.content).await?;
    tokio::fs::rename(&tmp_path, &file_path).await?;

    // 5. Ingest with context enrichment
    let role = request.role.unwrap_or(SourceRole::Docs);
    let entry_type = role.to_entry_type();
    let file_note_id = generate_note_id();
    let title = request.title.as_deref().unwrap_or_else(|| {
        file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
    });

    let ingested = crate::ingest::ingest_reference_file_with_context(
        settings,
        &file_path,
        &request.content,
        &file_note_id,
        title,
        entry_type,
        Some(&context_prefix),
    )
    .await?;

    crate::storage::upsert_note(pool, &ingested.note).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &file_note_id,
        title,
        entry_type,
        None,
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(settings, embedder, pool, &ingested.chunks, &chunk_ids).await?;

    // 6. Insert into reference_files with provenance metadata
    let now = Utc::now();
    sqlx::query(
        "INSERT OR REPLACE INTO reference_files (topic_id, note_id, path, role, source_url, source_type, fetched_at) VALUES (?, ?, ?, ?, ?, 'inline', ?)",
    )
    .bind(&topic_id)
    .bind(&file_note_id)
    .bind(&request.path)
    .bind(role.as_str())
    .bind(request.source_url.as_deref())
    .bind(now.to_rfc3339())
    .execute(pool)
    .await?;

    Ok(ReferenceSaveResult {
        topic_id,
        note_id: file_note_id,
        path: request.path,
    })
}

// ── Internal helpers ────────────────────────────────────────────────

/// Find an existing topic note (shared note with reference files) by fuzzy matching.
///
/// Tries: exact match → case-insensitive → LIKE.
/// Returns `(id, title)` if found, `None` otherwise.
pub(crate) async fn find_existing_topic(
    pool: &SqlitePool,
    name: &str,
) -> KnowledgeResult<Option<(String, String)>> {
    // Exact match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE title = ? AND scope = 'shared_note' LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    if let Some(found) = row {
        return Ok(Some(found));
    }

    // Case-insensitive match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE LOWER(title) = LOWER(?) AND scope = 'shared_note' LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    if let Some(found) = row {
        return Ok(Some(found));
    }

    // LIKE match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE title LIKE ? AND scope = 'shared_note' LIMIT 1",
    )
    .bind(format!("%{}%", name))
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Resolve topic by fuzzy matching, or auto-create `_web-cache`, or error.
async fn resolve_or_error(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    model: &str,
    topic_name: &str,
) -> KnowledgeResult<(String, String)> {
    if let Some(found) = find_existing_topic(engine.pool(), topic_name).await? {
        return Ok(found);
    }

    // Auto-create _web-cache as a system topic note
    if topic_name == "_web-cache" {
        let request = NoteCreateRequest {
            title: "_web-cache".to_string(),
            archetype: None,
            scope: WriteScope::SharedNote,
            body: "Auto-saved web content awaiting curation by reflection.".to_string(),
            parent: None,
            tags: Some(vec!["system".to_string(), "web-cache".to_string()]),
            source: None,
            trust_score: Some(8),
        };
        let result = super::notes::note_create(engine, ghost_name, model, request).await?;
        return Ok((result.note_id, "_web-cache".to_string()));
    }

    Err(KnowledgeError::UnknownNote(format!(
        "Topic note '{}' not found. Create it with note_write first.",
        topic_name
    )))
}

/// Extract the subdirectory component from a path like "bambulab-a1/specs.md".
/// Returns None for root-level files like "specs.md".
fn extract_subdir(path: &str) -> Option<&str> {
    let sep = path.find('/')?;
    let subdir = &path[..sep];
    if subdir.is_empty() {
        None
    } else {
        Some(subdir)
    }
}

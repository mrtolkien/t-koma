//! Engine method for saving content to reference topics.
//!
//! `reference_save` is the primary write path for incremental knowledge
//! accumulation. It creates topics and collections implicitly, writes
//! content files, and indexes everything with context enrichment.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::KnowledgeResult;
use crate::models::{
    KnowledgeScope, ReferenceSaveRequest, ReferenceSaveResult, SourceRole, generate_note_id,
};

use super::KnowledgeEngine;
use super::notes::sanitize_filename;
use super::topics::build_topic_front_matter;

/// Save content to a reference topic, creating the topic and collection if needed.
pub(crate) async fn reference_save(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    request: ReferenceSaveRequest,
) -> KnowledgeResult<ReferenceSaveResult> {
    let pool = engine.pool();
    let settings = engine.settings();
    let embedder = engine.embedder();

    let mut created_topic = false;
    let mut created_collection = false;

    // 1. Resolve or create topic
    let (topic_id, topic_title) = match resolve_or_create_topic(
        pool,
        settings,
        embedder,
        ghost_name,
        &request,
    )
    .await?
    {
        TopicResolution::Existing { id, title } => (id, title),
        TopicResolution::Created { id, title } => {
            created_topic = true;
            (id, title)
        }
    };

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

    // 3. Handle collection creation if path has a subdirectory component
    let context_prefix = if let Some(subdir) = extract_subdir(&request.path) {
        let subdir_path = topic_dir.join(subdir);
        let index_path = subdir_path.join("_index.md");

        if !index_path.exists() {
            // Create _index.md for the collection
            let coll_title = request
                .collection_title
                .as_deref()
                .unwrap_or_else(|| unsanitize_dirname(subdir));
            let coll_description = request.collection_description.as_deref().unwrap_or("");
            let coll_tags = request.collection_tags.as_deref();

            create_collection_index(
                pool,
                settings,
                embedder,
                &topic_id,
                &index_path,
                coll_title,
                coll_description,
                coll_tags,
                ghost_name,
            )
            .await?;
            created_collection = true;
        }

        // Build context prefix from collection
        let coll_title = request
            .collection_title
            .as_deref()
            .unwrap_or_else(|| unsanitize_dirname(subdir));
        let coll_desc = request.collection_description.as_deref().unwrap_or("");
        if coll_desc.is_empty() {
            format!("[{}]", coll_title)
        } else {
            format!("[{}: {}]", coll_title, coll_desc)
        }
    } else {
        // Root-level file: use topic title as context
        format!("[{}]", topic_title)
    };

    // 4. Write content file
    let tmp_path = file_path.with_extension("tmp");
    tokio::fs::write(&tmp_path, &request.content).await?;
    tokio::fs::rename(&tmp_path, &file_path).await?;

    // 5. Ingest with context enrichment
    let role = request.role.unwrap_or(SourceRole::Docs);
    let note_type = role.to_note_type();
    let file_note_id = generate_note_id();
    let title = request
        .title
        .as_deref()
        .unwrap_or_else(|| file_path.file_name().and_then(|n| n.to_str()).unwrap_or("file"));

    let ingested = crate::ingest::ingest_reference_file_with_context(
        settings,
        &file_path,
        &request.content,
        &file_note_id,
        title,
        note_type,
        Some(&context_prefix),
    )
    .await?;

    crate::storage::upsert_note(pool, &ingested.note).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &file_note_id,
        title,
        note_type,
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
        created_topic,
        created_collection,
    })
}

// ── Internal helpers ────────────────────────────────────────────────

enum TopicResolution {
    Existing { id: String, title: String },
    Created { id: String, title: String },
}

/// Resolve topic by fuzzy matching, or create a new one.
async fn resolve_or_create_topic(
    pool: &SqlitePool,
    settings: &crate::KnowledgeSettings,
    embedder: &crate::embeddings::EmbeddingClient,
    ghost_name: &str,
    request: &ReferenceSaveRequest,
) -> KnowledgeResult<TopicResolution> {
    // Exact match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE title = ? AND note_type = 'ReferenceTopic' AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(&request.topic)
    .fetch_optional(pool)
    .await?;
    if let Some((id, title)) = row {
        return Ok(TopicResolution::Existing { id, title });
    }

    // Case-insensitive match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE LOWER(title) = LOWER(?) AND note_type = 'ReferenceTopic' AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(&request.topic)
    .fetch_optional(pool)
    .await?;
    if let Some((id, title)) = row {
        return Ok(TopicResolution::Existing { id, title });
    }

    // LIKE fuzzy match
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM notes WHERE title LIKE ? AND note_type = 'ReferenceTopic' AND scope = 'shared_reference' LIMIT 1",
    )
    .bind(format!("%{}%", request.topic))
    .fetch_optional(pool)
    .await?;
    if let Some((id, title)) = row {
        return Ok(TopicResolution::Existing { id, title });
    }

    // No match — create new topic
    let topic_id = generate_note_id();
    let now = Utc::now();
    let title = request.topic.clone();

    let reference_root = crate::paths::shared_references_root(settings)?;
    let topic_dir_name = sanitize_filename(&title);
    let topic_dir = reference_root.join(&topic_dir_name);
    tokio::fs::create_dir_all(&topic_dir).await?;

    let front_matter = build_topic_front_matter(
        &topic_id,
        &title,
        ghost_name,
        8, // default trust score
        request.tags.as_deref(),
        &now,
    );

    let body = request.topic_description.as_deref().unwrap_or("");
    let content = format!("+++\n{}\n+++\n\n{}\n", front_matter, body);
    let topic_path = topic_dir.join("topic.md");

    let tmp_path = topic_path.with_extension("md.tmp");
    tokio::fs::write(&tmp_path, &content).await?;
    tokio::fs::rename(&tmp_path, &topic_path).await?;

    // Index the topic note
    let ingested = crate::ingest::ingest_markdown(
        settings,
        KnowledgeScope::SharedReference,
        None,
        &topic_path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &topic_id, &ingested.tags).await?;
    crate::storage::replace_links(pool, &topic_id, None, &ingested.links).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &topic_id,
        &title,
        "ReferenceTopic",
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(settings, embedder, pool, &ingested.chunks, &chunk_ids).await?;

    Ok(TopicResolution::Created { id: topic_id, title })
}

/// Create a `_index.md` for a new collection.
#[allow(clippy::too_many_arguments)]
async fn create_collection_index(
    pool: &SqlitePool,
    settings: &crate::KnowledgeSettings,
    embedder: &crate::embeddings::EmbeddingClient,
    topic_id: &str,
    index_path: &std::path::Path,
    title: &str,
    description: &str,
    tags: Option<&[String]>,
    ghost_name: &str,
) -> KnowledgeResult<()> {
    let coll_id = generate_note_id();
    let now = Utc::now();

    let mut lines = Vec::new();
    lines.push(format!("id = \"{}\"", coll_id));
    lines.push(format!("title = \"{}\"", title.replace('"', "\\\"")));
    lines.push("type = \"ReferenceCollection\"".to_string());
    lines.push(format!("created_at = \"{}\"", now.to_rfc3339()));
    lines.push("trust_score = 8".to_string());
    lines.push(format!("parent = \"{}\"", topic_id));
    if let Some(tag_list) = tags {
        let formatted: Vec<String> = tag_list.iter().map(|t| format!("\"{}\"", t)).collect();
        lines.push(format!("tags = [{}]", formatted.join(", ")));
    }
    lines.push(String::new());
    lines.push("[created_by]".to_string());
    lines.push(format!("ghost = \"{}\"", ghost_name));
    lines.push("model = \"tool\"".to_string());

    let front_matter = lines.join("\n");
    let content = format!("+++\n{}\n+++\n\n{}\n", front_matter, description);

    // Create directory and write
    if let Some(parent) = index_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp_path = index_path.with_extension("md.tmp");
    tokio::fs::write(&tmp_path, &content).await?;
    tokio::fs::rename(&tmp_path, index_path).await?;

    // Index the collection note
    let ingested = crate::ingest::ingest_reference_collection(settings, index_path, &content).await?;
    let mut note = ingested.note.clone();
    note.parent_id = Some(topic_id.to_string());
    crate::storage::upsert_note(pool, &note).await?;
    crate::storage::replace_tags(pool, &coll_id, &ingested.tags).await?;
    crate::storage::replace_links(pool, &coll_id, None, &ingested.links).await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &coll_id,
        title,
        "ReferenceCollection",
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(settings, embedder, pool, &ingested.chunks, &chunk_ids).await?;

    Ok(())
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

/// Reverse a sanitized directory name back to a human-readable title.
/// E.g., "bambulab-a1" → "bambulab a1" (hyphens to spaces, title case not applied).
fn unsanitize_dirname(dirname: &str) -> &str {
    // Just return as-is — the user can provide collection_title for nicer names
    dirname
}

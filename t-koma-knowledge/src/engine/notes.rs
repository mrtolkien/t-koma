use chrono::{DateTime, Utc};
use sqlx;

use crate::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{
    KnowledgeScope, NoteCreateRequest, NoteDocument, NoteUpdateRequest, NoteWriteResult,
    OwnershipScope, WriteScope, generate_note_id,
};
use crate::parser::CommentEntry;
use crate::paths::{ghost_notes_root, shared_notes_root};

use super::KnowledgeEngine;

pub(crate) async fn note_create(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    request: NoteCreateRequest,
) -> KnowledgeResult<NoteWriteResult> {
    let note_id = generate_note_id();
    let now = Utc::now();
    let (target_dir, scope, owner_ghost) =
        resolve_write_target(engine.settings(), ghost_name, &request.scope)?;

    // Derive subfolder from first tag (creation-time only, files don't move on tag change)
    let target_dir = if let Some(tags) = &request.tags {
        if let Some(first_tag) = tags.first() {
            target_dir.join(sanitize_tag_path(first_tag))
        } else {
            target_dir
        }
    } else {
        target_dir
    };
    tokio::fs::create_dir_all(&target_dir).await?;

    let trust_score = request.trust_score.unwrap_or(5);
    let front_matter = build_front_matter(
        &note_id,
        &request.title,
        request.archetype.as_deref(),
        ghost_name,
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
    let ingested =
        crate::ingest::ingest_markdown(engine.settings(), scope, owner_ghost, &path, &content)
            .await?;
    let pool = engine.pool();
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &note_id, &ingested.tags).await?;
    crate::storage::replace_links(
        pool,
        &note_id,
        ingested.note.owner_ghost.as_deref(),
        &ingested.links,
    )
    .await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &note_id,
        &ingested.note.title,
        &ingested.note.entry_type,
        ingested.note.archetype.as_deref(),
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(
        engine.settings(),
        engine.embedder(),
        pool,
        &ingested.chunks,
        &chunk_ids,
    )
    .await?;

    Ok(NoteWriteResult { note_id, path })
}

pub(crate) async fn note_update(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    request: NoteUpdateRequest,
) -> KnowledgeResult<NoteWriteResult> {
    // Fetch existing note and verify access
    let doc = engine
        .memory_get(ghost_name, &request.note_id, OwnershipScope::All)
        .await?;
    verify_write_access(ghost_name, &doc)?;

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
        Some(ghost_name.to_string())
    };
    let ingested =
        crate::ingest::ingest_markdown(engine.settings(), scope, owner_ghost, &doc.path, &content)
            .await?;
    let pool = engine.pool();
    crate::storage::upsert_note(pool, &ingested.note).await?;
    crate::storage::replace_tags(pool, &request.note_id, &ingested.tags).await?;
    crate::storage::replace_links(
        pool,
        &request.note_id,
        ingested.note.owner_ghost.as_deref(),
        &ingested.links,
    )
    .await?;
    let chunk_ids = crate::storage::replace_chunks(
        pool,
        &request.note_id,
        &ingested.note.title,
        &ingested.note.entry_type,
        ingested.note.archetype.as_deref(),
        &ingested.chunks,
    )
    .await?;
    crate::index::embed_chunks(
        engine.settings(),
        engine.embedder(),
        pool,
        &ingested.chunks,
        &chunk_ids,
    )
    .await?;

    Ok(NoteWriteResult {
        note_id: request.note_id,
        path: doc.path,
    })
}

pub(crate) async fn note_validate(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    note_id: &str,
    trust_score: Option<i64>,
) -> KnowledgeResult<NoteWriteResult> {
    let doc = engine
        .memory_get(ghost_name, note_id, OwnershipScope::All)
        .await?;
    verify_write_access(ghost_name, &doc)?;

    let raw = tokio::fs::read_to_string(&doc.path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let mut front = parsed.front;

    let now = Utc::now();
    front.last_validated_at = Some(now);
    front.last_validated_by = Some(crate::parser::CreatedBy {
        ghost: ghost_name.to_string(),
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
    let scope = doc.scope;
    let owner_ghost = if scope.is_shared() {
        None
    } else {
        Some(ghost_name.to_string())
    };
    let ingested =
        crate::ingest::ingest_markdown(engine.settings(), scope, owner_ghost, &doc.path, &content)
            .await?;
    crate::storage::upsert_note(engine.pool(), &ingested.note).await?;

    Ok(NoteWriteResult {
        note_id: note_id.to_string(),
        path: doc.path,
    })
}

pub(crate) async fn note_comment(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    note_id: &str,
    text: &str,
) -> KnowledgeResult<NoteWriteResult> {
    let doc = engine
        .memory_get(ghost_name, note_id, OwnershipScope::All)
        .await?;
    verify_write_access(ghost_name, &doc)?;

    let raw = tokio::fs::read_to_string(&doc.path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let mut front = parsed.front;

    let comment = CommentEntry {
        ghost: ghost_name.to_string(),
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
    let scope = doc.scope;
    let owner_ghost = if scope.is_shared() {
        None
    } else {
        Some(ghost_name.to_string())
    };
    let ingested =
        crate::ingest::ingest_markdown(engine.settings(), scope, owner_ghost, &doc.path, &content)
            .await?;
    crate::storage::upsert_note(engine.pool(), &ingested.note).await?;

    Ok(NoteWriteResult {
        note_id: note_id.to_string(),
        path: doc.path,
    })
}

/// Determine the filesystem directory, internal scope, and owner_ghost for writing.
pub(crate) fn resolve_write_target(
    settings: &KnowledgeSettings,
    ghost_name: &str,
    scope: &WriteScope,
) -> KnowledgeResult<(std::path::PathBuf, KnowledgeScope, Option<String>)> {
    match scope {
        WriteScope::SharedNote => {
            let dir = shared_notes_root(settings)?;
            Ok((dir, KnowledgeScope::SharedNote, None))
        }
        WriteScope::GhostNote => {
            let dir = ghost_notes_root(settings, ghost_name)?;
            Ok((dir, KnowledgeScope::GhostNote, Some(ghost_name.to_string())))
        }
    }
}

/// Verify the calling ghost has write access to a note.
pub(crate) fn verify_write_access(ghost_name: &str, doc: &NoteDocument) -> KnowledgeResult<()> {
    if doc.scope.is_shared() {
        // Shared notes are writable by any ghost
        return Ok(());
    }
    // Private notes: only the owner ghost can write
    if doc.created_by_ghost != ghost_name {
        return Err(KnowledgeError::AccessDenied(format!(
            "ghost '{}' cannot modify note owned by '{}'",
            ghost_name, doc.created_by_ghost,
        )));
    }
    Ok(())
}

/// Build TOML front matter for a new note.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_front_matter(
    id: &str,
    title: &str,
    archetype: Option<&str>,
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
    if let Some(arch) = archetype {
        lines.push(format!("archetype = \"{}\"", arch));
    }
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
pub(crate) fn rebuild_front_matter(front: &crate::parser::FrontMatter) -> String {
    let mut lines = Vec::new();
    lines.push(format!("id = \"{}\"", front.id));
    lines.push(format!("title = \"{}\"", front.title.replace('"', "\\\"")));
    // Write archetype if present; fall back to type for reference files
    if let Some(archetype) = &front.archetype {
        lines.push(format!("archetype = \"{}\"", archetype));
    } else if let Some(note_type) = &front.note_type {
        lines.push(format!("type = \"{}\"", note_type));
    }
    lines.push(format!(
        "created_at = \"{}\"",
        front.created_at.to_rfc3339()
    ));
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
        lines.push(format!(
            "last_validated_at = \"{}\"",
            validated_at.to_rfc3339()
        ));
    }
    if let Some(validated_by) = &front.last_validated_by {
        lines.push(String::new());
        lines.push("[last_validated_by]".to_string());
        lines.push(format!("ghost = \"{}\"", validated_by.ghost));
        lines.push(format!("model = \"{}\"", validated_by.model));
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

pub(crate) async fn note_delete(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    note_id: &str,
) -> KnowledgeResult<()> {
    let doc = engine
        .memory_get(ghost_name, note_id, OwnershipScope::All)
        .await?;
    verify_write_access(ghost_name, &doc)?;

    // Delete file from disk
    if doc.path.exists() {
        tokio::fs::remove_file(&doc.path).await?;
    }

    // Delete from DB: chunks (FTS + vec), tags, links, then the note itself
    let pool = engine.pool();
    let existing_chunk_ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .fetch_all(pool)
        .await?;

    sqlx::query("DELETE FROM chunks WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM chunk_fts WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    if !existing_chunk_ids.is_empty() {
        let placeholders = existing_chunk_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM chunk_vec WHERE rowid IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for (chunk_id,) in &existing_chunk_ids {
            q = q.bind(chunk_id);
        }
        q.execute(pool).await?;
    }
    sqlx::query("DELETE FROM note_tags WHERE note_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM note_links WHERE source_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    // Clear inbound link targets so they can re-resolve if a note with the
    // same title is recreated later.
    sqlx::query("UPDATE note_links SET target_id = NULL WHERE target_id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(note_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Sanitize a hierarchical tag (e.g. `rust/library`) into a safe relative path.
///
/// Each `/`-separated segment is stripped of unsafe characters (`..`, absolute
/// prefixes, non-alphanumeric) so the result can never escape the parent
/// directory via path traversal. Empty segments are dropped.
pub(crate) fn sanitize_tag_path(tag: &str) -> std::path::PathBuf {
    let mut path = std::path::PathBuf::new();
    for segment in tag.split('/') {
        let clean: String = segment
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .to_lowercase();
        let clean = clean.trim_matches('-').to_string();
        if !clean.is_empty() && clean != "." && clean != ".." {
            path.push(clean);
        }
    }
    path
}

/// Sanitize a title for use as a filename.
pub(crate) fn sanitize_filename(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

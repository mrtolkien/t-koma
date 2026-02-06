use chrono::{DateTime, Utc};

use crate::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{
    KnowledgeContext, KnowledgeScope, MemoryScope, NoteCreateRequest, NoteDocument,
    NoteUpdateRequest, NoteWriteResult, WriteScope, generate_note_id,
};
use crate::parser::CommentEntry;
use crate::paths::{
    ghost_diary_root, ghost_private_root, ghost_projects_root, shared_knowledge_root,
};

use super::KnowledgeEngine;

pub(crate) async fn note_create(
    engine: &KnowledgeEngine,
    context: &KnowledgeContext,
    request: NoteCreateRequest,
) -> KnowledgeResult<NoteWriteResult> {
    let note_id = generate_note_id();
    let now = Utc::now();
    let (target_dir, scope, owner_ghost) =
        resolve_write_target(context, engine.settings(), &request.scope)?;
    tokio::fs::create_dir_all(&target_dir).await?;

    let trust_score = request.trust_score.unwrap_or(5);
    let front_matter = build_front_matter(
        &note_id,
        &request.title,
        &request.note_type,
        &context.ghost_name,
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
    let ingested = crate::ingest::ingest_markdown(
        engine.settings(),
        scope,
        owner_ghost,
        &path,
        &content,
    )
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
        &ingested.note.note_type,
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
    context: &KnowledgeContext,
    request: NoteUpdateRequest,
) -> KnowledgeResult<NoteWriteResult> {
    // Fetch existing note and verify access
    let doc = engine
        .memory_get(context, &request.note_id, MemoryScope::All)
        .await?;
    verify_write_access(context, &doc)?;

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
        Some(context.ghost_name.clone())
    };
    let ingested = crate::ingest::ingest_markdown(
        engine.settings(),
        scope,
        owner_ghost,
        &doc.path,
        &content,
    )
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
        &ingested.note.note_type,
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
    context: &KnowledgeContext,
    note_id: &str,
    trust_score: Option<i64>,
) -> KnowledgeResult<NoteWriteResult> {
    let doc = engine
        .memory_get(context, note_id, MemoryScope::All)
        .await?;
    verify_write_access(context, &doc)?;

    let raw = tokio::fs::read_to_string(&doc.path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let mut front = parsed.front;

    let now = Utc::now();
    front.last_validated_at = Some(now);
    front.last_validated_by = Some(crate::parser::CreatedBy {
        ghost: context.ghost_name.clone(),
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
        Some(context.ghost_name.clone())
    };
    let ingested = crate::ingest::ingest_markdown(
        engine.settings(),
        scope,
        owner_ghost,
        &doc.path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(engine.pool(), &ingested.note).await?;

    Ok(NoteWriteResult {
        note_id: note_id.to_string(),
        path: doc.path,
    })
}

pub(crate) async fn note_comment(
    engine: &KnowledgeEngine,
    context: &KnowledgeContext,
    note_id: &str,
    text: &str,
) -> KnowledgeResult<NoteWriteResult> {
    let doc = engine
        .memory_get(context, note_id, MemoryScope::All)
        .await?;
    verify_write_access(context, &doc)?;

    let raw = tokio::fs::read_to_string(&doc.path).await?;
    let parsed = crate::parser::parse_note(&raw)?;
    let mut front = parsed.front;

    let comment = CommentEntry {
        ghost: context.ghost_name.clone(),
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
        Some(context.ghost_name.clone())
    };
    let ingested = crate::ingest::ingest_markdown(
        engine.settings(),
        scope,
        owner_ghost,
        &doc.path,
        &content,
    )
    .await?;
    crate::storage::upsert_note(engine.pool(), &ingested.note).await?;

    Ok(NoteWriteResult {
        note_id: note_id.to_string(),
        path: doc.path,
    })
}

/// Determine the filesystem directory, internal scope, and owner_ghost for writing.
pub(crate) fn resolve_write_target(
    context: &KnowledgeContext,
    settings: &KnowledgeSettings,
    scope: &WriteScope,
) -> KnowledgeResult<(std::path::PathBuf, KnowledgeScope, Option<String>)> {
    match scope {
        WriteScope::Shared => {
            let dir = shared_knowledge_root(settings)?;
            Ok((dir, KnowledgeScope::Shared, None))
        }
        WriteScope::Private => {
            let dir = ghost_private_root(&context.workspace_root);
            Ok((
                dir,
                KnowledgeScope::GhostPrivate,
                Some(context.ghost_name.clone()),
            ))
        }
        WriteScope::Projects => {
            let dir = ghost_projects_root(&context.workspace_root);
            Ok((
                dir,
                KnowledgeScope::GhostProjects,
                Some(context.ghost_name.clone()),
            ))
        }
        WriteScope::Diary => {
            let dir = ghost_diary_root(&context.workspace_root);
            Ok((
                dir,
                KnowledgeScope::GhostDiary,
                Some(context.ghost_name.clone()),
            ))
        }
    }
}

/// Verify the calling ghost has write access to a note.
pub(crate) fn verify_write_access(
    context: &KnowledgeContext,
    doc: &NoteDocument,
) -> KnowledgeResult<()> {
    if doc.scope.is_shared() {
        // Shared notes are writable by any ghost
        return Ok(());
    }
    // Private notes: only the owner ghost can write
    if doc.created_by_ghost != context.ghost_name {
        return Err(KnowledgeError::AccessDenied(format!(
            "ghost '{}' cannot modify note owned by '{}'",
            context.ghost_name, doc.created_by_ghost,
        )));
    }
    Ok(())
}

/// Build TOML front matter for a new note.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_front_matter(
    id: &str,
    title: &str,
    note_type: &str,
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
    lines.push(format!("type = \"{}\"", note_type));
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
    lines.push(format!(
        "title = \"{}\"",
        front.title.replace('"', "\\\"")
    ));
    lines.push(format!("type = \"{}\"", front.note_type));
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
    if let Some(files) = &front.files {
        let formatted: Vec<String> = files.iter().map(|f| format!("\"{}\"", f)).collect();
        lines.push(format!("files = [{}]", formatted.join(", ")));
    }
    // Reference topic fields
    if let Some(status) = &front.status {
        lines.push(format!("status = \"{}\"", status));
    }
    if let Some(fetched_at) = front.fetched_at {
        lines.push(format!("fetched_at = \"{}\"", fetched_at.to_rfc3339()));
    }
    if let Some(max_age) = front.max_age_days {
        lines.push(format!("max_age_days = {}", max_age));
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
            lines.push(format!(
                "text = \"{}\"",
                comment.text.replace('"', "\\\"")
            ));
        }
    }
    if let Some(sources) = &front.sources {
        for src in sources {
            lines.push(String::new());
            lines.push("[[sources]]".to_string());
            lines.push(format!("type = \"{}\"", src.source_type));
            lines.push(format!("url = \"{}\"", src.url));
            if let Some(ref_name) = &src.ref_name {
                lines.push(format!("ref = \"{}\"", ref_name));
            }
            if let Some(commit) = &src.commit {
                lines.push(format!("commit = \"{}\"", commit));
            }
            if let Some(paths) = &src.paths {
                let formatted: Vec<String> = paths.iter().map(|p| format!("\"{}\"", p)).collect();
                lines.push(format!("paths = [{}]", formatted.join(", ")));
            }
            if let Some(role) = &src.role {
                lines.push(format!("role = \"{}\"", role));
            }
        }
    }
    lines.join("\n")
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

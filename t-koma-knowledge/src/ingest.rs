use std::path::Path;

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::KnowledgeSettings;
use crate::chunker::{Chunk, chunk_code, chunk_markdown};
use crate::errors::KnowledgeResult;
use crate::models::KnowledgeScope;
use crate::parser::{ParsedNote, extract_links, parse_note};
use crate::storage::{ChunkRecord, NoteRecord};

#[derive(Debug, Clone)]
pub struct IngestedNote {
    pub note: NoteRecord,
    pub chunks: Vec<ChunkRecord>,
    pub links: Vec<(String, Option<String>)>,
    pub tags: Vec<String>,
}

/// Result of ingesting a reference topic's `topic.md`.
pub struct IngestedTopic {
    pub note: IngestedNote,
}

pub async fn ingest_reference_topic(
    settings: &KnowledgeSettings,
    path: &Path,
    raw: &str,
) -> KnowledgeResult<IngestedTopic> {
    let parsed = parse_note(raw)?;
    let hash = compute_hash(raw);

    let note = NoteRecord {
        id: parsed.front.id.clone(),
        title: parsed.front.title.clone(),
        entry_type: "ReferenceTopic".to_string(),
        archetype: None,
        path: path.to_path_buf(),
        scope: KnowledgeScope::SharedReference.as_str().to_string(),
        owner_ghost: None,
        created_at: parsed.front.created_at.to_rfc3339(),
        created_by_ghost: parsed.front.created_by.ghost.clone(),
        created_by_model: parsed.front.created_by.model.clone(),
        trust_score: parsed.front.trust_score,
        last_validated_at: parsed.front.last_validated_at.map(|dt| dt.to_rfc3339()),
        last_validated_by_ghost: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.ghost.clone()),
        last_validated_by_model: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.model.clone()),
        version: parsed.front.version,
        parent_id: parsed.front.parent.clone(),
        comments_json: parsed
            .front
            .comments
            .as_ref()
            .map(|value| serde_json::to_string(value).unwrap_or_default()),
        content_hash: hash,
    };

    let tags = parsed.front.tags.clone().unwrap_or_default();
    let chunks = build_markdown_chunks(settings, &parsed, &note, &tags);
    let links = parsed
        .links
        .iter()
        .map(|link| (link.target.clone(), link.alias.clone()))
        .collect();

    Ok(IngestedTopic {
        note: IngestedNote {
            note,
            chunks,
            links,
            tags,
        },
    })
}

pub async fn ingest_markdown(
    settings: &KnowledgeSettings,
    scope: KnowledgeScope,
    owner_ghost: Option<String>,
    path: &Path,
    raw: &str,
) -> KnowledgeResult<IngestedNote> {
    let parsed = parse_note(raw)?;
    let hash = compute_hash(raw);

    // For note scopes, entry_type is always "Note" and the semantic
    // classification lives in archetype. For reference scopes, preserve
    // the structural type from front matter (ReferenceTopic, etc.).
    let (entry_type, archetype) = if scope.is_note() {
        ("Note".to_string(), parsed.front.effective_archetype())
    } else {
        let et = parsed.front.effective_type().unwrap_or("Note").to_string();
        (et, None)
    };

    let note = NoteRecord {
        id: parsed.front.id.clone(),
        title: parsed.front.title.clone(),
        entry_type,
        archetype,
        path: path.to_path_buf(),
        scope: scope.as_str().to_string(),
        owner_ghost,
        created_at: parsed.front.created_at.to_rfc3339(),
        created_by_ghost: parsed.front.created_by.ghost.clone(),
        created_by_model: parsed.front.created_by.model.clone(),
        trust_score: parsed.front.trust_score,
        last_validated_at: parsed.front.last_validated_at.map(|dt| dt.to_rfc3339()),
        last_validated_by_ghost: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.ghost.clone()),
        last_validated_by_model: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.model.clone()),
        version: parsed.front.version,
        parent_id: parsed.front.parent.clone(),
        comments_json: parsed
            .front
            .comments
            .as_ref()
            .map(|value| serde_json::to_string(value).unwrap_or_default()),
        content_hash: hash,
    };

    let tags = parsed.front.tags.clone().unwrap_or_default();
    let chunks = build_markdown_chunks(settings, &parsed, &note, &tags);
    let links = parsed
        .links
        .iter()
        .map(|link| (link.target.clone(), link.alias.clone()))
        .collect();

    Ok(IngestedNote {
        note,
        chunks,
        links,
        tags,
    })
}

pub async fn ingest_reference_file(
    settings: &KnowledgeSettings,
    path: &Path,
    raw: &str,
    note_id: &str,
    title: &str,
    note_type: &str,
) -> KnowledgeResult<IngestedNote> {
    ingest_reference_file_with_context(settings, path, raw, note_id, title, note_type, None).await
}

/// Ingest a reference file with optional context prefix for chunk enrichment.
///
/// When `context_prefix` is provided, it is prepended to each chunk's content
/// before hashing and embedding. This ensures search queries about the parent
/// collection/topic find file chunks even when the raw file content doesn't
/// mention the collection name. The raw file on disk stays unchanged.
pub async fn ingest_reference_file_with_context(
    settings: &KnowledgeSettings,
    path: &Path,
    raw: &str,
    note_id: &str,
    title: &str,
    note_type: &str,
    context_prefix: Option<&str>,
) -> KnowledgeResult<IngestedNote> {
    let hash = compute_hash(raw);
    let note = NoteRecord {
        id: note_id.to_string(),
        title: title.to_string(),
        entry_type: note_type.to_string(),
        archetype: None,
        path: path.to_path_buf(),
        scope: KnowledgeScope::SharedReference.as_str().to_string(),
        owner_ghost: None,
        created_at: Utc::now().to_rfc3339(),
        created_by_ghost: "system".to_string(),
        created_by_model: "system".to_string(),
        trust_score: 10,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: hash,
    };

    let chunks = if path.extension().and_then(|v| v.to_str()) == Some("md") {
        chunk_markdown(raw)
    } else {
        match chunk_code(raw, path) {
            Ok(chunks) => chunks,
            Err(_) => vec![Chunk {
                title: "file".to_string(),
                content: raw.to_string(),
                index: 0,
            }],
        }
    };

    let chunk_records = chunks
        .into_iter()
        .map(|chunk| {
            let enriched_content = match context_prefix {
                Some(prefix) => format!("{}\n\n{}", prefix, chunk.content),
                None => chunk.content.clone(),
            };
            ChunkRecord {
                note_id: note.id.clone(),
                chunk_index: chunk.index as i64,
                title: chunk.title,
                content: enriched_content.clone(),
                content_hash: compute_hash(&enriched_content),
                embedding_model: Some(settings.embedding_model.clone()),
                embedding_dim: settings.embedding_dim.map(|d| d as i64),
            }
        })
        .collect();

    Ok(IngestedNote {
        note,
        chunks: chunk_records,
        links: Vec::new(),
        tags: Vec::new(),
    })
}

/// Ingest a `_index.md` file as a `ReferenceCollection` note.
///
/// Collections are sub-groupings within a reference topic. Their `_index.md`
/// files have front matter with `type = "ReferenceCollection"` and are indexed
/// for search just like topic notes.
pub async fn ingest_reference_collection(
    settings: &KnowledgeSettings,
    path: &Path,
    raw: &str,
) -> KnowledgeResult<IngestedNote> {
    let parsed = parse_note(raw)?;
    let hash = compute_hash(raw);

    let note = NoteRecord {
        id: parsed.front.id.clone(),
        title: parsed.front.title.clone(),
        entry_type: "ReferenceCollection".to_string(),
        archetype: None,
        path: path.to_path_buf(),
        scope: KnowledgeScope::SharedReference.as_str().to_string(),
        owner_ghost: None,
        created_at: parsed.front.created_at.to_rfc3339(),
        created_by_ghost: parsed.front.created_by.ghost.clone(),
        created_by_model: parsed.front.created_by.model.clone(),
        trust_score: parsed.front.trust_score,
        last_validated_at: parsed.front.last_validated_at.map(|dt| dt.to_rfc3339()),
        last_validated_by_ghost: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.ghost.clone()),
        last_validated_by_model: parsed
            .front
            .last_validated_by
            .as_ref()
            .map(|entry| entry.model.clone()),
        version: parsed.front.version,
        parent_id: parsed.front.parent.clone(),
        comments_json: parsed
            .front
            .comments
            .as_ref()
            .map(|value| serde_json::to_string(value).unwrap_or_default()),
        content_hash: hash,
    };

    let tags = parsed.front.tags.clone().unwrap_or_default();
    let chunks = build_markdown_chunks(settings, &parsed, &note, &tags);
    let links = parsed
        .links
        .iter()
        .map(|link| (link.target.clone(), link.alias.clone()))
        .collect();

    Ok(IngestedNote {
        note,
        chunks,
        links,
        tags,
    })
}

/// Ingest a diary entry — a plain markdown file with no front matter.
///
/// The filename must be `YYYY-MM-DD.md`. A deterministic note ID is generated
/// as `diary:{ghost}:{date}` so re-indexing the same file produces an upsert.
pub async fn ingest_diary_entry(
    settings: &KnowledgeSettings,
    owner_ghost: &str,
    path: &Path,
    raw: &str,
) -> KnowledgeResult<IngestedNote> {
    let date = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
        crate::errors::KnowledgeError::InvalidFrontMatter(format!(
            "diary file has no stem: {}",
            path.display()
        ))
    })?;

    // Validate YYYY-MM-DD format
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(crate::errors::KnowledgeError::InvalidFrontMatter(format!(
            "diary filename is not YYYY-MM-DD: {date}"
        )));
    }

    let id = format!("diary:{owner_ghost}:{date}");
    let hash = compute_hash(raw);

    let note = NoteRecord {
        id,
        title: date.to_string(),
        entry_type: "Diary".to_string(),
        archetype: None,
        path: path.to_path_buf(),
        scope: KnowledgeScope::GhostDiary.as_str().to_string(),
        owner_ghost: Some(owner_ghost.to_string()),
        created_at: Utc::now().to_rfc3339(),
        created_by_ghost: owner_ghost.to_string(),
        created_by_model: "unknown".to_string(),
        trust_score: 10,
        last_validated_at: None,
        last_validated_by_ghost: None,
        last_validated_by_model: None,
        version: None,
        parent_id: None,
        comments_json: None,
        content_hash: hash,
    };

    let chunks = chunk_markdown(raw);
    let chunk_records = chunks
        .into_iter()
        .map(|chunk| ChunkRecord {
            note_id: note.id.clone(),
            chunk_index: chunk.index as i64,
            title: chunk.title,
            content: chunk.content.clone(),
            content_hash: compute_hash(&chunk.content),
            embedding_model: Some(settings.embedding_model.clone()),
            embedding_dim: settings.embedding_dim.map(|d| d as i64),
        })
        .collect();

    let links = extract_links(raw)
        .into_iter()
        .map(|link| (link.target, link.alias))
        .collect();

    Ok(IngestedNote {
        note,
        chunks: chunk_records,
        links,
        tags: Vec::new(),
    })
}

fn build_markdown_chunks(
    settings: &KnowledgeSettings,
    parsed: &ParsedNote,
    note: &NoteRecord,
    tags: &[String],
) -> Vec<ChunkRecord> {
    let chunks = chunk_markdown(&parsed.body);
    let tag_prefix = if !tags.is_empty() {
        Some(format!("[tags: {}]", tags.join(", ")))
    } else {
        None
    };

    chunks
        .into_iter()
        .map(|chunk| {
            // Prepend tags to the first chunk for FTS and embedding search
            let content = if chunk.index == 0 {
                if let Some(ref prefix) = tag_prefix {
                    format!("{}\n\n{}", prefix, chunk.content)
                } else {
                    chunk.content.clone()
                }
            } else {
                chunk.content.clone()
            };
            ChunkRecord {
                note_id: note.id.clone(),
                chunk_index: chunk.index as i64,
                title: chunk.title,
                content: content.clone(),
                content_hash: compute_hash(&content),
                embedding_model: Some(settings.embedding_model.clone()),
                embedding_dim: settings.embedding_dim.map(|d| d as i64),
            }
        })
        .collect()
}

fn compute_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

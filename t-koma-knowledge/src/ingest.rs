use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::chunker::{chunk_code, chunk_markdown, Chunk};
use crate::errors::KnowledgeResult;
use crate::models::KnowledgeScope;
use crate::parser::{parse_note, ParsedNote};
use crate::paths::types_allowlist_path;
use crate::storage::{ChunkRecord, NoteRecord};
use crate::KnowledgeSettings;

#[derive(Debug, Clone)]
pub struct IngestedNote {
    pub note: NoteRecord,
    pub chunks: Vec<ChunkRecord>,
    pub links: Vec<(String, Option<String>)>,
    pub tags: Vec<String>,
}

pub async fn ingest_reference_topic(
    settings: &KnowledgeSettings,
    path: &Path,
    raw: &str,
) -> KnowledgeResult<(IngestedNote, Vec<String>)> {
    let parsed = parse_note(raw)?;
    let hash = compute_hash(raw);

    let note = NoteRecord {
        id: parsed.front.id.clone(),
        title: parsed.front.title.clone(),
        note_type: "ReferenceTopic".to_string(),
        type_valid: true,
        path: path.to_path_buf(),
        scope: KnowledgeScope::Reference.as_str().to_string(),
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
        comments_json: parsed.front.comments.as_ref().map(|value| {
            serde_json::to_string(value).unwrap_or_default()
        }),
        content_hash: hash,
    };

    let chunks = build_markdown_chunks(settings, &parsed, &note);
    let links = parsed
        .links
        .iter()
        .map(|link| (link.target.clone(), link.alias.clone()))
        .collect();
    let tags = parsed.front.tags.clone().unwrap_or_default();
    let files = parsed.front.files.clone().unwrap_or_default();

    Ok((
        IngestedNote {
            note,
            chunks,
            links,
            tags,
        },
        files,
    ))
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
    let type_allowlist = load_type_allowlist(settings).await?;

    let type_valid = type_allowlist
        .as_ref()
        .map(|set| set.contains(parsed.front.note_type.as_str()))
        .unwrap_or(true);

    let note = NoteRecord {
        id: parsed.front.id.clone(),
        title: parsed.front.title.clone(),
        note_type: parsed.front.note_type.clone(),
        type_valid,
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
        comments_json: parsed.front.comments.as_ref().map(|value| {
            serde_json::to_string(value).unwrap_or_default()
        }),
        content_hash: hash,
    };

    let chunks = build_markdown_chunks(settings, &parsed, &note);
    let links = parsed
        .links
        .iter()
        .map(|link| (link.target.clone(), link.alias.clone()))
        .collect();
    let tags = parsed.front.tags.clone().unwrap_or_default();

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
    let hash = compute_hash(raw);
    let note = NoteRecord {
        id: note_id.to_string(),
        title: title.to_string(),
        note_type: note_type.to_string(),
        type_valid: true,
        path: path.to_path_buf(),
        scope: KnowledgeScope::Reference.as_str().to_string(),
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

    Ok(IngestedNote {
        note,
        chunks: chunk_records,
        links: Vec::new(),
        tags: Vec::new(),
    })
}

fn build_markdown_chunks(
    settings: &KnowledgeSettings,
    parsed: &ParsedNote,
    note: &NoteRecord,
) -> Vec<ChunkRecord> {
    let chunks = chunk_markdown(&parsed.body);
    chunks
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
        .collect()
}

fn compute_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

async fn load_type_allowlist(settings: &KnowledgeSettings) -> KnowledgeResult<Option<HashSet<String>>> {
    let path = types_allowlist_path(settings)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    let value: TypeAllowList = toml::from_str(&raw)?;
    Ok(Some(value.types.into_iter().collect()))
}


#[derive(Debug, serde::Deserialize)]
struct TypeAllowList {
    pub types: Vec<String>,
}

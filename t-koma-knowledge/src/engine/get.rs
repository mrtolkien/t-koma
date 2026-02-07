use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::{KnowledgeScope, NoteDocument, OwnershipScope, WriteScope};
use crate::paths::ghost_inbox_path;

use super::KnowledgeEngine;
use super::search::resolve_scopes;

pub(crate) async fn memory_get(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    note_id_or_title: &str,
    scope: OwnershipScope,
) -> KnowledgeResult<NoteDocument> {
    let scopes = resolve_scopes(&scope);
    for scope in scopes {
        let doc = fetch_note(engine.pool(), note_id_or_title, scope, ghost_name).await?;
        if let Some(doc) = doc {
            return Ok(doc);
        }
    }

    Err(KnowledgeError::UnknownNote(note_id_or_title.to_string()))
}

pub(crate) async fn memory_capture(
    engine: &KnowledgeEngine,
    ghost_name: &str,
    payload: &str,
    scope: WriteScope,
    source: Option<&str>,
) -> KnowledgeResult<String> {
    let settings = engine.settings();
    let target_path = match scope {
        WriteScope::SharedNote => crate::paths::shared_notes_root(settings)?,
        WriteScope::GhostNote => ghost_inbox_path(settings, ghost_name)?,
    };
    tokio::fs::create_dir_all(&target_path).await?;

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let slug = source.map(slugify).unwrap_or_else(|| "inbox".to_string());
    let file_name = format!("{}-{}.md", timestamp, slug);
    let path = target_path.join(file_name);

    let content = if let Some(src) = source {
        format!("+++\nsource = \"{}\"\n+++\n\n{}", src, payload)
    } else {
        payload.to_string()
    };
    tokio::fs::write(&path, content).await?;

    Ok(path.to_string_lossy().to_string())
}

/// Convert a source string into a filename-safe slug.
///
/// Lowercase, replace non-alphanumeric chars with hyphens, collapse runs,
/// trim edges, truncate to 40 chars.
fn slugify(s: &str) -> String {
    let mut slug = String::with_capacity(s.len());
    let mut prev_hyphen = true; // suppress leading hyphens
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    // Trim trailing hyphen
    while slug.ends_with('-') {
        slug.pop();
    }
    slug.truncate(40);
    // Trim at last full word boundary if we landed mid-word
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "inbox".to_string()
    } else {
        slug
    }
}

pub(crate) async fn fetch_note(
    pool: &SqlitePool,
    note_id_or_title: &str,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Option<NoteDocument>> {
    let row = if scope.is_shared() {
        sqlx::query_as::<_, (
            String,
            String,
            String,
            String,
            String,
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        )>(
            r#"SELECT id, title, note_type, path, scope, trust_score, created_at, created_by_ghost,
                      created_by_model, last_validated_at, last_validated_by_ghost, last_validated_by_model,
                      version, parent_id, comments_json
               FROM notes
               WHERE (id = ? OR title = ?) AND scope = ? AND owner_ghost IS NULL
               LIMIT 1"#,
        )
        .bind(note_id_or_title)
        .bind(note_id_or_title)
        .bind(scope.as_str())
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, (
            String,
            String,
            String,
            String,
            String,
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        )>(
            r#"SELECT id, title, note_type, path, scope, trust_score, created_at, created_by_ghost,
                      created_by_model, last_validated_at, last_validated_by_ghost, last_validated_by_model,
                      version, parent_id, comments_json
               FROM notes
               WHERE (id = ? OR title = ?) AND scope = ? AND owner_ghost = ?
               LIMIT 1"#,
        )
        .bind(note_id_or_title)
        .bind(note_id_or_title)
        .bind(scope.as_str())
        .bind(ghost_name)
        .fetch_optional(pool)
        .await?
    };

    if let Some((
        id,
        title,
        note_type,
        path,
        scope,
        trust_score,
        created_at,
        created_by_ghost,
        created_by_model,
        last_validated_at,
        last_validated_by_ghost,
        last_validated_by_model,
        version,
        parent_id,
        comments_json,
    )) = row
    {
        let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        return Ok(Some(NoteDocument {
            id,
            title,
            note_type,
            path: path.into(),
            scope: scope.parse().unwrap_or(KnowledgeScope::SharedNote),
            trust_score,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            created_by_ghost,
            created_by_model,
            last_validated_at: last_validated_at
                .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_validated_by_ghost,
            last_validated_by_model,
            version,
            parent_id,
            comments_json,
            body,
        }));
    }

    Ok(None)
}

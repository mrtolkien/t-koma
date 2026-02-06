use std::path::PathBuf;

use crate::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};

/// Resolve the root data directory for knowledge storage.
///
/// Priority: settings override > `T_KOMA_DATA_DIR` env > XDG data dir.
pub fn data_root(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(override_dir) = &settings.data_root_override {
        return Ok(override_dir.clone());
    }
    if let Ok(override_dir) = std::env::var("T_KOMA_DATA_DIR") {
        return Ok(PathBuf::from(override_dir));
    }

    let dir = dirs::data_dir().ok_or(KnowledgeError::MissingDataDir)?;
    Ok(dir.join("t-koma"))
}

// ── Shared paths ────────────────────────────────────────────────────

/// Root directory for shared notes: `$DATA/shared/notes/`
pub fn shared_notes_root(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?.join("shared").join("notes"))
}

/// Root directory for shared reference topics: `$DATA/shared/references/`
pub fn shared_references_root(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?.join("shared").join("references"))
}

// ── Ghost paths ─────────────────────────────────────────────────────

/// Ghost inbox (not indexed): `$DATA/ghosts/$slug/inbox/`
pub fn ghost_inbox_path(settings: &KnowledgeSettings, slug: &str) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?.join("ghosts").join(slug).join("inbox"))
}

/// Ghost notes root: `$DATA/ghosts/$slug/notes/`
pub fn ghost_notes_root(settings: &KnowledgeSettings, slug: &str) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?.join("ghosts").join(slug).join("notes"))
}

/// Ghost reference topics root: `$DATA/ghosts/$slug/references/`
pub fn ghost_references_root(settings: &KnowledgeSettings, slug: &str) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?
        .join("ghosts")
        .join(slug)
        .join("references"))
}

/// Ghost diary root: `$DATA/ghosts/$slug/diary/`
pub fn ghost_diary_root(settings: &KnowledgeSettings, slug: &str) -> KnowledgeResult<PathBuf> {
    Ok(data_root(settings)?.join("ghosts").join(slug).join("diary"))
}

// ── Database path ───────────────────────────────────────────────────

/// Knowledge index database: `$DATA/shared/index.sqlite3`
pub fn knowledge_db_path(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.knowledge_db_path_override {
        return Ok(path.clone());
    }
    Ok(data_root(settings)?.join("shared").join("index.sqlite3"))
}

/// Types allowlist file: `$DATA/shared/notes/types.toml`
pub fn types_allowlist_path(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.types_allowlist_path {
        return Ok(path.clone());
    }
    Ok(shared_notes_root(settings)?.join("types.toml"))
}

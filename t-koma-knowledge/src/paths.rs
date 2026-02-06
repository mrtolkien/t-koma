use std::path::{Path, PathBuf};

use crate::config::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};

pub const KNOWLEDGE_DIR: &str = "knowledge";
pub const REFERENCE_DIR: &str = "reference";

pub fn data_root() -> KnowledgeResult<PathBuf> {
    if let Ok(override_dir) = std::env::var("T_KOMA_DATA_DIR") {
        return Ok(PathBuf::from(override_dir));
    }

    let dir = dirs::data_dir().ok_or(KnowledgeError::MissingDataDir)?;
    Ok(dir.join("t-koma"))
}

pub fn shared_knowledge_root(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.shared_root_override {
        return Ok(path.clone());
    }
    Ok(data_root()?.join(KNOWLEDGE_DIR))
}

pub fn reference_root(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.reference_root_override {
        return Ok(path.clone());
    }
    Ok(data_root()?.join(REFERENCE_DIR))
}

pub fn knowledge_db_path(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.knowledge_db_path_override {
        return Ok(path.clone());
    }
    Ok(shared_knowledge_root(settings)?.join("index.sqlite3"))
}

pub fn types_allowlist_path(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    if let Some(path) = &settings.types_allowlist_path {
        return Ok(path.clone());
    }
    Ok(shared_knowledge_root(settings)?.join("types.toml"))
}

pub fn shared_inbox_path(settings: &KnowledgeSettings) -> KnowledgeResult<PathBuf> {
    Ok(shared_knowledge_root(settings)?.join("inbox"))
}

pub fn ghost_inbox_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("private_knowledge").join("inbox")
}

pub fn ghost_projects_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("projects")
}

pub fn ghost_diary_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("diary")
}

pub fn ghost_private_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join("private_knowledge")
}

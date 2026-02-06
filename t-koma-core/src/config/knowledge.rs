//! Knowledge system configuration types.
//!
//! These types define the resolved (non-optional) settings used by
//! `t-koma-knowledge`. They are created from the user-facing
//! `KnowledgeToolsSettings` TOML structs via `From`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::settings::{KnowledgeSearchSettings, KnowledgeToolsSettings};

/// Resolved knowledge engine settings (all values filled with defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSettings {
    #[serde(default = "default_embedding_url")]
    pub embedding_url: String,
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    #[serde(default)]
    pub embedding_dim: Option<usize>,
    #[serde(default = "default_embedding_batch")]
    pub embedding_batch: usize,
    #[serde(default = "default_reconcile_seconds")]
    pub reconcile_seconds: u64,
    #[serde(default)]
    pub types_allowlist_path: Option<PathBuf>,
    #[serde(default)]
    pub knowledge_db_path_override: Option<PathBuf>,
    /// Override the root data directory for all knowledge paths.
    /// When set, all paths (shared notes, references, ghost dirs) derive from
    /// this root instead of `T_KOMA_DATA_DIR` / XDG. Primarily for testing.
    #[serde(default)]
    pub data_root_override: Option<PathBuf>,
    #[serde(default)]
    pub search: SearchDefaults,
}

impl Default for KnowledgeSettings {
    fn default() -> Self {
        Self {
            embedding_url: default_embedding_url(),
            embedding_model: default_embedding_model(),
            embedding_dim: None,
            embedding_batch: default_embedding_batch(),
            reconcile_seconds: default_reconcile_seconds(),
            types_allowlist_path: None,
            knowledge_db_path_override: None,
            data_root_override: None,
            search: SearchDefaults::default(),
        }
    }
}

/// Resolved search tuning knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDefaults {
    #[serde(default = "default_rrf_k")]
    pub rrf_k: usize,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_graph_depth")]
    pub graph_depth: u8,
    #[serde(default = "default_graph_max")]
    pub graph_max: usize,
    #[serde(default = "default_bm25_limit")]
    pub bm25_limit: usize,
    #[serde(default = "default_dense_limit")]
    pub dense_limit: usize,
    /// Boost multiplier for documentation files in reference search.
    #[serde(default = "default_doc_boost")]
    pub doc_boost: f32,
}

impl Default for SearchDefaults {
    fn default() -> Self {
        Self {
            rrf_k: default_rrf_k(),
            max_results: default_max_results(),
            graph_depth: default_graph_depth(),
            graph_max: default_graph_max(),
            bm25_limit: default_bm25_limit(),
            dense_limit: default_dense_limit(),
            doc_boost: default_doc_boost(),
        }
    }
}

fn default_embedding_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

fn default_embedding_model() -> String {
    "qwen3-embedding:8b".to_string()
}

fn default_embedding_batch() -> usize {
    32
}

fn default_reconcile_seconds() -> u64 {
    300
}

fn default_rrf_k() -> usize {
    60
}

fn default_max_results() -> usize {
    8
}

fn default_graph_depth() -> u8 {
    1
}

fn default_graph_max() -> usize {
    20
}

fn default_bm25_limit() -> usize {
    20
}

fn default_dense_limit() -> usize {
    20
}

fn default_doc_boost() -> f32 {
    1.5
}

impl From<&KnowledgeToolsSettings> for KnowledgeSettings {
    fn from(value: &KnowledgeToolsSettings) -> Self {
        let mut settings = KnowledgeSettings::default();
        if let Some(url) = &value.embedding_url {
            settings.embedding_url = url.clone();
        }
        if let Some(model) = &value.embedding_model {
            settings.embedding_model = model.clone();
        }
        if let Some(dim) = value.embedding_dim {
            settings.embedding_dim = Some(dim);
        }
        if let Some(batch) = value.embedding_batch {
            settings.embedding_batch = batch;
        }
        if let Some(seconds) = value.reconcile_seconds {
            settings.reconcile_seconds = seconds;
        }
        if let Some(path) = &value.types_allowlist_path {
            settings.types_allowlist_path = Some(PathBuf::from(path));
        }
        if let Some(path) = &value.knowledge_db_path_override {
            settings.knowledge_db_path_override = Some(PathBuf::from(path));
        }
        apply_search_overrides(&mut settings.search, &value.search);
        settings
    }
}

fn apply_search_overrides(search: &mut SearchDefaults, overrides: &KnowledgeSearchSettings) {
    if let Some(rrf_k) = overrides.rrf_k {
        search.rrf_k = rrf_k;
    }
    if let Some(max_results) = overrides.max_results {
        search.max_results = max_results;
    }
    if let Some(graph_depth) = overrides.graph_depth {
        search.graph_depth = graph_depth;
    }
    if let Some(graph_max) = overrides.graph_max {
        search.graph_max = graph_max;
    }
    if let Some(bm25_limit) = overrides.bm25_limit {
        search.bm25_limit = bm25_limit;
    }
    if let Some(dense_limit) = overrides.dense_limit {
        search.dense_limit = dense_limit;
    }
    if let Some(doc_boost) = overrides.doc_boost {
        search.doc_boost = doc_boost;
    }
}

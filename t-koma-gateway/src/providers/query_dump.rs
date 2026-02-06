//! Debug query logging for LLM provider requests and responses.
//!
//! When enabled via `dump_queries = true` in `[logging]` config, writes raw
//! JSON to `./logs/queries/{timestamp}-{provider}-{model}.{phase}.json`.
//! Request and response share the same base name so they sort together.
//! Failures are logged as warnings but never block the request.

use std::path::PathBuf;

use chrono::Utc;
use serde_json::Value;
use tracing::warn;

const QUERY_DIR: &str = "./logs/queries";

/// Handle for a query dump session, pairing request and response files.
pub struct QueryDump {
    base: PathBuf,
}

impl QueryDump {
    /// Dump the request JSON and return a handle for the paired response.
    ///
    /// Writes `{base}.request.json` where base is
    /// `{timestamp}-{provider}-{model}`.
    pub async fn request(provider: &str, model: &str, value: &Value) -> Option<Self> {
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S%.3f");
        let model_safe = sanitize_model(model);
        let base = PathBuf::from(QUERY_DIR)
            .join(format!("{}-{}-{}", timestamp, provider, model_safe));

        let dir = std::path::Path::new(QUERY_DIR);
        if let Err(e) = tokio::fs::create_dir_all(dir).await {
            warn!("dump_queries: failed to create dir: {}", e);
            return None;
        }

        let path = base.with_extension("request.json");
        write_json(&path, value).await;

        Some(Self { base })
    }

    /// Dump the response JSON paired with the earlier request.
    ///
    /// Writes `{base}.response.json` using the same base name.
    pub async fn response(&self, value: &Value) {
        let path = self.base.with_extension("response.json");
        write_json(&path, value).await;
    }
}

/// Sanitize a model name for safe use in filenames.
fn sanitize_model(model: &str) -> String {
    model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Write a pretty-printed JSON value to a file, warning on failure.
async fn write_json(path: &std::path::Path, value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(json_str) => {
            if let Err(e) = tokio::fs::write(path, json_str).await {
                warn!("dump_queries: failed to write {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            warn!("dump_queries: failed to serialize: {}", e);
        }
    }
}

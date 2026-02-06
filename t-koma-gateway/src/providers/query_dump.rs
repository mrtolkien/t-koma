//! Debug query logging for LLM provider requests and responses.
//!
//! When enabled via `dump_queries = true` in `[logging]` config, writes raw
//! JSON to `./logs/queries/{timestamp}-{provider}-{model}-{phase}.json`.
//! Failures are logged as warnings but never block the request.

use chrono::Utc;
use serde_json::Value;
use tracing::warn;

/// Dump a JSON value to the query log directory.
///
/// # Arguments
/// * `provider` - Provider name (e.g., "anthropic", "openrouter")
/// * `model` - Model identifier
/// * `phase` - Phase label (e.g., "request", "response")
/// * `value` - The JSON value to dump
pub async fn dump_query(provider: &str, model: &str, phase: &str, value: &Value) {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S%.3f");
    // Sanitize model name for filename safety
    let model_safe: String = model
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let filename = format!("{}-{}-{}-{}.json", timestamp, provider, model_safe, phase);
    let dir = std::path::Path::new("./logs/queries");

    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        warn!("dump_queries: failed to create dir: {}", e);
        return;
    }

    let path = dir.join(filename);
    match serde_json::to_string_pretty(value) {
        Ok(json_str) => {
            if let Err(e) = tokio::fs::write(&path, json_str).await {
                warn!("dump_queries: failed to write {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            warn!("dump_queries: failed to serialize: {}", e);
        }
    }
}

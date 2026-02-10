use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{Tool, ToolContext};
use crate::web::fetch::{FetchError, WebFetchRequest, WebFetchService, http::HttpFetchProvider};

/// Generate a filename from a URL for web-cache dedup.
pub(crate) fn url_to_cache_filename(url: &str, ext: &str) -> String {
    use std::hash::{Hash, Hasher};
    let slug: String = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .chars()
        .take(60)
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .to_lowercase();
    let mut hasher = std::hash::DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{}-{:08x}.{}", slug, hash as u32, ext)
}

#[derive(Debug, Deserialize)]
struct WebFetchInput {
    url: String,
    mode: Option<String>,
    max_chars: Option<usize>,
    #[serde(default)]
    raw: bool,
}

pub struct WebFetchTool;

impl WebFetchTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "mode": {"type": "string", "enum": ["text", "markdown"]},
                "max_chars": {"type": "integer", "minimum": 1},
                "raw": {"type": "boolean", "description": "Return full page content instead of extracted article. Default false."}
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    fn format_error(err: FetchError) -> String {
        match err {
            FetchError::Disabled => "web_fetch is disabled in configuration".to_string(),
            FetchError::UnsupportedProvider(provider) => format!(
                "web_fetch provider '{}' is not supported in this build",
                provider
            ),
            FetchError::InvalidUrl => "invalid URL for web_fetch".to_string(),
            FetchError::UnsupportedContentType(ct) => {
                format!("unsupported content type for web_fetch: {}", ct)
            }
            FetchError::RequestFailed(msg) => format!("web_fetch request failed: {}", msg),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a web page as text or markdown. Successful fetches are automatically saved for later curation."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: WebFetchInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        t_koma_core::load_dotenv();
        let settings = t_koma_core::Settings::load().map_err(|e| e.to_string())?;

        if !settings.tools.web.enabled || !settings.tools.web.fetch.enabled {
            return Err("web_fetch tool is disabled in config".to_string());
        }

        let provider_name = settings.tools.web.fetch.provider.as_str();
        if provider_name != "http" {
            return Err(format!(
                "web_fetch provider '{}' is not supported in this build",
                provider_name
            ));
        }

        let provider = HttpFetchProvider::new(
            std::time::Duration::from_secs(settings.tools.web.fetch.timeout_seconds),
            settings.tools.web.fetch.mode.clone(),
            settings.tools.web.fetch.max_chars,
        )
        .map_err(Self::format_error)?;

        let service = WebFetchService::new(
            Box::new(provider),
            std::time::Duration::from_secs(settings.tools.web.fetch.cache_ttl_minutes * 60),
        );

        let url = input.url;
        let request = WebFetchRequest {
            url: url.clone(),
            mode: input.mode,
            max_chars: input.max_chars,
            raw: input.raw,
        };

        let response = service.fetch(request).await.map_err(Self::format_error)?;

        // Auto-save fetched content to _web-cache reference topic (skip non-2xx)
        if (200..300).contains(&response.status) {
            let filename = url_to_cache_filename(&url, "md");
            context
                .auto_save_web_result(&url, &response.content, &filename)
                .await;
        }

        let serialized = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        let ref_id = context.cache_tool_result("web_fetch", &serialized);
        Ok(format!("[Result #{}] {}", ref_id, serialized))
    }
}

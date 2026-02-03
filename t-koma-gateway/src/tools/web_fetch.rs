use serde::Deserialize;
use serde_json::{json, Value};

use crate::tools::Tool;
use crate::web::fetch::{http::HttpFetchProvider, FetchError, WebFetchRequest, WebFetchService};

#[derive(Debug, Deserialize)]
struct WebFetchInput {
    url: String,
    mode: Option<String>,
    max_chars: Option<usize>,
}

pub struct WebFetchTool;

impl WebFetchTool {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "mode": {"type": "string", "enum": ["text", "markdown"]},
                "max_chars": {"type": "integer", "minimum": 1}
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
        "Fetch the textual content of a web page and return it as text or markdown."
    }

    fn input_schema(&self) -> Value {
        Self::schema()
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "Use web_fetch to retrieve the textual content of a URL.\n\
- Only http/https URLs are supported.\n\
- The result may be truncated to a safe length.\n\
- Do not fetch sensitive or private URLs.",
        )
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
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

        let request = WebFetchRequest {
            url: input.url,
            mode: input.mode,
            max_chars: input.max_chars,
        };

        let response = service.fetch(request).await.map_err(Self::format_error)?;

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }
}

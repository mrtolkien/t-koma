//! OpenRouter API client with OpenAI-compatible format.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::{ChatContentBlock, ChatMessage, ChatRole};
use crate::prompt::render::SystemBlock;
use crate::providers::provider::{
    Provider, ProviderContentBlock, ProviderError, ProviderResponse, ProviderUsage,
};
use crate::tools::Tool;

/// OpenRouter API client
#[derive(Clone)]
pub struct OpenRouterClient {
    http_client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    http_referer: Option<String>,
    app_name: Option<String>,
    routing: Option<Vec<String>>,
    dump_queries: bool,
}

/// Request body for the Chat Completions API
#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<OpenRouterProviderRoutingRequest>,
    max_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
struct OpenRouterProviderRoutingRequest {
    order: Vec<String>,
}

/// OpenAI-compatible message format
#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// OpenAI-compatible tool call
#[derive(Debug, Serialize, Deserialize)]
struct ToolCall {
    id: String,
    r#type: String,
    function: ToolCallFunction,
}

/// Tool call function details
#[derive(Debug, Serialize, Deserialize)]
struct ToolCallFunction {
    name: String,
    arguments: String,
}

/// OpenAI-compatible tool definition
#[derive(Debug, Serialize)]
struct OpenAiToolDefinition {
    r#type: String,
    function: OpenAiFunctionDefinition,
}

/// OpenAI-compatible function definition
#[derive(Debug, Serialize)]
struct OpenAiFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    id: String,
    model: String,
    choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
}

/// Choice in the response
#[derive(Debug, Deserialize)]
struct Choice {
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

/// Usage information
#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    input_tokens_details: Option<TokenDetails>,
    #[allow(dead_code)]
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct TokenDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_read: Option<u32>,
    #[serde(default)]
    cache_creation: Option<u32>,
}

/// OpenRouter API error response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenRouterError {
    error: OpenRouterErrorDetail,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenRouterErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

/// Model information from OpenRouter API
#[derive(Debug, Clone, Deserialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "context_length")]
    pub context_length: Option<u32>,
}

/// Response from the models endpoint
#[derive(Debug, Deserialize)]
pub struct OpenRouterModelsResponse {
    pub data: Vec<OpenRouterModel>,
}

impl OpenRouterClient {
    /// Create a new OpenRouter client
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
        http_referer: Option<String>,
        app_name: Option<String>,
        routing: Option<Vec<String>>,
    ) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http_client,
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            http_referer,
            app_name,
            routing,
            dump_queries: false,
        }
    }

    /// Enable or disable debug query logging
    pub fn with_dump_queries(mut self, enabled: bool) -> Self {
        self.dump_queries = enabled;
        self
    }

    /// Update the model for this client
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Build request headers with optional attribution
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let auth_value = format!("Bearer {}", self.api_key);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).expect("Invalid API key format"),
        );

        // Optional headers for OpenRouter rankings
        if let Some(ref referer) = self.http_referer
            && let Ok(value) = HeaderValue::from_str(referer)
        {
            headers.insert("HTTP-Referer", value);
        }
        if let Some(ref app_name) = self.app_name
            && let Ok(value) = HeaderValue::from_str(app_name)
        {
            headers.insert("X-Title", value);
        }

        headers
    }

    /// Send a simple single-turn message.
    pub async fn send_message(
        &self,
        content: impl AsRef<str>,
    ) -> Result<ProviderResponse, ProviderError> {
        self.send_conversation(None, vec![], vec![], Some(content.as_ref()), None, None)
            .await
    }

    /// Convert system blocks to a single system message
    fn convert_system_blocks(&self, system: Option<Vec<SystemBlock>>) -> Option<OpenAiMessage> {
        let blocks = system?;
        if blocks.is_empty() {
            return None;
        }

        let content = blocks
            .into_iter()
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n\n");

        Some(OpenAiMessage {
            role: "system".to_string(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
        })
    }

    /// Convert API messages to OpenAI format
    fn convert_messages(
        &self,
        history: Vec<ChatMessage>,
        new_message: Option<&str>,
    ) -> Vec<OpenAiMessage> {
        let mut messages: Vec<OpenAiMessage> = Vec::new();

        for msg in history {
            let mut text_parts = Vec::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut tool_messages: Vec<OpenAiMessage> = Vec::new();

            for block in msg.content {
                match block {
                    ChatContentBlock::Text { text, .. } => text_parts.push(text),
                    ChatContentBlock::ToolUse { id, name, input } => {
                        let arguments =
                            serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                        tool_calls.push(ToolCall {
                            id,
                            r#type: "function".to_string(),
                            function: ToolCallFunction { name, arguments },
                        });
                    }
                    ChatContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        tool_messages.push(OpenAiMessage {
                            role: "tool".to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id),
                        });
                    }
                }
            }

            let text = if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            };

            match msg.role {
                ChatRole::Assistant => {
                    if text.is_some() || !tool_calls.is_empty() {
                        messages.push(OpenAiMessage {
                            role: "assistant".to_string(),
                            content: text,
                            tool_calls: if tool_calls.is_empty() {
                                None
                            } else {
                                Some(tool_calls)
                            },
                            tool_call_id: None,
                        });
                    }
                }
                ChatRole::User => {
                    if let Some(text) = text {
                        messages.push(OpenAiMessage {
                            role: "user".to_string(),
                            content: Some(text),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                }
            }

            messages.extend(tool_messages);
        }

        // Add new message if provided
        if let Some(content) = new_message {
            messages.push(OpenAiMessage {
                role: "user".to_string(),
                content: Some(content.to_string()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        messages
    }

    /// Convert tools to OpenAI format
    fn convert_tools(&self, tools: &[&dyn Tool]) -> Vec<OpenAiToolDefinition> {
        tools
            .iter()
            .map(|tool| OpenAiToolDefinition {
                r#type: "function".to_string(),
                function: OpenAiFunctionDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.input_schema(),
                },
            })
            .collect()
    }

    /// Convert OpenAI response to provider response
    fn convert_response(
        &self,
        response: ChatCompletionsResponse,
        raw_json: &str,
    ) -> ProviderResponse {
        let stop_reason = response
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone());
        let choice = response.choices.into_iter().next();

        let content = match choice {
            Some(choice) => {
                let mut blocks = Vec::new();

                // Add text content if present
                if let Some(text) = choice.message.content
                    && !text.is_empty()
                {
                    blocks.push(ProviderContentBlock::Text { text });
                }

                // Add tool calls
                if let Some(tool_calls) = choice.message.tool_calls {
                    for tool_call in tool_calls {
                        if let Ok(input) = serde_json::from_str(&tool_call.function.arguments) {
                            blocks.push(ProviderContentBlock::ToolUse {
                                id: tool_call.id,
                                name: tool_call.function.name,
                                input,
                            });
                        }
                    }
                }

                blocks
            }
            None => vec![],
        };

        ProviderResponse {
            id: response.id,
            model: response.model,
            content,
            usage: response.usage.map(|u| ProviderUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                cache_read_tokens: u.cache_read_tokens(),
                cache_creation_tokens: u.cache_creation_tokens(),
            }),
            stop_reason,
            raw_json: Some(raw_json.to_string()),
        }
    }

    fn provider_routing_request(&self) -> Option<OpenRouterProviderRoutingRequest> {
        let routing = self.routing.as_ref()?;
        let order: Vec<String> = routing
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        if order.is_empty() {
            return None;
        }
        Some(OpenRouterProviderRoutingRequest { order })
    }

    /// Fetch available models from OpenRouter
    pub async fn fetch_models(&self) -> Result<Vec<OpenRouterModel>, ProviderError> {
        let url = format!("{}/models", self.base_url);

        let response = self
            .http_client
            .get(&url)
            .headers(self.build_headers())
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let models_response: OpenRouterModelsResponse = response.json().await?;
        Ok(models_response.data)
    }
}

#[async_trait::async_trait]
impl Provider for OpenRouterClient {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn send_conversation(
        &self,
        system: Option<Vec<SystemBlock>>,
        history: Vec<ChatMessage>,
        tools: Vec<&dyn Tool>,
        new_message: Option<&str>,
        _message_limit: Option<usize>,
        _tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        // Build messages
        let mut messages = Vec::new();

        // Add system message if present
        if let Some(system_msg) = self.convert_system_blocks(system) {
            messages.push(system_msg);
        }

        // Add history and new message
        messages.extend(self.convert_messages(history, new_message));

        // Build tools
        let tool_definitions = if tools.is_empty() {
            None
        } else {
            Some(self.convert_tools(&tools))
        };

        let tool_choice = if tools.is_empty() {
            None
        } else {
            Some(serde_json::json!("auto"))
        };

        let request_body = ChatCompletionsRequest {
            model: self.model.clone(),
            messages,
            tools: tool_definitions,
            tool_choice,
            provider: self.provider_routing_request(),
            max_tokens: 4096,
        };

        let dump = if self.dump_queries
            && let Ok(val) = serde_json::to_value(&request_body)
        {
            crate::providers::query_dump::QueryDump::request("openrouter", &self.model, &val).await
        } else {
            None
        };

        let response = self
            .http_client
            .post(&url)
            .headers(self.build_headers())
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let response_text = response.text().await?;

        if let Some(dump) = &dump
            && let Ok(val) = serde_json::from_str::<Value>(&response_text)
        {
            dump.response(&val).await;
        }

        let completions_response: ChatCompletionsResponse = serde_json::from_str(&response_text)
            .map_err(|e| {
                let preview = if response_text.len() > 500 {
                    &response_text[..response_text.floor_char_boundary(500)]
                } else {
                    &response_text
                };
                ProviderError::InvalidFormat(format!(
                    "Failed to parse OpenRouter response: {e}\nBody preview: {preview}"
                ))
            })?;
        Ok(self.convert_response(completions_response, &response_text))
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

impl Usage {
    fn cache_read_tokens(&self) -> Option<u32> {
        self.cache_read_input_tokens
            .or(self.cached_tokens)
            .or_else(|| self.prompt_tokens_details.as_ref()?.cache_read_value())
            .or_else(|| self.input_tokens_details.as_ref()?.cache_read_value())
    }

    fn cache_creation_tokens(&self) -> Option<u32> {
        self.cache_creation_input_tokens
            .or_else(|| self.prompt_tokens_details.as_ref()?.cache_creation_value())
            .or_else(|| self.input_tokens_details.as_ref()?.cache_creation_value())
    }
}

impl TokenDetails {
    fn cache_read_value(&self) -> Option<u32> {
        self.cache_read_input_tokens
            .or(self.cached_tokens)
            .or(self.cache_read)
    }

    fn cache_creation_value(&self) -> Option<u32> {
        self.cache_creation_input_tokens.or(self.cache_creation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_client_creation() {
        let client =
            OpenRouterClient::new("test-key", "openrouter/model-a", None, None, None, None);
        assert_eq!(client.model(), "openrouter/model-a");
    }

    #[test]
    fn test_convert_tools() {
        use crate::tools::{Tool, ToolContext};

        struct TestTool;
        #[async_trait::async_trait]
        impl Tool for TestTool {
            fn name(&self) -> &str {
                "test_tool"
            }
            fn description(&self) -> &str {
                "A test tool"
            }
            fn input_schema(&self) -> Value {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "param": {"type": "string"}
                    }
                })
            }
            async fn execute(
                &self,
                _args: Value,
                _context: &mut ToolContext,
            ) -> Result<String, String> {
                Ok("ok".to_string())
            }
        }

        let client = OpenRouterClient::new("test-key", "test-model", None, None, None, None);
        let tools: Vec<&dyn Tool> = vec![&TestTool];
        let openai_tools = client.convert_tools(&tools);

        assert_eq!(openai_tools.len(), 1);
        assert_eq!(openai_tools[0].r#type, "function");
        assert_eq!(openai_tools[0].function.name, "test_tool");
    }

    #[test]
    fn test_provider_routing_request_trims_and_serializes() {
        let client = OpenRouterClient::new(
            "test-key",
            "test-model",
            None,
            None,
            None,
            Some(vec![" anthropic ".to_string(), "".to_string()]),
        );

        let request = client.provider_routing_request().expect("routing present");
        assert_eq!(request.order, vec!["anthropic".to_string()]);
    }

    #[test]
    fn test_chat_request_includes_provider_when_configured() {
        let client = OpenRouterClient::new(
            "test-key",
            "test-model",
            None,
            None,
            None,
            Some(vec!["anthropic".to_string()]),
        );

        let body = ChatCompletionsRequest {
            model: "test-model".to_string(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            provider: client.provider_routing_request(),
            max_tokens: 10,
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["provider"]["order"], serde_json::json!(["anthropic"]));
    }

    #[test]
    fn test_chat_request_omits_provider_when_not_configured() {
        let body = ChatCompletionsRequest {
            model: "test-model".to_string(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            provider: None,
            max_tokens: 10,
        };
        let json = serde_json::to_value(body).unwrap();
        assert!(json.get("provider").is_none());
    }

    #[test]
    fn test_cache_usage_parsing() {
        let usage: Usage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {
                "cached_tokens": 60,
                "cache_creation_input_tokens": 15
            }
        }))
        .unwrap();

        assert_eq!(usage.cache_read_tokens(), Some(60));
        assert_eq!(usage.cache_creation_tokens(), Some(15));
    }

    #[test]
    fn test_cache_usage_absent() {
        let usage: Usage = serde_json::from_value(serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        }))
        .unwrap();

        assert_eq!(usage.cache_read_tokens(), None);
        assert_eq!(usage.cache_creation_tokens(), None);
    }
}

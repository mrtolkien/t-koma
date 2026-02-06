//! Anthropic API client with session support and prompt caching.

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::ChatMessage;
use crate::prompt::render::SystemBlock;
use crate::providers::anthropic::history::AnthropicMessage;
use crate::providers::provider::{
    Provider, ProviderContentBlock, ProviderError, ProviderResponse, ProviderUsage,
};
use crate::tools::Tool;

/// Anthropic API client
#[derive(Clone)]
pub struct AnthropicClient {
    http_client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    dump_queries: bool,
}

/// Request body for the Messages API with prompt caching support
#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    /// System prompt as array of blocks (supports cache_control)
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>,
    /// Conversation history
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    // TODO: Add tool_choice when we need to force specific tool usage.
    // For now, the model decides based on tool definitions.
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

/// Response from the Messages API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub msg_type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<Usage>,
}

/// Content block in the response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

/// Usage information including prompt caching metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens read from cache (cache hit)
    #[serde(rename = "cache_read_input_tokens")]
    #[serde(default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache (cache creation)
    #[serde(rename = "cache_creation_input_tokens")]
    #[serde(default)]
    pub cache_creation_tokens: u32,
}

/// Errors that can occur when calling the Anthropic API
#[derive(Debug, thiserror::Error)]
pub enum AnthropicError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("API error: {message}")]
    ApiError { message: String },
    #[error("No content in response")]
    NoContent,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<AnthropicError> for ProviderError {
    fn from(err: AnthropicError) -> Self {
        match err {
            AnthropicError::HttpError(e) => ProviderError::HttpError(e),
            AnthropicError::ApiError { message } => ProviderError::ApiError { message },
            AnthropicError::NoContent => ProviderError::NoContent,
            AnthropicError::Serialization(e) => ProviderError::Serialization(e),
        }
    }
}

impl AnthropicClient {
    /// Create a new Anthropic client
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
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
            base_url: "https://api.anthropic.com/v1".to_string(),
            dump_queries: false,
        }
    }

    /// Enable or disable debug query logging
    pub fn with_dump_queries(mut self, enabled: bool) -> Self {
        self.dump_queries = enabled;
        self
    }

    /// Send a simple single-turn message.
    pub async fn send_message(
        &self,
        content: impl AsRef<str>,
    ) -> Result<MessagesResponse, AnthropicError> {
        self.send_conversation(None, vec![], vec![], Some(content.as_ref()), None, None)
            .await
    }

    /// Send a single-turn message with tool definitions.
    pub async fn send_message_with_tools(
        &self,
        content: impl AsRef<str>,
        tools: Vec<&dyn Tool>,
    ) -> Result<MessagesResponse, AnthropicError> {
        self.send_conversation(None, vec![], tools, Some(content.as_ref()), None, None)
            .await
    }

    /// Send a conversation with full history and prompt caching
    ///
    /// # Arguments
    /// * `system` - Optional system prompt blocks with cache_control
    /// * `history` - Previous conversation messages
    /// * `tools` - Available tools
    /// * `new_message` - Optional new user message to add
    /// * `message_limit` - Optional limit on history messages to include
    /// * `_tool_choice` - Placeholder for future forced tool selection
    pub async fn send_conversation(
        &self,
        system: Option<Vec<SystemBlock>>,
        history: Vec<ChatMessage>,
        tools: Vec<&dyn Tool>,
        new_message: Option<&str>,
        message_limit: Option<usize>,
        _tool_choice: Option<String>,
    ) -> Result<MessagesResponse, AnthropicError> {
        let url = format!("{}/messages", self.base_url);

        // Build messages: neutral history -> Anthropic API payload.
        let messages = crate::providers::anthropic::history::to_anthropic_messages(
            history,
            new_message,
            message_limit,
        );

        // Build tool definitions
        let tool_definitions = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| ToolDefinition {
                        name: t.name().to_string(),
                        description: t.description().to_string(),
                        input_schema: t.input_schema(),
                    })
                    .collect(),
            )
        };

        let request_body = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system,
            messages,
            tools: tool_definitions,
        };

        let dump = if self.dump_queries
            && let Ok(val) = serde_json::to_value(&request_body)
        {
            crate::providers::query_dump::QueryDump::request("anthropic", &self.model, &val).await
        } else {
            None
        };

        let response = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AnthropicError::ApiError {
                message: format!("HTTP {}: {}", status, error_text),
            });
        }

        let messages_response: MessagesResponse = response.json().await?;

        if let Some(dump) = dump
            && let Ok(val) = serde_json::to_value(&messages_response)
        {
            dump.response(&val).await;
        }

        Ok(messages_response)
    }

    /// Extract text content from a response
    pub fn extract_text(response: &MessagesResponse) -> Option<String> {
        response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .next()
    }

    /// Extract all text content from a response
    pub fn extract_all_text(response: &MessagesResponse) -> String {
        response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract tool uses from a response
    pub fn extract_tool_uses(response: &MessagesResponse) -> Vec<(String, String, Value)> {
        response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Check if the response has tool uses
    pub fn has_tool_uses(response: &MessagesResponse) -> bool {
        response
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    }

    /// Convert MessagesResponse to ProviderResponse
    fn to_provider_response(&self, response: MessagesResponse) -> ProviderResponse {
        let content = response
            .content
            .into_iter()
            .map(|block| match block {
                ContentBlock::Text { text } => ProviderContentBlock::Text { text },
                ContentBlock::ToolUse { id, name, input } => {
                    ProviderContentBlock::ToolUse { id, name, input }
                }
            })
            .collect();

        ProviderResponse {
            id: response.id,
            model: response.model,
            content,
            usage: response.usage.map(|u| ProviderUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: Some(u.cache_read_tokens),
                cache_creation_tokens: Some(u.cache_creation_tokens),
            }),
            stop_reason: response.stop_reason,
        }
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicClient {
    fn name(&self) -> &str {
        "anthropic"
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
        message_limit: Option<usize>,
        _tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError> {
        let response = self
            .send_conversation(
                system,
                history,
                tools,
                new_message,
                message_limit,
                _tool_choice,
            )
            .await?;
        Ok(self.to_provider_response(response))
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_client_creation() {
        let client = AnthropicClient::new("test-key", "anthropic-model-a");
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.model, "anthropic-model-a");
        assert_eq!(client.base_url, "https://api.anthropic.com/v1");
    }

    #[test]
    fn test_extract_text() {
        let response = MessagesResponse {
            id: "msg_001".to_string(),
            msg_type: "message".to_string(),
            role: "assistant".to_string(),
            model: "anthropic-model-a".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello, world!".to_string(),
            }],
            stop_reason: None,
            stop_sequence: None,
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            }),
        };

        assert_eq!(
            AnthropicClient::extract_text(&response),
            Some("Hello, world!".to_string())
        );
    }

    #[test]
    fn test_extract_tool_uses() {
        let response = MessagesResponse {
            id: "msg_001".to_string(),
            msg_type: "message".to_string(),
            role: "assistant".to_string(),
            model: "anthropic-model-a".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I'll check that.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tool_123".to_string(),
                    name: "get_weather".to_string(),
                    input: serde_json::json!({"location": "SF"}),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            stop_sequence: None,
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 1000,
                cache_creation_tokens: 0,
            }),
        };

        let tools = AnthropicClient::extract_tool_uses(&response);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].0, "tool_123");
        assert_eq!(tools[0].1, "get_weather");
        assert!(AnthropicClient::has_tool_uses(&response));
    }

    #[test]
    fn test_usage_with_cache() {
        let usage = Usage {
            input_tokens: 50,
            output_tokens: 100,
            cache_read_tokens: 10000,
            cache_creation_tokens: 0,
        };

        assert_eq!(usage.cache_read_tokens, 10000);
    }
}

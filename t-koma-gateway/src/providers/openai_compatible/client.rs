//! OpenAI-compatible API client.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::{ChatContentBlock, ChatMessage, ChatRole};
use crate::prompt::render::SystemBlock;
use crate::providers::provider::{
    Provider, ProviderContentBlock, ProviderError, ProviderResponse, ProviderUsage,
};
use crate::tools::Tool;

/// OpenAI-compatible API client.
#[derive(Clone)]
pub struct OpenAiCompatibleClient {
    http_client: reqwest::Client,
    api_key: Option<String>,
    model: String,
    base_url: String,
    provider_name: String,
    dump_queries: bool,
    extra_headers: HeaderMap,
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
    max_tokens: u32,
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
    #[allow(dead_code)]
    total_tokens: u32,
}

impl OpenAiCompatibleClient {
    /// Create a new OpenAI-compatible client.
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        provider_name: impl Into<String>,
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
            api_key,
            model: model.into(),
            base_url: base_url.into(),
            provider_name: provider_name.into(),
            dump_queries: false,
            extra_headers: HeaderMap::new(),
        }
    }

    /// Enable or disable debug query logging
    pub fn with_dump_queries(mut self, enabled: bool) -> Self {
        self.dump_queries = enabled;
        self
    }

    /// Set extra headers sent with every request.
    pub fn with_extra_headers(mut self, headers: HeaderMap) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Build request headers with optional auth and extra headers.
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(api_key) = &self.api_key {
            let auth_value = format!("Bearer {}", api_key);
            if let Ok(header_value) = HeaderValue::from_str(&auth_value) {
                headers.insert(AUTHORIZATION, header_value);
            }
        }

        headers.extend(
            self.extra_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        headers
    }

    fn normalized_base_url(&self) -> String {
        self.base_url.trim_end_matches('/').to_string()
    }

    fn chat_completions_url(&self) -> String {
        let base = self.normalized_base_url();
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
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

                if let Some(text) = choice.message.content
                    && !text.is_empty()
                {
                    blocks.push(ProviderContentBlock::Text { text });
                }

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
                cache_read_tokens: None,
                cache_creation_tokens: None,
            }),
            stop_reason,
            raw_json: if self.dump_queries {
                Some(raw_json.to_string())
            } else {
                None
            },
        }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatibleClient {
    fn name(&self) -> &str {
        &self.provider_name
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
        let url = self.chat_completions_url();

        let mut messages = Vec::new();

        if let Some(system_msg) = self.convert_system_blocks(system) {
            messages.push(system_msg);
        }

        messages.extend(self.convert_messages(history, new_message));

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
            max_tokens: 4096,
        };

        let dump = if self.dump_queries
            && let Ok(val) = serde_json::to_value(&request_body)
        {
            crate::providers::query_dump::QueryDump::request(&self.provider_name, &self.model, &val)
                .await
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
                    "Failed to parse OpenAI-compatible response: {e}\nBody preview: {preview}"
                ))
            })?;
        Ok(self.convert_response(completions_response, &response_text))
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_compatible_client_creation() {
        let client = OpenAiCompatibleClient::new(
            "http://127.0.0.1:8080",
            None,
            "llama3.1",
            "openai_compatible",
        );
        assert_eq!(client.model(), "llama3.1");
    }

    #[test]
    fn test_chat_completions_url_without_v1_suffix() {
        let client = OpenAiCompatibleClient::new(
            "http://127.0.0.1:8080/",
            None,
            "llama3.1",
            "openai_compatible",
        );
        assert_eq!(
            client.chat_completions_url(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }

    #[test]
    fn test_chat_completions_url_with_v1_suffix() {
        let client = OpenAiCompatibleClient::new(
            "http://127.0.0.1:8080/v1",
            None,
            "llama3.1",
            "openai_compatible",
        );
        assert_eq!(
            client.chat_completions_url(),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }
}

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
        http_referer: Option<String>,
        app_name: Option<String>,
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
            base_url: "https://openrouter.ai/api/v1".to_string(),
            http_referer,
            app_name,
        }
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
    fn convert_response(&self, response: ChatCompletionsResponse) -> ProviderResponse {
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
                cache_read_tokens: None,
                cache_creation_tokens: None,
            }),
            stop_reason,
        }
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
                message: format!("HTTP {}: {}", status, error_text),
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
            max_tokens: 4096,
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
                message: format!("HTTP {}: {}", status, error_text),
            });
        }

        let completions_response: ChatCompletionsResponse = response.json().await?;
        Ok(self.convert_response(completions_response))
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_client_creation() {
        let client = OpenRouterClient::new("test-key", "openrouter/model-a", None, None);
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
            fn prompt(&self) -> Option<&'static str> {
                None
            }
            async fn execute(
                &self,
                _args: Value,
                _context: &mut ToolContext,
            ) -> Result<String, String> {
                Ok("ok".to_string())
            }
        }

        let client = OpenRouterClient::new("test-key", "test-model", None, None);
        let tools: Vec<&dyn Tool> = vec![&TestTool];
        let openai_tools = client.convert_tools(&tools);

        assert_eq!(openai_tools.len(), 1);
        assert_eq!(openai_tools[0].r#type, "function");
        assert_eq!(openai_tools[0].function.name, "test_tool");
    }
}

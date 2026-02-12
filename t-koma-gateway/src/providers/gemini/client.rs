//! Google Gemini API client.

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::history::ChatMessage;
use crate::prompt::render::SystemBlock;
use crate::providers::gemini::history::{GeminiContent, to_gemini_contents};
use crate::providers::provider::{
    Provider, ProviderContentBlock, ProviderError, ProviderResponse, ProviderUsage,
};
use crate::tools::Tool;

/// Gemini API client
#[derive(Clone)]
pub struct GeminiClient {
    http_client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    dump_queries: bool,
}

/// Request body for the Gemini generateContent API
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDeclaration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

/// System instruction for Gemini
#[derive(Debug, Serialize)]
struct SystemInstruction {
    parts: Vec<SystemPart>,
}

/// System instruction part
#[derive(Debug, Serialize)]
struct SystemPart {
    text: String,
}

/// Tool declaration for Gemini
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDeclaration {
    function_declarations: Vec<FunctionDeclaration>,
}

/// Function declaration
#[derive(Debug, Serialize)]
struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

/// Generation configuration
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

/// Response from the generateContent API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

/// Candidate response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub content: CandidateContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Candidate content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateContent {
    pub parts: Vec<CandidatePart>,
    pub role: String,
}

/// Candidate part
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum CandidatePart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallData,
    },
}

/// Function call data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallData {
    pub name: String,
    pub args: Value,
}

/// Usage metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    pub prompt_token_count: u32,
    pub candidates_token_count: u32,
    #[serde(default)]
    pub total_token_count: u32,
}

impl GeminiClient {
    /// Create a new Gemini client
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
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
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            dump_queries: false,
        }
    }

    /// Enable or disable debug query logging
    pub fn with_dump_queries(mut self, enabled: bool) -> Self {
        self.dump_queries = enabled;
        self
    }

    /// Send a conversation with full history
    ///
    /// # Arguments
    /// * `system` - Optional system instruction blocks
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
    ) -> Result<(GenerateContentResponse, String), ProviderError> {
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        // Convert history to Gemini format
        let contents = to_gemini_contents(history, new_message, message_limit);

        // Build system instruction
        let system_instruction = system.map(|blocks| SystemInstruction {
            parts: blocks
                .into_iter()
                .map(|b| SystemPart { text: b.text })
                .collect(),
        });

        // Build tool declarations
        let tool_declarations = if tools.is_empty() {
            None
        } else {
            Some(vec![ToolDeclaration {
                function_declarations: tools
                    .iter()
                    .map(|tool| FunctionDeclaration {
                        name: tool.name().to_string(),
                        description: tool.description().to_string(),
                        parameters: sanitize_schema(tool.input_schema()),
                    })
                    .collect(),
            }])
        };

        let request_body = GenerateContentRequest {
            contents,
            system_instruction,
            tools: tool_declarations,
            generation_config: Some(GenerationConfig {
                max_output_tokens: Some(8192),
            }),
        };

        let request_value: Value = serde_json::to_value(&request_body)?;

        let dump_handle = if self.dump_queries {
            super::super::query_dump::QueryDump::request("gemini", &self.model, &request_value)
                .await
        } else {
            None
        };

        let response = self
            .http_client
            .post(&url)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        if let Some(handle) = dump_handle
            && let Ok(response_value) = serde_json::from_str::<Value>(&response_text)
        {
            handle.response(&response_value).await;
        }

        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: response_text,
            });
        }

        let parsed: GenerateContentResponse = serde_json::from_str(&response_text)?;

        Ok((parsed, response_text))
    }
}

#[async_trait::async_trait]
impl Provider for GeminiClient {
    fn name(&self) -> &str {
        "gemini"
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
        tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError> {
        let (response, raw_json) = self
            .send_conversation(
                system,
                history,
                tools,
                new_message,
                message_limit,
                tool_choice,
            )
            .await?;

        // Extract first candidate
        let candidate = response
            .candidates
            .first()
            .ok_or(ProviderError::NoContent)?;

        // Convert parts to provider content blocks
        let mut content = Vec::new();
        for part in &candidate.content.parts {
            match part {
                CandidatePart::Text { text } => {
                    content.push(ProviderContentBlock::Text { text: text.clone() });
                }
                CandidatePart::FunctionCall { function_call } => {
                    // Generate a unique ID for tool use
                    let id = format!("call_{}", uuid::Uuid::new_v4());
                    content.push(ProviderContentBlock::ToolUse {
                        id,
                        name: function_call.name.clone(),
                        input: function_call.args.clone(),
                    });
                }
            }
        }

        // Convert usage metadata
        let usage = response.usage_metadata.map(|u| ProviderUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
            cache_read_tokens: None,
            cache_creation_tokens: None,
        });

        Ok(ProviderResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: self.model.clone(),
            content,
            usage,
            stop_reason: candidate.finish_reason.clone(),
            raw_json: Some(raw_json),
        })
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

/// Recursively strip JSON Schema fields that Gemini's FunctionDeclaration
/// parameters do not support. Currently removes `additionalProperties` at
/// every nesting level.
fn sanitize_schema(mut schema: Value) -> Value {
    strip_unsupported_fields(&mut schema);
    schema
}

fn strip_unsupported_fields(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };

    obj.remove("additionalProperties");

    for child in obj.values_mut() {
        strip_unsupported_fields(child);
    }
}

// Simple UUID module
mod uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug, Clone, Copy)]
    pub struct Uuid(u64);

    impl Uuid {
        pub fn new_v4() -> Self {
            Self(COUNTER.fetch_add(1, Ordering::SeqCst))
        }
    }

    impl std::fmt::Display for Uuid {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:016x}", self.0)
        }
    }
}

# Anthropic Claude API Guide for AI Agents

A comprehensive guide for integrating with the Anthropic Claude API, including
prompt caching, tool use, and conversation management.

## Table of Contents

- [API Basics](#api-basics)
- [Prompt Caching](#prompt-caching)
- [Tool Use](#tool-use)
- [Conversation Management](#conversation-management)
- [Content Blocks](#content-blocks)
- [Error Handling](#error-handling)
- [Best Practices](#best-practices)

---

## API Basics

### Authentication

```rust
use reqwest::Client;

// API key from environment
let api_key = std::env::var("ANTHROPIC_API_KEY")?;

// Headers required for all requests
let headers = vec![
    ("x-api-key", api_key),
    ("anthropic-version", "2023-06-01"),
    ("content-type", "application/json"),
];
```

### Basic Request Structure

```rust
use serde::{Serialize, Deserialize};
use serde_json::Value;

/// Messages API request body
#[derive(Serialize)]
struct MessagesRequest {
    model: String,                    // e.g., "claude-sonnet-4-5-20250929"
    max_tokens: u32,                  // Maximum tokens in response
    messages: Vec<Message>,           // Conversation history
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>, // System prompt (optional)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolDefinition>,       // Available tools (optional)
}

#[derive(Serialize)]
struct Message {
    role: String,                     // "user" or "assistant"
    content: Vec<ContentBlock>,       // Content blocks (text, tool_use, tool_result)
}
```

### Response Structure

```rust
#[derive(Deserialize)]
struct MessagesResponse {
    id: String,
    model: String,
    content: Vec<ContentBlock>,       // Response content blocks
    usage: Usage,
    stop_reason: Option<String>,      // "end_turn", "max_tokens", "stop_sequence"
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u32,
    output_tokens: u32,
    // Cache-specific fields (optional, only if caching used)
    cache_creation_input_tokens: Option<u32>,
    cache_read_input_tokens: Option<u32>,
}
```

---

## Prompt Caching

### What is Prompt Caching?

Prompt caching allows you to cache parts of your prompt (typically the system
prompt and conversation history) to reduce token costs and latency on subsequent
requests. The cache has a 5-minute TTL.

### Cache Control Blocks

```rust
use serde::{Serialize, Deserialize};

/// Cache control for system blocks and content
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CacheControl {
    /// Use this block to create/extend a cache
    Ephemeral,
}

/// System prompt block with optional caching
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SystemBlock {
    pub r#type: String,  // "text"
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// Content block with optional caching (for conversation history)
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ApiContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}
```

### Caching Strategy

**Rule**: Only the LAST block in a sequence can have `cache_control: ephemeral`.

```rust
/// Build system prompt with caching on the LAST block only
pub fn build_anthropic_system_prompt(prompt: &SystemPrompt) -> Vec<SystemBlock> {
    let mut blocks = vec![];
    
    // First blocks: NO cache control
    for section in &prompt.sections[..prompt.sections.len().saturating_sub(1)] {
        blocks.push(SystemBlock {
            r#type: "text".to_string(),
            text: section.clone(),
            cache_control: None,
        });
    }
    
    // Last block: WITH cache control
    if let Some(last) = prompt.sections.last() {
        blocks.push(SystemBlock {
            r#type: "text".to_string(),
            text: last.clone(),
            cache_control: Some(CacheControl::Ephemeral),
        });
    }
    
    blocks
}
```

### Cache Usage Tracking

```rust
// Check cache performance in response
if let Some(cache_creation) = response.usage.cache_creation_input_tokens {
    println!("Cache created with {} tokens", cache_creation);
}

if let Some(cache_read) = response.usage.cache_read_input_tokens {
    println!("Cache hit: {} tokens read from cache", cache_read);
}
```

**Important**: Cache creation costs 1.25x the base input token price, but cache
reads cost only 0.1x. For repeated similar prompts, this is a huge cost saving.

---

## Tool Use

### Tool Definition

```rust
use serde::{Serialize, Deserialize};
use serde_json::Value;
use async_trait::async_trait;

/// Tool definition sent to Claude
#[derive(Serialize, Clone, Debug)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,  // JSON Schema
}

/// Trait for implementing tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (must match definition)
    fn name(&self) -> &str;
    
    /// Tool description
    fn description(&self) -> &str;
    
    /// JSON Schema for tool parameters
    fn parameters(&self) -> Value;
    
    /// Execute the tool
    async fn execute(&self, args: Value) -> Result<String, String>;
    
    /// Convert to ToolDefinition for API
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.parameters(),
        }
    }
}
```

### Example Tool Implementation

```rust
use async_trait::async_trait;

/// Shell command execution tool
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "run_shell_command"
    }
    
    fn description(&self) -> &str {
        "Execute a shell command on the host system. \
         Returns stdout on success, stderr on failure."
    }
    
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }
    
    async fn execute(&self, args: Value) -> Result<String, String> {
        let command = args["command"]
            .as_str()
            .ok_or("Missing 'command' parameter")?;
        
        // Execute command...
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .map_err(|e| e.to_string())?;
        
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}
```

### Tool Use Loop Pattern

Claude can request multiple tool calls in sequence. You must handle this loop:

```rust
/// Tool use loop with max iterations to prevent infinite loops
pub async fn send_conversation_with_tools(
    &self,
    session_id: &str,
    system_blocks: Vec<SystemBlock>,
    api_messages: Vec<ApiMessage>,
    tools: Vec<&dyn Tool>,
    new_message: Option<&str>,
    model: &str,
) -> Result<String, Box<dyn Error>> {
    // Initial request
    let mut response = self.anthropic.send_conversation(
        Some(system_blocks.clone()),
        api_messages.clone(),
        tools.clone(),
        new_message,
        None,
        None,
    ).await?;
    
    // Tool use loop (max 5 iterations)
    for iteration in 0..5 {
        let has_tool_use = response.content.iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
        
        if !has_tool_use {
            break;  // No tool use, we're done
        }
        
        println!("Tool use detected (iteration {})", iteration + 1);
        
        // 1. Save assistant message with tool_use blocks to DB
        let assistant_content: Vec<DbContentBlock> = response.content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => {
                    DbContentBlock::Text { text: text.clone() }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    DbContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
            })
            .collect();
        
        save_message_to_db(session_id, MessageRole::Assistant, assistant_content).await?;
        
        // 2. Execute tools and collect results
        let mut tool_results = Vec::new();
        for block in &response.content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                println!("Executing tool: {} (id: {})", name, id);
                
                // Find and execute the tool
                let result = match name.as_str() {
                    "run_shell_command" => {
                        let tool = ShellTool;
                        tool.execute(input.clone()).await
                    }
                    _ => Err(format!("Unknown tool: {}", name)),
                };
                
                tool_results.push(DbContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: match result {
                        Ok(output) => output,
                        Err(e) => format!("Error: {}", e),
                    },
                    is_error: result.is_err().then_some(true),
                });
            }
        }
        
        // 3. Save tool results to DB
        save_message_to_db(session_id, MessageRole::User, tool_results.clone()).await?;
        
        // 4. Rebuild conversation history with tool results
        let history = fetch_conversation_history(session_id).await?;
        let new_api_messages = build_api_messages(&history, Some(50));
        
        // 5. Send tool results back to Claude
        response = self.anthropic.send_conversation(
            Some(system_blocks.clone()),
            new_api_messages,
            tools.clone(),
            None,  // No new user message, just continuing with tool results
            None,
            None,
        ).await?;
    }
    
    // Extract and return final text response
    let text = extract_all_text(&response);
    
    // Save final assistant response
    save_message_to_db(
        session_id,
        MessageRole::Assistant,
        vec![DbContentBlock::Text { text: text.clone() }],
    ).await?;
    
    Ok(text)
}
```

---

## Conversation Management

### Building API Messages from History

```rust
/// Convert database messages to Anthropic API format
pub fn build_api_messages(
    messages: &[Message],
    limit: Option<usize>,
) -> Vec<ApiMessage> {
    let messages = match limit {
        Some(n) => &messages[messages.len().saturating_sub(n)..],
        None => messages,
    };
    
    let mut api_messages = Vec::new();
    
    for msg in messages {
        let content: Vec<ApiContentBlock> = msg.content
            .iter()
            .map(|block| match block {
                DbContentBlock::Text { text } => {
                    ApiContentBlock::Text {
                        text: text.clone(),
                        cache_control: None,  // Cache only last block
                    }
                }
                DbContentBlock::ToolUse { id, name, input } => {
                    ApiContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
                DbContentBlock::ToolResult { tool_use_id, content, is_error } => {
                    ApiContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    }
                }
            })
            .collect();
        
        api_messages.push(ApiMessage {
            role: msg.role.to_string(),
            content,
        });
    }
    
    // Add cache control to last content block of last message
    if let Some(last_msg) = api_messages.last_mut() {
        if let Some(last_block) = last_msg.content.last_mut() {
            if let ApiContentBlock::Text { cache_control, .. } = last_block {
                *cache_control = Some(CacheControl::Ephemeral);
            }
        }
    }
    
    api_messages
}
```

### Session-Based Conversation Storage

```rust
/// Database schema for sessions and messages
-- Sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,           -- UUID
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- Messages table with ContentBlock support
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,            -- "user", "assistant", "system"
    content TEXT NOT NULL,         -- JSON array of ContentBlock
    model TEXT,                    -- Model used (for assistant messages)
    created_at DATETIME NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- ContentBlock enum for database
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: Option<bool> },
}
```

---

## Content Blocks

### Content Block Types

The Claude API uses content blocks for all message content:

| Block Type | Direction | Description |
|-----------|-----------|-------------|
| `text` | Both | Plain text content |
| `tool_use` | Response | Claude requesting tool execution |
| `tool_result` | Request | Result of tool execution |
| `image` | Request | Base64-encoded image (not covered here) |

### Text Extraction

```rust
/// Extract all text from a response
pub fn extract_all_text(response: &MessagesResponse) -> String {
    response.content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if response contains tool_use blocks
pub fn has_tool_use(response: &MessagesResponse) -> bool {
    response.content.iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
}

/// Extract tool_use blocks from response
pub fn extract_tool_uses(response: &MessagesResponse) -> Vec<&ToolUseBlock> {
    response.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => {
                Some(ToolUseBlock { id, name, input })
            }
            _ => None,
        })
        .collect()
}
```

---

## Error Handling

### API Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AnthropicError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    
    #[error("API error: {message} (type: {error_type})")]
    ApiError {
        error_type: String,
        message: String,
    },
    
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    
    #[error("Tool execution failed: {0}")]
    ToolExecutionError(String),
    
    #[error("Max tool iterations reached")]
    MaxIterationsReached,
}

/// Parse API error from response
fn parse_api_error(response_text: &str) -> AnthropicError {
    #[derive(Deserialize)]
    struct ApiErrorResponse {
        error: ApiErrorDetail,
    }
    
    #[derive(Deserialize)]
    struct ApiErrorDetail {
        r#type: String,
        message: String,
    }
    
    match serde_json::from_str::<ApiErrorResponse>(response_text) {
        Ok(err) => AnthropicError::ApiError {
            error_type: err.error.r#type,
            message: err.error.message,
        },
        Err(_) => AnthropicError::InvalidResponse(response_text.to_string()),
    }
}
```

### Common Error Scenarios

```rust
// Rate limiting (429)
if response.status() == 429 {
    // Implement exponential backoff
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    
    tokio::time::sleep(Duration::from_secs(retry_after)).await;
    // Retry request...
}

// Context length exceeded (400)
if response.status() == 400 {
    let error = parse_api_error(&response.text().await?);
    if error.to_string().contains("context_length_exceeded") {
        // Truncate history and retry
    }
}
```

---

## Best Practices

### 1. Always Limit Conversation History

```rust
// Limit history to prevent context overflow
const MAX_HISTORY_MESSAGES: usize = 50;

let api_messages = build_api_messages(&history, Some(MAX_HISTORY_MESSAGES));
```

### 2. Use Prompt Caching for System Prompts

System prompts are typically large and static. Always cache them:

```rust
let system_blocks = vec![
    SystemBlock {
        r#type: "text".to_string(),
        text: system_prompt,
        cache_control: Some(CacheControl::Ephemeral),  // Cache this!
    },
];
```

### 3. Implement Tool Loop Safety

Always set a maximum iteration limit:

```rust
const MAX_TOOL_ITERATIONS: usize = 5;

for iteration in 0..MAX_TOOL_ITERATIONS {
    // ... tool handling ...
}

// If we exit due to max iterations, inform the user
if iteration == MAX_TOOL_ITERATIONS - 1 {
    return Err(AnthropicError::MaxIterationsReached.into());
}
```

### 4. Log with Session Context

Always include session IDs in logs for traceability:

```rust
info!("[session:{}] Tool use detected: {}", session_id, tool_name);
info!("[session:{}] Claude response: {}", session_id, response_text);
```

### 5. Handle Tool Results Carefully

Save tool results before sending back to Claude to prevent data loss on errors:

```rust
// 1. Execute tool
let result = tool.execute(args).await;

// 2. Save to DB BEFORE sending to Claude
save_tool_result(session_id, tool_use_id, &result).await?;

// 3. Then send to Claude
send_tool_results_to_claude(session_id).await?;
```

### 6. Use Structured Logging for Token Usage

```rust
info!(
    input_tokens = response.usage.input_tokens,
    output_tokens = response.usage.output_tokens,
    cache_creation = response.usage.cache_creation_input_tokens,
    cache_read = response.usage.cache_read_input_tokens,
    "[session:{}] Token usage", session_id
);
```

### 7. Model Selection

Current model identifiers (as of 2025):

| Model | Identifier | Use Case |
|-------|-----------|----------|
| Claude 4 Opus | `claude-opus-4-5-20250929` | Complex reasoning |
| Claude 4 Sonnet | `claude-sonnet-4-5-20250929` | Balanced |
| Claude 4 Haiku | `claude-haiku-4-5-20250929` | Fast, simple tasks |

---

## Quick Reference

### API Endpoints

```
POST https://api.anthropic.com/v1/messages
```

### Essential Headers

```rust
vec![
    ("x-api-key", api_key),
    ("anthropic-version", "2023-06-01"),
    ("content-type", "application/json"),
]
```

### Token Limits

| Model | Context Window | Max Output |
|-------|---------------|------------|
| Opus 4 | 200K tokens | 8K tokens |
| Sonnet 4 | 200K tokens | 8K tokens |
| Haiku 4 | 200K tokens | 4K tokens |

### Cache Pricing (relative to base input)

- Cache creation: 1.25x base price
- Cache read: 0.1x base price
- Cache TTL: 5 minutes

---

## Resources

- Anthropic API Docs: https://docs.anthropic.com/
- Messages API Reference: https://docs.anthropic.com/en/api/messages
- Prompt Caching Guide: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching
- Tool Use Guide: https://docs.anthropic.com/en/docs/build-with-claude/tool-use
- Models Overview: https://docs.anthropic.com/en/docs/about-claude/models

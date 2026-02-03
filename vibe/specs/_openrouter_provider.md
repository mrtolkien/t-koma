# OpenRouter Provider Implementation Spec

## Overview
Add OpenRouter as an alternative model provider alongside Anthropic. OpenRouter provides a unified API for accessing hundreds of AI models through a single OpenAI-compatible endpoint.

## Goals
1. Implement OpenRouter API client in the gateway
2. Support provider selection (Anthropic vs OpenRouter) via CLI interactive menu
3. Support model selection for OpenRouter with live fetching of top 20 models
4. Allow manual input of custom model IDs
5. Maintain backward compatibility with existing Anthropic-only setup

## Architecture

### Provider Abstraction

Create a `Provider` trait that both Anthropic and OpenRouter implement:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider name
    fn name(&self) -> &str;
    /// Current model
    fn model(&self) -> &str;
    /// Send a conversation and get response
    async fn send_conversation(
        &self,
        system: Option<Vec<SystemBlock>>,
        history: Vec<ApiMessage>,
        tools: Vec<&dyn Tool>,
        new_message: Option<&str>,
        message_limit: Option<usize>,
        tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError>;
}

/// Unified response type across providers
pub struct ProviderResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub usage: Option<ProviderUsage>,
    pub stop_reason: Option<String>,
}

pub struct ProviderUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
}
```

### OpenRouter API Client

**Endpoint:** `https://openrouter.ai/api/v1/chat/completions`

**Request Format (OpenAI-compatible):**
```json
{
  "model": "anthropic/claude-3.5-sonnet",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "tools": [...],
  "max_tokens": 4096
}
```

**Headers:**
- `Authorization: Bearer <OPENROUTER_API_KEY>`
- `Content-Type: application/json`
- `HTTP-Referer: https://t-koma.dev` (optional)
- `X-Title: t-koma` (optional)

**Response Format:**
```json
{
  "id": "gen-...",
  "model": "anthropic/claude-3.5-sonnet",
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you today?",
      "tool_calls": [...]
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30
  }
}
```

### OpenRouter Models API

**Endpoint:** `https://openrouter.ai/api/v1/models`

**Response:** Array of model objects with:
- `id`: Model ID (e.g., "anthropic/claude-3.5-sonnet")
- `name`: Display name
- `description`: Model description
- `context_length`: Max context length
- `pricing`: Token pricing

## Implementation Plan

### Phase 1: Configuration Updates

Update `t-koma-core/src/config.rs`:
- Add `openrouter_api_key: Option<String>`
- Add `openrouter_http_referer: Option<String>`
- Add `openrouter_app_name: Option<String>`

Update `.env.example` with new variables.

### Phase 2: Message Type Updates

Add new WebSocket messages to `t-koma-core/src/message.rs`:

```rust
pub enum WsMessage {
    // ... existing variants
    /// Select provider and model
    SelectProvider {
        provider: ProviderType,
        model: String,
    },
    /// Request available models from gateway
    ListAvailableModels {
        provider: ProviderType,
    },
}

pub enum WsResponse {
    // ... existing variants
    /// Provider selection confirmation
    ProviderSelected {
        provider: String,
        model: String,
    },
    /// Available models list
    AvailableModels {
        provider: String,
        models: Vec<ModelInfo>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Anthropic,
    OpenRouter,
}
```

### Phase 3: OpenRouter Client

Create `t-koma-gateway/src/models/openrouter/` module:
- `client.rs`: HTTP client implementation
- `mod.rs`: Module exports

The client will:
- Use OpenAI-compatible message format
- Support tool calling (OpenAI format)
- Convert between internal and OpenAI tool formats

### Phase 4: Provider Trait

Create `t-koma-gateway/src/models/provider.rs`:
- Define `Provider` trait
- Define `ProviderResponse` struct
- Define `ProviderError` type
- Define helper functions for text/tool extraction

Update `AppState` to hold `Arc<dyn Provider>` instead of concrete `AnthropicClient`.

### Phase 5: CLI Interactive Selection

The CLI will:
1. After connection, fetch available models from OpenRouter API directly
2. Show menu:
```
╔════════════════════════════════════╗
║     Select Model Provider          ║
╠════════════════════════════════════╣
║  1. Anthropic (Claude)             ║
║  2. OpenRouter                     ║
╚════════════════════════════════════╝

If OpenRouter selected:
Fetching top models from OpenRouter...

╔════════════════════════════════════╗
║     Select OpenRouter Model        ║
╠════════════════════════════════════╣
║  1. anthropic/claude-3.5-sonnet    ║
║     Claude 3.5 Sonnet by Anthropic ║
║  2. deepseek/deepseek-r1           ║
║     DeepSeek R1                    ║
║  ... (top 20 by popularity)        ║
║  0. Enter custom model ID          ║
╚════════════════════════════════════╝
```

### Phase 6: Gateway Updates

Update `t-koma-gateway/src/server.rs`:
- Handle `SelectProvider` message
- Handle `ListAvailableModels` message (proxy to OpenRouter API)
- Switch provider dynamically per session

Update `t-koma-gateway/src/state.rs`:
- Store multiple provider clients
- Switch provider based on session

Update `t-koma-gateway/src/main.rs`:
- Initialize both Anthropic and OpenRouter clients if keys available

## OpenRouter Model IDs (Popular)

| Model | ID |
|-------|-----|
| Claude 3.5 Sonnet | `anthropic/claude-3.5-sonnet` |
| Claude 3 Opus | `anthropic/claude-3-opus` |
| Claude 3 Haiku | `anthropic/claude-3-haiku` |
| DeepSeek R1 | `deepseek/deepseek-r1` |
| DeepSeek V3 | `deepseek/deepseek-chat` |
| GPT-4o | `openai/gpt-4o` |
| GPT-4o Mini | `openai/gpt-4o-mini` |
| Gemini Flash 1.5 | `google/gemini-flash-1.5` |
| Gemini Pro 1.5 | `google/gemini-pro-1.5` |
| Llama 3.1 405B | `meta-llama/llama-3.1-405b-instruct` |

## Security Considerations

1. OpenRouter API key stored securely (same as Anthropic key)
2. Provider selection is per-session, not persisted
3. Validate model IDs before sending to API
4. CLI fetches models directly from OpenRouter (no gateway proxy needed)

## Testing

1. Unit tests for OpenRouter client (mock responses)
2. Integration tests with live API (human-reviewed snapshots)
3. Test provider switching mid-session
4. Test custom model ID input

## Files to Modify

- `t-koma-core/src/config.rs` - Add OpenRouter config
- `t-koma-core/src/message.rs` - Add provider selection messages
- `t-koma-gateway/src/models/mod.rs` - Add openrouter module, provider trait
- `t-koma-gateway/src/models/openrouter/*.rs` - NEW: OpenRouter client
- `t-koma-gateway/src/models/provider.rs` - NEW: Provider trait
- `t-koma-gateway/src/state.rs` - Use Provider trait, support switching
- `t-koma-gateway/src/server.rs` - Handle provider selection messages
- `t-koma-gateway/src/main.rs` - Initialize both clients
- `t-koma-cli/src/main.rs` - Add provider selection menu, fetch models
- `t-koma-cli/src/app.rs` - Send provider selection
- `t-koma-cli/src/openrouter.rs` - NEW: OpenRouter API client for CLI
- `.env.example` - Add new env vars
- `vibe/knowledge/openrouter.md` - NEW: Documentation
- `AGENTS.md` - Update provider documentation

## Backward Compatibility

- Default provider remains Anthropic
- Existing `ANTHROPIC_API_KEY` still required if using Anthropic
- `OPENROUTER_API_KEY` is optional - only required if using OpenRouter
- Sessions without explicit provider selection use gateway default

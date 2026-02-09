# OpenRouter Integration

## Overview

OpenRouter provides a unified API for accessing hundreds of AI models through a
single OpenAI-compatible endpoint. t-koma integrates OpenRouter as an
alternative provider alongside the native Anthropic integration.

## Key Features

- **Unified API**: Access models from Anthropic, OpenAI, Google, Meta, and more
  through a single endpoint
- **Model Selection**: CLI provides interactive model selection from top 20
  popular models
- **Fallback Support**: Automatic fallback to alternative providers if a model
  is unavailable
- **Cost Optimization**: OpenRouter routes requests to the most cost-effective
  provider

## Configuration

### Environment Variables

```bash
# Required for OpenRouter
OPENROUTER_API_KEY=your-api-key-here

# Optional - for OpenRouter rankings
OPENROUTER_HTTP_REFERER=https://your-site.com
OPENROUTER_APP_NAME=Your App Name

# Set default provider
DEFAULT_PROVIDER=openrouter  # or 'anthropic'
```

### API Key

Get your API key from: https://openrouter.ai/keys

## Architecture

### Provider Trait

Both Anthropic and OpenRouter implement the `Provider` trait:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;

    async fn send_message(&self, content: &str) -> Result<ProviderResponse, ProviderError>;

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
```

### OpenRouter Client

Located in `t-koma-gateway/src/providers/openrouter/client.rs`:

- Converts internal message format to OpenAI-compatible format
- Handles tool calling via OpenAI's function calling format
- Fetches available models from `/api/v1/models`

### Per-model provider routing

OpenRouter `provider.order` routing is configured directly on each model:

```toml
[models.kimi25]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"
routing = ["anthropic"]
```

Notes:

- `routing` is only valid for OpenRouter models (`provider = "openrouter"`).
- `routing` must include at least one non-empty provider slug when set.

### Message Format Conversion

The OpenRouter client performs these conversions:

1. **System Prompt**: Combined into a single `system` message
2. **History**: Converted to OpenAI's `messages` array with `role` and `content`
3. **Tools**: Converted to OpenAI's `functions` format
4. **Tool Results**: Formatted as user messages with tool output

## CLI Provider Selection

When starting the CLI chat mode, users are presented with:

1. **Provider Selection**:

   ```
   ╔════════════════════════════════════╗
   ║     Select Model Provider          ║
   ╠════════════════════════════════════╣
   ║  1. Anthropic (Claude)             ║
   ║  2. OpenRouter                     ║
   ╚════════════════════════════════════╝
   ```

2. **Model Selection** (for OpenRouter):
   - Fetches top 20 models from OpenRouter API
   - Allows custom model ID entry
   - Shows model names and descriptions

## API Endpoints

### OpenRouter API

- **Base URL**: `https://openrouter.ai/api/v1`
- **Chat Completions**: `POST /chat/completions`
- **Models**: `GET /models`

### Request Format

```json
{
  "model": "anthropic/claude-3.5-sonnet",
  "provider": {
    "order": ["anthropic"],
    "allow_fallbacks": false
  },
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "tools": [...],
  "max_tokens": 4096
}
```

### Response Format

```json
{
  "id": "gen-...",
  "model": "anthropic/claude-3.5-sonnet",
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you today?",
      "tool_calls": [...]
    }
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30,
    "prompt_tokens_details": {
      "cached_tokens": 5
    }
  }
}
```

### Cache observability

The OpenRouter client parses cache usage details when OpenRouter includes them
in the `usage` payload, and maps them into `ProviderUsage`:

- `cache_read_tokens`
- `cache_creation_tokens`

If OpenRouter does not return cache detail fields, both remain `None`.

## WebSocket Messages

### SelectProvider

Client → Gateway:

```json
{
  "type": "select_provider",
  "provider": "openrouter",
  "model": "anthropic/claude-3.5-sonnet"
}
```

Gateway → Client:

```json
{
  "type": "provider_selected",
  "provider": "openrouter",
  "model": "anthropic/claude-3.5-sonnet"
}
```

### ListAvailableModels

Client → Gateway:

```json
{
  "type": "list_available_models",
  "provider": "openrouter"
}
```

Gateway → Client:

```json
{
  "type": "available_models",
  "provider": "openrouter",
  "models": [
    {
      "id": "anthropic/claude-3.5-sonnet",
      "name": "Claude 3.5 Sonnet",
      "description": "...",
      "context_length": 200000
    }
  ]
}
```

## Error Handling

### Common Errors

1. **Invalid API Key**: Returns 401 Unauthorized
2. **Model Not Found**: Returns 404 with model ID
3. **Rate Limiting**: Returns 429 with retry-after header
4. **Provider Error**: Returns 502 with upstream error details

### Error Response

```json
{
  "error": {
    "message": "Invalid model ID",
    "type": "invalid_model"
  }
}
```

## Tool Calling

OpenRouter supports tool calling via the OpenAI format:

```json
{
  "tools": [{
    "type": "function",
    "function": {
      "name": "run_shell_command",
      "description": "Execute a shell command",
      "parameters": {...}
    }
  }]
}
```

The t-koma gateway:

1. Converts internal tool definitions to OpenAI format
2. Extracts `tool_calls` from responses
3. Executes tools and returns results
4. Continues conversation with tool results

## Testing

### Unit Tests

```bash
cargo test -p t-koma-gateway
```

### Live Tests

```bash
cargo test -p t-koma-gateway --features live-tests
```

Note: Live tests require `OPENROUTER_API_KEY` to be set.

## Security Considerations

1. **API Key Storage**: OpenRouter API key stored alongside other provider keys
2. **No Key Exposure**: API keys never exposed via WebSocket or HTTP APIs
3. **Model Validation**: User-provided model IDs are validated before use
4. **Request Headers**: Optional `HTTP-Referer` and `X-Title` for attribution

## Migration Guide

### From Anthropic Only

1. Add `OPENROUTER_API_KEY` to `.env`
2. Add or update `default_model` in `config.toml` (optional)
3. Restart gateway
4. Use CLI to select OpenRouter models

### Backward Compatibility

- Existing Anthropic-only setups continue to work
- `ANTHROPIC_API_KEY` still required if using Anthropic
- CLI uses the configured `default_model` if no selection is made

## Resources

- [OpenRouter Docs](https://openrouter.ai/docs)
- [Models List](https://openrouter.ai/models)
- [API Keys](https://openrouter.ai/keys)
- [Pricing](https://openrouter.ai/models?tab=pricing)

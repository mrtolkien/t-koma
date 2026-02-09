# OpenAI-Compatible Provider Config

## Summary

T-KOMA now supports a generic `openai_compatible` provider at model level.
This replaces the previous global llama.cpp base URL layout.

## Model-level fields

Use these fields on `[models.<alias>]`:

- `provider = "openai_compatible"`
- `model = "your-model-id"`
- `base_url = "http://host:port"` (or `.../v1`)
- `api_key_env = "CUSTOM_ENV_VAR"` (optional)
- `context_window = 200000` (optional)

Default API key env var is `OPENAI_API_KEY` when `api_key_env` is not set.

## OpenRouter routing

OpenRouter keeps `provider = "openrouter"` but routing now lives on each model:

- `routing = ["anthropic"]`

This is passed as the OpenRouter `provider.order` field.

## Example

```toml
[models.local_qwen]
provider = "openai_compatible"
model = "qwen2.5"
base_url = "http://127.0.0.1:8080"

[models.kimi25]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"
routing = ["anthropic"]
```

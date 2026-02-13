# OpenAI Compatible

Generic OpenAI-compatible endpoints (Ollama, vLLM, llama.cpp, etc.).

## Environment Variable

- `OPENAI_API_KEY` (default, optional)
- Override per model with `api_key_env` field.

## Configuration

```toml
[models.local-llama]
provider = "openai_compatible"
model = "llama3.1"
base_url = "http://127.0.0.1:8080"

[models.custom-hosted]
provider = "openai_compatible"
model = "my-model"
base_url = "http://my-host:8000/v1"
api_key_env = "MY_CUSTOM_KEY" # reads from $MY_CUSTOM_KEY instead of $OPENAI_API_KEY
```

## Notes

- `base_url` is required. The client auto-appends `/v1/chat/completions` if the URL does
  not already end in `/v1`.
- Auth is optional. When `api_key_env` is unset, falls back to `OPENAI_API_KEY`. If
  neither is set, requests are sent without an Authorization header.

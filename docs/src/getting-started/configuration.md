# Configuration

t-koma is configured through a TOML file and environment variables.

## Config File Location

| Platform | Path                                               |
| -------- | -------------------------------------------------- |
| Linux    | `~/.config/t-koma/config.toml`                     |
| macOS    | `~/Library/Application Support/t-koma/config.toml` |
| Windows  | `%APPDATA%/t-koma/config.toml`                     |

Override with `T_KOMA_CONFIG_DIR` environment variable.

## Example Configuration

```toml
default_model = "primary"
heartbeat_model = "fallback"

[models]
[models.primary]
provider = "openrouter"
model = "your-openrouter-model-id"
routing = ["anthropic"] # optional OpenRouter provider order

[models.fallback]
provider = "openai_compatible"
model = "your-local-model-id"
base_url = "http://127.0.0.1:8080"

[gateway]
host = "127.0.0.1"
port = 3000
```

## Model Configuration

Each model alias in the `[models]` table requires:

- `provider` — one of: `anthropic`, `openrouter`, `gemini`, `kimi_code`,
  `openai_compatible`
- `model` — the model identifier for that provider

Optional model fields:

- `base_url` — override the provider's default API endpoint
- `api_key_env` — environment variable name for the API key (overrides default)
- `routing` — upstream provider order (OpenRouter only)
- `headers` — custom HTTP headers (as a TOML table)
- `retry_on_empty` — retry when the model returns an empty response (default: false)

## Multi-Model Fallback

`default_model` and `heartbeat_model` accept a single alias or an ordered list:

```toml
# Single model:
default_model = "primary"

# Fallback chain (tries each in order):
default_model = ["kimi25", "gemma3", "qwen3"]
heartbeat_model = ["alpha", "kimi25"]
```

When a model hits rate limits (429) or server errors (5xx), t-koma automatically falls
back to the next model in the chain. See
[Multi-Model Fallback](../concepts/multi-model-fallback.md) for details.

## Gateway Settings

```toml
[gateway]
host = "127.0.0.1" # bind address
port = 3000 # HTTP/WebSocket port
```

## Heartbeat Timing

```toml
[heartbeat_timing]
idle_minutes = 4 # minutes of idle before heartbeat triggers
check_seconds = 60 # scheduler polling interval
continue_minutes = 30 # minutes between heartbeat re-checks
```

## Data Directory

Data is stored at the platform data directory:

| Platform | Path                                    |
| -------- | --------------------------------------- |
| Linux    | `~/.local/share/t-koma/`                |
| macOS    | `~/Library/Application Support/t-koma/` |

Override with `T_KOMA_DATA_DIR` environment variable. This directory contains:

- `koma.sqlite3` — unified main database (operators, ghosts, interfaces, sessions,
  messages, usage/job logs)
- `ghosts/<name>/` — per-ghost workspace and knowledge files
- `shared/` — shared knowledge (notes and references)

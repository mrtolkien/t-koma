# OpenRouter

Multi-model routing via [OpenRouter](https://openrouter.ai/).

## Environment Variable

- `OPENROUTER_API_KEY` (required)

## Configuration

```toml
[models.or-sonnet]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"

[models.or-deepseek]
provider = "openrouter"
model = "deepseek/deepseek-chat-v3-0324"
routing = ["deepinfra", "fireworks"] # optional upstream provider fallback chain
```

## Notes

- Upstream routing is configured per model with `routing = ["provider-slug"]` on
  `[models.<alias>]`. This controls which upstream providers OpenRouter tries, in order.
- Model names use the `org/model` format from OpenRouter's model list.

# Anthropic

Direct Anthropic API access.

## Environment Variable

- `ANTHROPIC_API_KEY` (required)

## Configuration

```toml
[models.claude-sonnet]
provider = "anthropic"
model = "claude-sonnet-4-5-20250929"

[models.claude-haiku]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
```

## Notes

- Context window: 200,000 tokens (model-dependent).
- Supports prompt caching via cache control headers.

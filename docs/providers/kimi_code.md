# Kimi Code

Moonshot Kimi Code API access. Reuses `OpenAiCompatibleClient` internally.

## Environment Variable

- `KIMI_API_KEY` (required)

## Configuration

```toml
[models.kimi]
provider = "kimi_code"
model = "kimi-k2-0711-chat"

[models.kimi-custom-url]
provider = "kimi_code"
model = "kimi-k2-0711-chat"
base_url = "https://custom-endpoint.example.com/v1" # optional override
```

## Notes

- Default base URL: `https://api.kimi.com/coding/v1` (overridable via `base_url`).
- Context window: 262,144 tokens.
- Kimi Code is distinct from Moonshot Open Platform (`api.moonshot.ai`) -- different
  endpoints, keys, and models.

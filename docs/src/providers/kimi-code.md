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

## User-Agent Header

The Kimi Code API (`api.kimi.com/coding`) restricts access to recognized coding agents
via the `User-Agent` header. t-koma sends `User-Agent: KimiCLI/1.12.0` by default. This
can be overridden via the per-model `headers` config field:

```toml
[models.kimi.headers]
User-Agent = "KimiCLI/2.0.0"
```

## Notes

- Default base URL: `https://api.kimi.com/coding/v1` (overridable via `base_url`).
- Context window: 262,144 tokens.
- Kimi Code is distinct from Moonshot Open Platform (`api.moonshot.ai`) — different
  endpoints, keys, and models.
- Per-model `headers` config allows setting arbitrary HTTP headers for any
  OpenAI-compatible or Kimi Code provider.

# Gemini (Google)

Google Gemini API access.

## Environment Variable

- `GEMINI_API_KEY` (required)

## Configuration

```toml
[models.gemini-flash]
provider = "gemini"
model = "gemini-2.0-flash-exp"

[models.gemini-pro]
provider = "gemini"
model = "gemini-2.0-pro-exp"
```

## Notes

- Base URL: `https://generativelanguage.googleapis.com/v1beta`.
- Context window: up to 1,000,000 tokens (model-dependent).

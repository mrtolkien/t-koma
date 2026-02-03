# Web Tools

## Overview
T-KOMA exposes two web tools:

- `web_search`: Brave Search API-backed web search with structured results.
- `web_fetch`: HTTP fetch with basic HTML-to-text conversion (no JS).

Both tools are controlled via `~/.config/t-koma/config.toml` and respect
in-process caching.

## Configuration

```toml
[tools.web]
enabled = true

[tools.web.search]
enabled = true
provider = "brave"
max_results = 5
timeout_seconds = 30
cache_ttl_minutes = 15
min_interval_ms = 1000

[tools.web.fetch]
enabled = true
provider = "http"
mode = "markdown"
max_chars = 20000
timeout_seconds = 30
cache_ttl_minutes = 15
```

### Secrets (env)

- `BRAVE_API_KEY`

## Rate Limiting

Brave free tier is ~1 query/second. The implementation enforces a minimum
interval and backs off when it receives HTTP 429 responses.

## Notes for Future Providers (Not Implemented Yet)

These are intentionally deferred but captured here for later implementation:

- Perplexity / Sonar: can be offered as a search provider, either directly via
  Perplexity API or via OpenRouter models.
- Z.ai: potential additional search provider if their API supports it.
- Firecrawl: optional web_fetch provider for JS-heavy pages. API supports
  options like `onlyMainContent`, `maxAge`, and markdown output.

Additions should be implemented behind the same `web_search` / `web_fetch` tool
interfaces so the agent sees a single stable tool surface.

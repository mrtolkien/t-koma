# Web Tools (web_search + web_fetch)

## Goal

Add first-class web access tools to T-KOMA with a simple, unified tool surface
for the agent while allowing configurable provider backends for search and
fetch.

## Context / Research Notes

- OpenClaw provides `web_search` and `web_fetch` with provider selection,
  caching, and a fetch pipeline that can use readability + optional Firecrawl
  for JS-heavy pages.
- Brave Search API supports structured web search results via REST with API key
  authentication.

## Non-Goals

- No browser automation.
- No executing JavaScript for web pages.
- No direct model-provider tool integrations (Anthropic/OpenAI tool
  implementations remain provider-agnostic on our side).
- No Perplexity/Z.ai/Firecrawl providers in this phase.

## User-Facing Tool Surface (Agent)

Expose two tools to the model:

- `web_search`:
  - Input:
    `{ query: string, count?: number, country?: string, search_lang?: string, ui_lang?: string, freshness?: string }`
  - Output: JSON string with list of results (title, url, snippet,
    source/provider metadata)
- `web_fetch`:
  - Input: `{ url: string, mode?: "text" | "markdown", max_chars?: number }`
  - Output: JSON string with
    `{ url, status, content_type, content, truncated, provider }`

## Configuration

Add non-sensitive settings in config TOML; secrets from env.

### Settings (config.toml)

```
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

## Providers

### web_search providers

- Brave Search API (default, structured results)

### web_fetch providers

- HTTP + basic HTML-to-text extraction (default)

## Implementation Outline

1. **Config**
   - Extend `t-koma-core/src/config/settings.rs` with `tools.web` settings
     structs and defaults.
   - Extend `t-koma-core/src/config/secrets.rs` for `BRAVE_API_KEY`.

2. **Tool Definitions**
   - Add tools in `t-koma-gateway/src/tools/`:
     - `web_search.rs`
     - `web_fetch.rs`
   - Register tools in `ToolManager`.
   - Provide `prompt()` guidance describing safe usage and limitations (no JS,
     no logins).

3. **Provider Layer (gateway)**
   - Add `t-koma-gateway/src/web/` module:
     - `search/mod.rs` with provider trait and rate-limited Brave
       implementation
     - `search/brave.rs`
     - `fetch/mod.rs` with provider trait
     - `fetch/http.rs` (reqwest + simple HTML-to-text conversion)
   - Central entrypoint used by tools.

4. **Caching**
   - In-memory cache with TTL for search and fetch (per process), keyed by query
     or URL + params.

5. **Tests**
   - Unit tests for provider selection and schema validation.
   - Use mock HTTP in provider tests (no live tests).

6. **Docs / Knowledge**
   - Update `AGENTS.md` with new web tool section.
   - Add `vibe/knowledge/web_tools.md` with provider setup instructions and
     caveats, including future provider notes (Perplexity/Z.ai/Firecrawl).

## Decisions (validated)

- Search provider: Brave only.
- Fetch provider: HTTP only (no Firecrawl yet).
- Persist notes about Perplexity/Z.ai/Firecrawl for later.
- Handle Brave rate limits: enforce minimum 1 request/sec and back off on 429s.

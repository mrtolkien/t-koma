---
name: reference-researcher
description: Advanced research strategies, import patterns, crawl configuration, and staleness management for reference topics.
license: MIT
metadata:
  author: t-koma
  version: "2.0"
---

# Reference Researcher

You are a research specialist. This skill covers advanced strategies for
building high-quality reference topics. Basic reference_write and reference_import
usage is in the system prompt — this skill adds depth.

## Research Strategy

Follow this priority order to understand a topic before importing:

1. **Find documentation first**: Use `web_search` to find official docs. Many
   projects have a dedicated docsite repo (e.g. `org/docs`, `org/website`) that
   contains more useful content than the code repo itself.
   ```
   web_search(query="dioxus official documentation site")
   ```

2. **Read key doc pages**: Use `web_fetch` to read specific pages.
   ```
   web_fetch(url="https://dioxuslabs.com/learn/0.6/", prompt="Key concepts and getting started?")
   ```

3. **Find the code repository**: Use `gh search repos` to locate the main repo.
   ```
   run_shell_command(command="gh search repos 'dioxus' --language=Rust --sort=stars --limit=5")
   ```

4. **Read repo metadata**: Use `gh api` for description, stars, topics.
   ```
   run_shell_command(command="gh api repos/DioxusLabs/dioxus --jq '.description, .stargazers_count, .topics'")
   ```

5. **Understand before creating**: Read enough to write a meaningful topic
   description and pick the right sources.

## Import Patterns

### Default: Fetch the Entire Repository

The embedding system handles large codebases well (tree-sitter chunking, hybrid
search). Default to full repo imports:

```json
{
  "title": "Dioxus - Rust UI Framework",
  "body": "Dioxus is a portable, performant framework for building cross-platform UIs...",
  "sources": [
    {"type": "git", "url": "https://github.com/DioxusLabs/dioxus", "ref": "main"},
    {"type": "git", "url": "https://github.com/DioxusLabs/docsite", "ref": "main", "role": "docs"},
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/"}
  ],
  "tags": ["rust", "ui", "framework", "dioxus"],
  "max_age_days": 30
}
```

### Source Roles

- **`docs`**: Documentation content. Boosted 1.5x in search.
- **`code`**: Source code. Normal ranking.
- Inferred if omitted: `web` → docs, `git` → code.
- For git repos that ARE documentation (docsites, wikis), set `"role": "docs"`
  explicitly.

### If the Operator Denies (Too Large)

Retry with a `paths` filter. Prioritize:

1. `README.md` — project overview
2. `docs/` — official documentation
3. `examples/` — practical usage patterns
4. Key source directories the operator identified

```json
{
  "sources": [
    {
      "type": "git",
      "url": "https://github.com/DioxusLabs/dioxus",
      "ref": "main",
      "paths": ["README.md", "docs/", "examples/"]
    }
  ]
}
```

### Multi-Source Imports

Combine git repos, web pages, and crawls in a single import. Look for separate
documentation repos — they often have better content than the main repo's docs:

```json
{
  "sources": [
    {"type": "git", "url": "https://github.com/org/library", "ref": "main"},
    {"type": "git", "url": "https://github.com/org/library-docs", "ref": "main", "role": "docs"},
    {"type": "crawl", "url": "https://library.dev/docs/", "max_depth": 2, "max_pages": 50}
  ]
}
```

### Crawl Sources

Use `"type": "crawl"` to automatically discover and fetch linked pages from a
documentation site:

- `max_depth` (default 1, max 3): How many link-hops from the seed URL.
- `max_pages` (default 20, max 100): Maximum pages to fetch.
- Only follows links on the same host as the seed URL.

**When to crawl vs list individual pages:**
- **Crawl**: Documentation sites with clear navigation structure, API reference
  sites with many sub-pages, wikis.
- **Individual pages**: Sites with noisy navigation (forums, blogs), when you
  only need specific pages, landing pages with irrelevant links.

## Writing a Good Topic Description

The `body` is passed IN FULL to the LLM as context. Write it as a concise
briefing:

1. **Opening paragraph**: 2-3 sentence recap (good for embeddings).
2. **Key concepts**: Bullet-point core abstractions and patterns.
3. **Content notes**: Caveats about reference contents — known gaps, version
   issues, weak documentation areas.

Do NOT include code examples in the body — those belong in reference files.

## Staleness Management

### Setting max_age_days

- **30 days**: Actively developed libraries with frequent releases.
- **90 days**: Stable libraries with infrequent changes.
- **0 (never stale)**: Specs, RFCs, historical docs that won't change.

### Managing Quality

Use `reference_write` with action `update` to manage file status:

- Update tags and body on existing topics as you learn more.
- Mark files as `problematic` when you find partial inaccuracies.
- Mark files as `obsolete` when they would actively mislead.
- Always provide a reason — it's appended to the topic body as a warning.

```
reference_write(action="update", topic="dioxus", note_id="abc123",
  status="problematic", reason="API examples use v0.4 syntax, current is v0.6")
```

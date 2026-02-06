---
name: reference-researcher
description: Guide for gathering and curating reference material from external sources. Use when you need to research a library, framework, or API and build a searchable reference topic.
license: MIT
metadata:
  author: t-koma
  version: "1.0"
---

# Reference Researcher

You are a research specialist. Your job is to gather high-quality reference
material from external sources and build searchable reference topics that you and
other ghosts can use in future conversations.

## When to Create a Reference Topic

Create a reference topic when:

- You need to work with a library or framework not well-covered by your training
  data (cutting-edge or recently updated).
- The operator asks you to learn about a specific technology.
- You encounter a domain where precise, up-to-date information is critical
  (API specs, protocol docs, configuration references).

Do NOT create a reference topic for:

- Well-known, stable libraries you already know well (e.g., serde, tokio basics).
- Temporary or one-off questions that don't warrant indexing.

## Before Creating: Always Search First

1. **Check the system prompt** for "Available Reference Topics" — the 10 most
   recent topics are listed there.
2. **Use `reference_topic_search`** with a semantic query. Different models may
   have used different names for the same concept, so search broadly:
   ```
   reference_topic_search(query="Rust GUI framework with React-like syntax")
   ```
3. If a topic exists but is stale, consider updating it with
   `reference_topic_update` rather than creating a duplicate.

## Research Strategy

Follow this priority order to understand the topic before creating a reference:

1. **Find the repository**: Use `gh search repos` to locate the main repo.
   ```
   run_shell_command(command="gh search repos 'dioxus' --language=Rust --sort=stars --limit=5")
   ```

2. **Read repo metadata**: Use `gh api` to get description, stars, topics.
   ```
   run_shell_command(command="gh api repos/DioxusLabs/dioxus --jq '.description, .stargazers_count, .topics'")
   ```

3. **Find documentation sites**: Use `web_search` to find official docs.
   ```
   web_search(query="dioxus official documentation site")
   ```

4. **Read key pages**: Use `web_fetch` to read specific doc pages.
   ```
   web_fetch(url="https://dioxuslabs.com/learn/0.6/", prompt="What are the key concepts and getting started steps?")
   ```

5. **Understand before creating**: Read enough to write a meaningful topic
   description. The body should summarize the library's purpose, key concepts,
   and common patterns.

## Creating the Reference Topic

### Source Selection

**Default to fetching the entire repository.** The embedding system handles
large codebases well (tree-sitter chunking, hybrid search). The operator will be
asked to approve before anything is downloaded.

```json
{
  "title": "Dioxus - Rust UI Framework",
  "body": "Dioxus is a portable, performant framework for building cross-platform UIs in Rust...",
  "sources": [
    {"type": "git", "url": "https://github.com/DioxusLabs/dioxus", "ref": "main"},
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/"}
  ],
  "tags": ["rust", "ui", "framework", "dioxus"],
  "max_age_days": 30
}
```

### If the Operator Denies (Too Large)

Retry with a `paths` filter. Prioritize in this order:

1. `README.md` — always include the project overview
2. `docs/` — official documentation
3. `examples/` — practical usage patterns
4. Key source directories the operator or you identified as important

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

### Web Sources

Each web page is a separate source entry. There is no automatic crawling — list
the specific pages you want indexed:

```json
{
  "sources": [
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/"},
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/guide/state/"},
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/guide/routing/"}
  ]
}
```

## Writing a Good Topic Description

The `body` field is indexed and searchable. Write it as if explaining the topic
to another ghost who has never heard of it:

- What is this library/framework?
- What problem does it solve?
- What are the key concepts and abstractions?
- What are common patterns or gotchas?

A good body makes the topic discoverable via `reference_topic_search` even when
the searcher uses different terminology than the topic title.

## Setting max_age_days

- **30 days**: Actively developed libraries with frequent releases.
- **90 days**: Stable libraries with infrequent changes.
- **0 (never stale)**: Reference material that won't change (specs, RFCs,
  historical docs).

## Managing Existing Topics

- **Stale topics**: If `reference_topic_list` shows a stale topic, alert the
  operator and suggest refreshing it (refresh is a CLI operation).
- **Obsolete topics**: Use `reference_topic_update` to mark a topic as
  `"obsolete"` when the library is deprecated or superseded.
- **Tag updates**: Keep tags current to aid discoverability.

## Using Reference Material

After creating a topic, use `reference_search` to find specific information
within the indexed content:

```
reference_search(topic="dioxus", question="how to handle form input events")
```

This searches the chunked and embedded source files, returning the most relevant
code snippets and documentation passages.

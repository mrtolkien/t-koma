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

1. **Find documentation first**: Use `web_search` to find official docs. Many
   projects have a dedicated docsite repo (e.g. `org/docs`, `org/website`) that
   contains more useful content than the code repo itself.
   ```
   web_search(query="dioxus official documentation site")
   ```

2. **Read key doc pages**: Use `web_fetch` to read specific doc pages.
   ```
   web_fetch(url="https://dioxuslabs.com/learn/0.6/", prompt="What are the key concepts and getting started steps?")
   ```

3. **Find the code repository**: Use `gh search repos` to locate the main repo.
   ```
   run_shell_command(command="gh search repos 'dioxus' --language=Rust --sort=stars --limit=5")
   ```

4. **Read repo metadata**: Use `gh api` to get description, stars, topics.
   ```
   run_shell_command(command="gh api repos/DioxusLabs/dioxus --jq '.description, .stargazers_count, .topics'")
   ```

5. **Understand before creating**: Read enough to write a meaningful topic
   description. The body should summarize the library's purpose, key concepts,
   and common patterns.

## Two Write Paths

### `reference_import` — Bulk Import from External Sources

Use `reference_import` to clone git repos and fetch web pages in bulk. The
operator will be asked to approve before anything is downloaded.

**Default to fetching the entire repository.** The embedding system handles
large codebases well (tree-sitter chunking, hybrid search).

```json
{
  "title": "Dioxus - Rust UI Framework",
  "body": "Dioxus is a portable, performant framework for building cross-platform UIs in Rust...",
  "sources": [
    {"type": "git", "url": "https://github.com/DioxusLabs/dioxus", "ref": "main"},
    {"type": "git", "url": "https://github.com/DioxusLabs/docsite", "ref": "main", "role": "docs"},
    {"type": "web", "url": "https://dioxuslabs.com/learn/0.6/"}
  ],
  "tags": ["rust", "ui", "framework", "dioxus"],
  "max_age_days": 30
}
```

#### Source Roles

Each source has an optional `role` field: `"docs"` or `"code"`.

- **`docs`**: Documentation content. Boosted in search results (1.5x by default).
- **`code`**: Source code. Normal ranking.
- If omitted, role is inferred: `web` sources default to `docs`, `git` sources
  default to `code`.
- For git repos that are primarily documentation (docsites, wikis), set
  `"role": "docs"` explicitly to get the search boost.

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

### `reference_save` — Incremental Content Saving

Use `reference_save` to add individual files to a topic incrementally. No
operator approval needed. The topic and collection are created automatically if
they don't exist.

```
reference_save(
  topic="3d-printers",
  path="bambulab-a1/specs.md",
  content="...",
  source_url="https://wiki.bambulab.com/en/a1/specs",
  collection_title="BambuLab A1",
  collection_description="Specs and troubleshooting for the BambuLab A1 printer"
)
```

Key points:
- **Always search for existing topics first** with `reference_topic_search`.
- Topic names are fuzzy-matched (e.g. "3d printers" matches "3d-printers").
- Use subdirectory paths (`collection/file.md`) to organize into collections.
- Provide `collection_title` and `collection_description` — they're embedded
  alongside file chunks for better search quality.
- Use `reference_import` for bulk git/web imports; use `reference_save` for
  individual web pages, data, or content you compose.

## Writing a Good Topic Description

The `body` is passed IN FULL to the LLM as context when `reference_search`
matches the topic. Write it as a concise briefing — not a tutorial, not a full
explanation, but the essential context an LLM needs to work with this technology:

1. **Opening paragraph**: A 2-3 sentence recap of the topic (good for
   embeddings and discoverability).
2. **Key concepts**: Bullet-point the core abstractions, terminology, and
   patterns the LLM should know when reading the reference files.
3. **Content notes**: Caveats about the reference contents — known gaps, version
   discrepancies, areas where the docs are weak. These notes help the LLM
   interpret search results correctly.

Do NOT include code examples in the body — those belong in the reference files
themselves. The body is context, not content.

A good body makes the topic discoverable via `reference_topic_search` even when
the searcher uses different terminology than the topic title.

## Setting max_age_days

- **30 days**: Actively developed libraries with frequent releases.
- **90 days**: Stable libraries with infrequent changes.
- **0 (never stale)**: Reference material that won't change (specs, RFCs,
  historical docs).

## Managing Existing Topics

- **Tag updates**: Keep tags current to aid discoverability using
  `reference_topic_update`.
- **Body updates**: Update the topic body when you learn new information about
  the subject.

### Marking Individual Files

Use `reference_file_update` to manage the quality of individual reference files.
Three status levels:

- **`active`** (default): Normal ranking in search results.
- **`problematic`**: File has some incorrect or misleading information. Still
  searchable, but penalized (0.5x score). Use when a file is partially wrong
  but still contains useful content. Always provide a reason.
- **`obsolete`**: File is completely outdated or wrong. Excluded from search
  entirely. Use when a file would actively mislead. Always provide a reason.

```
reference_file_update(note_id="abc123", status="problematic", reason="API examples use v0.4 syntax, current version is v0.6")
```

The reason is appended to the topic.md body as a warning note, so future
researchers (ghosts or humans) understand why the file was flagged.

## Using Reference Material

After creating a topic, use `reference_search` to find specific information
within the indexed content:

```
reference_search(topic="dioxus", question="how to handle form input events")
```

This searches the chunked and embedded source files, returning the most relevant
code snippets and documentation passages. The response also includes the full
topic body as context.

To read the complete content of a specific reference file (not just snippets),
use `reference_get`:

```
reference_get(topic="dioxus", file_path="examples/form_input.rs")
reference_get(note_id="abc123")
```

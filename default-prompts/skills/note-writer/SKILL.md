---
name: note-writer
description: Create and manage structured knowledge notes. Use when you need to persist important information, create concept notes, record decisions, or maintain a knowledge base.
license: MIT
metadata:
  author: t-koma
  version: "1.0"
---

# Note Writer Skill

You are now in note-writing mode. This skill gives you access to tools for
creating, updating, validating, and commenting on structured knowledge notes.

## When to Create Notes

Create notes when:
- The operator shares important context that should persist across sessions
- You discover something reusable (a pattern, a decision, a gotcha)
- Research results should be preserved for future reference
- A concept needs to be defined and tracked

Do NOT create notes for:
- Ephemeral chat context (use `memory_capture` instead for quick inbox items)
- Reference material from external sources (use the reference-researcher skill)
- Diary entries (those are automatic)

## Note Structure

Every note has TOML front matter and a markdown body:

```markdown
+++
id = "auto-generated"
title = "Clear, searchable title"
note_type = "Concept"
scope = "ghost_note"
trust_score = 5
tags = ["relevant", "searchable", "tags"]
created_at = "auto"
[created_by]
ghost = "your-name"
model = "your-model"
+++

# Title

Body content in markdown. Be concise but complete.
```

## Note Types

Choose the right type for discoverability:

| Type | Use For |
|------|---------|
| `Concept` | Definitions, explanations, mental models |
| `Decision` | Architectural choices, trade-offs, rationale |
| `Procedure` | Step-by-step how-tos, workflows |
| `Observation` | Patterns noticed, empirical findings |
| `Reference` | Quick-reference cards, cheat sheets |

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4-6**: Reasonable confidence, based on experience or documentation
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by operator or primary sources

Start at 5 for most notes. Adjust with `memory_note_validate` as confidence
changes.

## Tags

Use consistent, lowercase tags. Prefer existing tags over creating new ones.
Check what tags exist with `knowledge_search` before creating notes.

Good tags: `rust`, `architecture`, `debugging`, `api-design`
Bad tags: `Important`, `TODO`, `misc`

## Updating vs. Creating

Before creating a new note, search first:
1. Use `knowledge_search` to check if a similar note exists
2. If found, use `memory_note_update` to refine it
3. If not found, create a new note

## Comments

Use `memory_note_comment` to append timestamped observations to existing notes
without changing the main body. Good for:
- Recording when a note was confirmed or contradicted
- Adding context from new conversations
- Noting that related information was found elsewhere

## Scope

- **private** (default): Private to you. Use for personal observations and
  working notes.
- **shared**: Visible to all ghosts. Use for established knowledge that
  benefits everyone.

Start with ghost scope. Promote to shared when the note is validated and
broadly useful.

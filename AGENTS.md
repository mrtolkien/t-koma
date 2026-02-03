# t-koma AGENTS.md

This document provides essential information for AI coding agents working on the
T-KOMA (ティーコマ) project.

## CRUCIAL

You should ALWAYS EDIT THIS FILE if:

- Any changes you make change the structure or features defined here
- You see outdated information in this file
- You make assumptions about a library that we use that turns out to be wrong

If you implement a complex feature that will need to be referenced later, save
important information in vibe/knowledge.

## Core Concepts

- T-KOMA (ティーコマ): The gateway service. Deterministic logic only.
- OPERATOR (オペレータ): End user. Approved via management CLI.
- INTERFACE: A messaging endpoint for an operator (Discord, TUI). An operator
  can have multiple interfaces.
- GHOST (ゴースト): Agent with its own DB and workspace/safe house (same folder
  as ghost DB).
- SESSION: A chat thread between an operator and a ghost (stored in ghost DB).
- Puppet Master: The name used for WebSocket clients.

Relationship summary:

- An operator owns multiple ghosts.
- A ghost can have multiple sessions with its owner.
- Ghost names are unique per T-KOMA.

## Development Flow

- Always start by creating a markdown spec in `vibe/specs/` for validation by
  the user.
- After validation, create an append-only tracking file in `vibe/ongoing/`.
- Update the ongoing file after each meaningful step.
- Iterate until all spec items are built and, if realistic, tested.
- Run `cargo check --all-features --all-targets`.
- Run `cargo clippy --all-features --all-targets`.
- Run `cargo test` (no live-tests).
- Rename the spec file to start with `_` when complete.

## Project Layout (Short)

- `t-koma-core`: Shared types, config, WebSocket message schema.
- `t-koma-db`: SQLite layer. Split into T-KOMA DB and per-ghost DBs.
- `t-koma-gateway`: T-KOMA server, models, tools, and transport handlers.
- `t-koma-cli`: TUI + management CLI.

## Database Notes

- T-KOMA DB: operators, ghosts, interfaces. Stored at platform data dir.
- Ghost DB: sessions and messages. Stored at
  `.../t-koma/ghosts/{name}/db.sqlite3`.
- Ghost workspace/safe house CWD is the same folder as its DB.
- Reference schemas: `t-koma-db/schema.sql`, `t-koma-db/ghost_schema.sql`.

Key types:

- `KomaDbPool`, `GhostDbPool`
- `OperatorRepository`, `GhostRepository`, `InterfaceRepository`,
  `SessionRepository`
- `OperatorStatus`, `Platform`, `ContentBlock`

Test helpers:

- `t_koma_db::test_helpers::create_test_koma_pool()`
- `t_koma_db::test_helpers::create_test_ghost_pool(ghost_name)`

## Architecture Rule

Transport layers (Discord, WebSocket) do NOT manage tools. They call
`SessionChat.chat()` via `AppState`. Keep tool logic in
`t-koma-gateway/src/session.rs`.

## Testing Rules (Short)

- Snapshot tests use `insta`.
- AI agents must NEVER accept or update snapshots.
- Live tests (`--features live-tests`) are human-only.

Full examples live in:

- `vibe/knowledge/testing.md`

## Code Style (Short)

- Rust 2024 edition, MSRV 1.85+
- Error handling via `thiserror` and `?`
- Async runtime: `tokio`
- Logging: `tracing`
- Public APIs must have `///` docs

## Security Reminders

- Never commit API keys. Use `.env` and config.
- Discord bot token follows same rule.
- WebSocket is `ws://` only; use TLS termination for production.

## Common Tasks (Pointer Only)

Detailed how-tos are in `vibe/knowledge/`:

- Adding providers: `vibe/knowledge/providers.md`
- Adding tools: `vibe/knowledge/tools.md`
- Testing patterns: `vibe/knowledge/testing.md`
- Skills system: `vibe/knowledge/skills.md`
- Anthropic/OpenRouter specifics: `vibe/knowledge/anthropic_claude_api.md`,
  `vibe/knowledge/openrouter.md`
- sqlite-vec notes: `vibe/knowledge/sqlite-vec.md`

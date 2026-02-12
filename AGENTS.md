# t-koma AGENTS.md

This file is the high-signal baseline for all agent runs. Feature-specific
implementation details live in `docs/dev/`.

`CLAUDE.md` is symlinked to this file.

## CRUCIAL

Always update this file when:

- project-wide core rules change
- information here is stale
- an important assumption documented here is proven wrong

## Workspace and Flow

- Work in the currently opened workspace/worktree only.
- Do not jump to repo roots outside the opened workspace.
- Prefer small, atomic commits with conventional commit messages. Commit your changes
  step by step.

## Core Concepts

- T-KOMA: deterministic gateway service
- Operator: approved end user
- Interface: messaging endpoint for an operator (Discord, TUI/API)
- Ghost: agent with its own DB/workspace
- Session: chat thread between an operator and a ghost
- Heartbeat: background session health check; transcripts go to `job_logs`
- Reflection: background knowledge curation run after heartbeat ticks; writes to
  knowledge stores and `job_logs`

## MCP Usage (Core)

- Use MCPs heavily when available:
  - `rust-analyzer-mcp` for refactors and code actions
  - `context7` for up-to-date library documentation
  - `gh` MCP for GitHub/PR workflows
- Prefer MCP-backed answers over assumptions for library/framework behavior.

## Code Quality Rules

- Keep modules focused and discoverable; split large features across well-named files.
- Avoid deep nesting: if logic passes ~4 indentation levels, extract functions.
- Avoid `mod.rs` as implementation-heavy files; use them as barrel modules.
- Prefer expressive code over comments; add comments only when they clarify non-obvious
  behavior.
- Public APIs must have `///` docs.

## Architecture Guardrails

- Transport layers (`discord`, WebSocket server) must not implement chat/tool
  orchestration.
- Interactive conversations must go through `SessionChat` in
  `t-koma-gateway/src/session.rs`.
- Keep provider-neutral history types in `t-koma-gateway/src/chat/history.rs`.
- Keep provider adapters in `t-koma-gateway/src/providers/`.
- Keep tool implementations and tool manager wiring in `t-koma-gateway/src/tools/`.
- Keep transport-agnostic operator flow in `t-koma-gateway/src/operator_flow.rs`.
- Keep scheduler state centralized in `t-koma-gateway/src/scheduler.rs`.
- No endpoint may bypass normal chat orchestration for interactive chat.
- Workspace boundary checks must remain canonicalization-aware (symlink-safe).

## Testing and Verification

Run these whenever realistic for your change:

1. `just check`
2. `just clippy`
3. `just test` (no live tests)
4. `just fmt`

Rules:

- Snapshot tests use `insta`; AI agents must not accept/update snapshots.
- Live tests (`--features live-tests`) are human-run only.

## Security

- Never commit API keys or tokens.
- Use env vars / `.env` for secrets.

## Developer Specs

Use these detailed guides for feature-specific work:

- `docs/dev/README.md`
- `docs/dev/add-provider.md`
- `docs/dev/add-interface.md`
- `docs/dev/add-tool.md`
- `docs/dev/prompts-and-messages.md`
- `docs/dev/background-jobs.md`
- `docs/dev/knowledge-system.md`

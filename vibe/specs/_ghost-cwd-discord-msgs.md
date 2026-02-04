# Spec: Discord Formatting, Deterministic Messages Module, Ghost CWD, Japanese Names

## Goals

- Render deterministic gateway messages in Discord with better visual formatting
  (prefer code blocks with syntax highlighting if appropriate) to preserve
  whitespace/alignment.
- Centralize all deterministic t-koma (gateway) message strings in one module
  for easier edits.
- Allow ghost names to include kanji and katakana characters (if feasible).
- Ensure each ghost runs tools in its own workspace directory (same as ghost DB
  folder), including shell tool; persist per-ghost cwd and allow changing it via
  tool.
- Keep ghosts inside their workspace by default; leaving requires explicit
  operator approval.

## Non-Goals

- No changes to model output formatting.
- No changes to live tests.

## Behavior Changes

1. Discord output: deterministic gateway messages are rendered with a Discord
   code block (syntax-highlighted with a readable language) to preserve
   whitespace and improve readability.
2. Deterministic messages: all gateway deterministic message strings are moved
   to a dedicated module and referenced from call sites.
3. Ghost name validation: accept kanji and katakana in addition to existing
   allowed characters; reject others as before.
4. Tool execution cwd: local tools run with the ghost’s workspace directory as
   CWD; CWD is stored per ghost and defaults to the ghost DB folder if not set.
5. CWD updates: add a tool for changing the working directory; shell tool may
   update cwd when user explicitly requests it. The ghost may not leave its
   workspace without operator approval.

## Implementation Notes

- Add a new module (e.g., `t-koma-gateway/src/deterministic_messages.rs`) and
  re-export constants or functions.
- Update Discord transport formatting to apply to deterministic messages only
  (non-model output).
- Update ghost name validation to include:
  - Kanji: Unicode Han script
  - Katakana: Unicode Katakana block
- Add database schema support for per-ghost cwd (e.g., `ghosts.cwd`) with
  migration and repository updates.
- Update tools to use ghost cwd for local execution. Track cwd per ghost and
  enforce workspace boundaries.
- Add a new tool for directory changes and include operator approval requirement
  for leaving the workspace.

## Tests

- Update or add unit tests for ghost name validation to include kanji/katakana.
- Add tests for deterministic message formatting (if any) to ensure module
  references compile.
- Add tests for cwd persistence and boundary enforcement; if direct tool tests
  are not available, use repository/unit tests.

## Open Questions

- Choose the Discord code block language that provides readable coloring for
  deterministic messages.
- Confirm which tools qualify as "local" and must honor ghost cwd (shell + any
  file tools).

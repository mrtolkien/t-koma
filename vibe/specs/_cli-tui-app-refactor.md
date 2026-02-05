# CLI/TUI App Refactor Spec

## Goal

Split the large CLI and TUI app modules into smaller, more maintainable files,
and eliminate the duplicate `app.rs` naming collision by renaming and
reorganizing modules.

## Scope

- Refactor `t-koma-cli/src/app.rs` into smaller modules with clearer
  responsibilities.
- Refactor `t-koma-cli/src/tui/app.rs` into separate modules for:
  - state/types
  - rendering
  - input handling
  - domain actions (DB/config/gateway/logs)
  - utility/helpers
- Rename files to avoid two `app.rs` modules.
- Update module wiring (`mod.rs`, `main.rs`, and imports) accordingly.

## Constraints

- Preserve existing behavior and UI output.
- Keep public API surface minimal and clear.
- Do not change runtime behavior, only structure and naming.

## Proposed Module Layout

### CLI (non-TUI)

- `t-koma-cli/src/cli_app.rs` (formerly `app.rs`): struct + run loop
- `t-koma-cli/src/cli_events.rs`: event handling + key handling + WS dispatch
- `t-koma-cli/src/cli_ws.rs`: connect + send message helpers

### TUI

- `t-koma-cli/src/tui/app/mod.rs`: `TuiApp` struct + constructor + main loop
- `t-koma-cli/src/tui/app/state.rs`: enums/structs used by app (OperatorView,
  PromptKind, PromptState, GateRow, GateEvent, Metrics)
- `t-koma-cli/src/tui/app/render.rs`: draw\_\* fns
- `t-koma-cli/src/tui/app/input.rs`: key handling, navigation, prompt handling
- `t-koma-cli/src/tui/app/actions.rs`: DB/config/gateway/log operations
- `t-koma-cli/src/tui/app/util.rs`: pure helper fns (colors, marquee, text
  parsing, truncate, ws_url_for_cli, shell_quote)

## Acceptance Criteria

- `cargo check` passes.
- No `app.rs` file name collision between CLI and TUI.
- File structure makes the major responsibilities obvious.

## Out of Scope

- Behavior changes.
- UI redesigns.
- New features.

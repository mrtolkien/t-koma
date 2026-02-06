# Standard Operator Workspace Escape Policy

## Summary
Standard operators should be blocked from `change_directory` outside their workspace by default. Add a per-operator flag to allow workspace escape when explicitly enabled. Expose this flag in the TUI operator management UI.

## Goals
- Persist a per-operator setting that permits `change_directory` outside workspace.
- Default: Standard operators cannot escape; Puppet Masters can.
- Enforce in the `change_directory` tool.
- Allow toggling in TUI.

## Non-Goals
- Changing other tool boundary checks.
- Altering approval flow for workspace escape (this is separate from operator permission).

## Data Model
- Add `allow_workspace_escape` boolean (0/1) to `operators`.
- Default `0` for Standard operators.
- Puppet Master treated as always allowed regardless of flag.

## Enforcement
- In `t-koma-gateway/src/tools/change_directory.rs`, deny when:
  - operator is Standard AND flag is false AND target is outside workspace.
- Keep existing approval flow and symlink-safe checks.

## TUI
- Operators list shows workspace escape status.
- Operator actions include toggle for workspace escape.

## Tests
- DB: update / read allow flag.
- Tool: change_directory outside workspace is blocked for standard (flag false), allowed when true.


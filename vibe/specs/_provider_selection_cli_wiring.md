# Spec: Wire Provider Selection Into CLI Config

## Goal

Make `t-koma-cli`'s provider/model selection actually update and reflect the CLI
configuration so users can manage provider defaults efficiently from the CLI.

## Why

Right now the interactive provider selection only affects the current WebSocket
session. It does not update the persisted config
(`~/.config/t-koma/config.toml`), so the choice is lost and the CLI doesn’t feel
like a configuration tool.

## Scope

- Add a CLI flow to read and update `Settings` provider config (default
  provider + model).
- Use the existing provider selection UI to set defaults.
- Ensure the selection is persisted to the TOML config file.
- Show current defaults before prompting, so users can see what’s already
  configured.

## Non-Goals

- No changes to gateway provider selection behavior beyond current per-session
  selection.
- No changes to secrets (API keys).

## Proposed Changes

- Add a new menu option to the CLI main menu: `Manage provider config`.
- Implement a config update flow in `t-koma-cli` that:
  - Loads `t_koma_core::Settings`.
  - Shows current default provider + model.
  - Lets the user choose provider/model (using existing provider selection flow)
    and then writes the new defaults to the config TOML.
- Add helper in `t-koma-core::Settings` to save updated settings back to the
  config file (if not already present).
- Ensure provider selection flow can optionally operate without a WebSocket when
  only updating local config (if needed); otherwise reuse existing
  WebSocket-based model listing but persist the choice locally.

## UX Notes

- If OpenRouter is selected but API key is missing, warn and let the user
  proceed (they can set the key later).
- If the gateway isn’t running, allow config-only update without requiring a
  live WebSocket connection.

## Tests

- Unit test for settings save/load roundtrip (t-koma-core).
- Manual check: run CLI, choose “Manage provider config”, update provider/model,
  confirm `~/.config/t-koma/config.toml` changes.

## Open Questions

- Should we prefer a config-only selection path (no gateway) even when gateway
  is available, or keep using gateway for model lists?

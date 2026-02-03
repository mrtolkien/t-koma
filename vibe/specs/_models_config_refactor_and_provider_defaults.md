# Spec: Models Config Refactor + Remove Hardcoded Provider Defaults

## Goals

1. Make CLI chat usable without approval (bypass approval for local CLI users).
2. Remove any hardcoded references to Claude/default provider in runtime
   behavior; always use config.
3. Replace `provider` config section with `models` + `default_model`, using
   alias-based tables for readability.
4. Ensure live tests use default provider/model from config (no assumptions).

## Why

- Current CLI WebSocket flow requires approval, blocking local use.
- Gateway still picks Claude in some paths; defaults must come from config only.
- Config should allow multiple models per provider.
- Tests must respect config defaults.

## Scope

- Config schema change: `models` list + `default_model` selector.
- Update all config loading/usage to new schema.
- Update gateway provider selection and default model logic to use config.
- Adjust CLI user platform and approval handling for CLI users.
- Update tests and docs.

## Non-Goals

- No changes to external API behavior beyond config usage.
- No OpenRouter API or Anthropic API client changes beyond model selection
  input.

## Proposed Config Schema

```toml
# Example (alias-based tables for readability)
default_model = "kimi25"

[models]
[models.kimi25]
provider = "openrouter"
model = "moonshotai/kimi-k2.5"

[models.claude]
provider = "anthropic"
model = "claude-sonnet-4-5-20250929"
```

Notes:
- `default_model` is an alias string that must exist in `[models]`.
- Aliases are the table names under `[models]` (e.g., `kimi25`, `claude`).

## Proposed Changes

- `t-koma-core/src/config/settings.rs`
  - Replace `ProviderSettings` with `ModelsSettings` (alias map + default
    alias).
  - Update defaults and TOML serialization.
  - No migration logic (dev stage).
- `t-koma-core/src/config/mod.rs`
  - Update helper accessors to use `settings.models`.
- `t-koma-gateway`
  - Ensure `default_provider` and model are derived from config `default_model`.
  - Ensure provider selection is not hardcoded.
  - Live tests should read config defaults.
- `t-koma-cli`
  - CLI user should be treated as `Platform::Cli` (auto-approved) for gateway
    approval.
  - Provider selection UI should operate on `models` alias list.
- `t-koma-db`
  - If approval logic uses platform, make sure CLI platform is auto-approved (or
    exempt in gateway).

## Tests

- Update config load/save tests for new schema.
- Update live tests to use config default model.

## Decisions

- No migration for old config format (dev stage).
- CLI approval bypass is platform-based (`Platform::Cli`).

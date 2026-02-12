# Add a Provider

This guide is for adding a new LLM provider integration (not just a new model alias).

## Scope and Ownership

- Provider adapters live in `t-koma-gateway/src/providers/`.
- Shared provider interface lives in `t-koma-gateway/src/providers/provider.rs`.
- Provider type enum and config parsing live in `t-koma-core`.

## Implementation Checklist

1. Add provider type to shared enums and parsing.
   - Update `t-koma-core/src/message.rs` (`ProviderType` + `FromStr` + `as_str`).
   - Update any provider selection UIs if this provider should be selectable.

2. Add config semantics and validation.
   - Update `t-koma-core/src/config/settings.rs` if new model fields are needed.
   - Update `t-koma-core/src/config/mod.rs` validation:
     - required credentials/settings
     - provider-specific constraints
     - API key resolution path (`api_key_for_alias` behavior)
   - Add/adjust tests in `t-koma-core/src/config/mod.rs`.

3. Implement provider client module.
   - Create `t-koma-gateway/src/providers/<provider>/client.rs`.
   - Implement `Provider` trait from `t-koma-gateway/src/providers/provider.rs`.
   - Convert provider wire format to `ProviderResponse` / `ProviderContentBlock`.
   - Ensure tool use + tool result round-trip works through unified blocks.

4. Register module exports.
   - Update `t-koma-gateway/src/providers/mod.rs`.
   - Update `t-koma-gateway/src/lib.rs` exports if needed.

5. Wire provider construction in gateway startup.
   - Update `t-koma-gateway/src/main.rs` model loop to instantiate client for new
     provider.
   - Respect existing logging/dump query wiring patterns.

6. Add TUI onboarding and selection flow.
   - There needs to be a simple, interactive TUI for adding this provider, minimizing
     user error.

7. Validate usage logging compatibility.
   - Ensure usage fields map cleanly to `ProviderUsage` (input/output/cache fields when
     available).

## Non-Negotiable Rules

- Keep provider-specific wire types inside provider modules.
- Do not leak provider formats into DB/core models.
- Prompt/render/history shared types stay provider-neutral.

## Validation

Run:

1. `just check`
2. `just clippy`
3. `just test`
4. `just fmt`

If you touched knowledge indexing/tooling as part of provider work, also run relevant
knowledge tests.

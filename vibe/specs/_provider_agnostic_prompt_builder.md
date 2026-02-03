# Spec: Provider-Agnostic Prompt Builder + Docs Cleanup

## Goal

Make prompt building provider-agnostic (or provider-specific via a trait) and
remove Claude-specific wording from comments and markdown so the project reads
as multi-provider.

## Scope

- Introduce a provider-agnostic prompt builder API used by `SessionChat` and
  tests.
- Implement provider-specific prompt builders for existing providers.
- Update code comments and markdown docs to use provider-neutral wording.

## Non-Goals

- Changing provider HTTP clients behavior.
- Modifying snapshot contents or running live tests.

## Plan

1. Inspect current prompt/history building usage and OpenRouter client
   expectations.
2. Add a prompt builder trait (or equivalent abstraction) and implement for
   Anthropic and OpenRouter.
3. Update `SessionChat` + tests to use the provider-agnostic prompt builder.
4. Update comments and markdown files to provider-neutral language.
5. Run `cargo check`, `cargo clippy`, `cargo test`.

## Validation

- `cargo check --all-features --all-targets`
- `cargo clippy --all-features --all-targets`
- `cargo test`

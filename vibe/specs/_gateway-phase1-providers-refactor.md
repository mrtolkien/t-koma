# Spec: Gateway Phase-1 Providers Refactor

## Scope

- `t-koma-gateway` only.
- Rename module namespace `models` to `providers`.
- Move prompt rendering API (`SystemBlock`, `build_system_prompt`, etc.) from
  `models/prompt.rs` into `prompt/render.rs`.
- Introduce provider-neutral chat history module and use it in provider trait,
  session flow, and clients.
- Keep behavior changes minimal and preserve existing tests.

## Planned Changes

1. Move `src/models/*` to `src/providers/*` and update all gateway imports and
   test references.
2. Add `src/prompt/render.rs`; re-export from `prompt/mod.rs`.
3. Add `src/chat/history.rs` with neutral `ChatMessage`/`ChatContentBlock`
   structures and builders from DB messages.
4. Update `Provider::send_conversation` history parameter from Anthropic
   `ApiMessage` to neutral `ChatMessage`.
5. Adapt Anthropic client internally from neutral history to Anthropic API
   structs.
6. Adapt OpenRouter client from neutral history directly, removing Anthropic
   history imports.
7. Run `cargo test -p t-koma-gateway`.

## Validation

- Gateway builds and tests pass.
- No remaining `crate::models::*` imports in `t-koma-gateway`.
- OpenRouter compiles without importing Anthropic history types.

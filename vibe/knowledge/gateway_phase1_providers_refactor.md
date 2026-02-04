# Gateway Phase-1: Providers + Neutral History Refactor

Date: 2026-02-04

## Summary

Refactor completed in `t-koma-gateway`:

- Namespace rename: `src/models/*` -> `src/providers/*`
- Prompt rendering moved from provider namespace to `src/prompt/render.rs`
- Provider-neutral history introduced at `src/chat/history.rs`
- Provider trait now accepts neutral `ChatMessage` history

## Key Design Notes

- `Provider::send_conversation` history parameter is now
  `Vec<chat::history::ChatMessage>`.
- Anthropic adapter converts neutral history to Anthropic wire payload via
  `providers/anthropic/history.rs::to_anthropic_messages(...)`.
- OpenRouter adapter now converts directly from neutral history and does not
  import Anthropic history types.
- `chat/history.rs` retains backward-compatible aliases (`ApiMessage`,
  `ApiContentBlock`, `build_api_messages`) for live tests and transitional code.

## Prompt Rendering Location

- Canonical render API is now under `prompt/render.rs`:
  - `SystemBlock`
  - `build_system_prompt`
  - `build_simple_system_prompt`


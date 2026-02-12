# Prompts and Messages

Use this guide when adding/updating system prompts, content templates, or message IDs.

## Prompts

- System prompts live under `prompts/system/`.
- Keep operator chat prompt in one file:
  - `prompts/system/system-prompt.md`
- Keep reflection prompt in one file:
  - `prompts/system/reflection-prompt.md`
- Template variables used in prompt body must be listed in front matter `vars = [...]`.

## Message Content

- Add localized messages in `t-koma-gateway/messages/en/*.toml`.
- Each message entry should include stable `id` (TOML table key) and message fields
  (`body`, optional `title`, `vars`, `kind`, `actions`).
- Use `{{var}}` placeholders for template variables.

## ID Wiring

- After adding/changing content IDs, update:
  - `t-koma-gateway/src/content/ids.rs`
- Verify call sites use semantic gateway message payloads (not raw hardcoded strings
  when content entries exist).

## Rendering Rules

- Outbound gateway responses should use semantic `GatewayMessage`.
- Every interactive message must preserve plaintext fallback via `text_fallback`.

## Validation

Run:

1. `just check`
2. `just clippy`
3. `just test`
4. `just fmt`

Manually verify at least one flow that uses changed prompts/messages to ensure templates
render as expected.

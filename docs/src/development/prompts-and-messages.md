# Prompts and Messages

This guide covers adding or updating system prompts, content templates, and message IDs.

## Prompts

System prompts live under `prompts/system/`:

- `prompts/system/system-prompt.md` — OPERATOR chat prompt
- `prompts/system/reflection-prompt.md` — reflection prompt

Template variables used in prompt body must be listed in front matter `vars = [...]`.

## Message Content

Localized messages live in `t-koma-gateway/messages/en/*.toml`. Each message entry
includes:

- `id` — stable TOML table key
- `body` — message body text
- `title` — optional title
- `vars` — template variable list
- `kind` — message kind
- `actions` — optional action buttons

Use `{{var}}` placeholders for template variables.

## ID Wiring

After adding/changing content IDs, update `t-koma-gateway/src/content/ids.rs`. Verify
call sites use semantic gateway message payloads (not raw hardcoded strings when content
entries exist).

## Rendering Rules

- Outbound gateway responses use semantic `GatewayMessage`.
- Every interactive message must preserve plaintext fallback via `text_fallback`.

## Validation

```bash
just check
just clippy
just test
just fmt
```

Manually verify at least one flow that uses changed prompts/messages to ensure templates
render as expected.

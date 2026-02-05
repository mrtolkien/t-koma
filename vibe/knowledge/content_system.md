# Gateway Content System

This document describes the gateway content system for user-facing messages and prompts.

## Locations
- `t-koma-gateway/messages/en/`: gateway user-facing messages (TOML bundles)
- `t-koma-gateway/prompts/`: gateway prompts (Markdown + TOML front matter)

These folders are intentionally easy to locate with Telescope.

## Naming
- Kebab-case IDs and filenames.
- Prompt filename matches `id`.
- Message IDs are TOML table names within a bundle file.
- Interface or provider overrides (future) use `@suffix`, e.g. `ghost-created@discord.md`.

## Messages (TOML bundles)
Message bundles are grouped by domain (discord/server/ghosts/etc) and locale.
Each message is a table keyed by ID.

Example:
```
[ghost-created-header]
body = '''
┌───────────────────────────────┐
│ GHOST (ゴースト) ONLINE        │
└───────────────────────────────┘
'''
```

Optional fields per message:
- `scope`: `shared` or `interface:<name>` or `provider:<name>` (defaults to `shared`)
- `vars`: list of template variables (defaults to `[]`)
- `title`, `body`, `actions`, `presentation`

Template variables use `{{var}}` syntax.

## Prompts (Markdown + TOML front matter)
Front matter uses `+++` and TOML fields:
- `id`
- `scope` (optional, defaults to `shared`)
- `role`
- `vars` (optional)
- `inputs` (optional)

Body is the prompt template with `{{var}}` placeholders.

## Build-Time Validation
The gateway `build.rs` validates:
- prompt `id` matches filename
- `scope` matches filename suffix (if used)
- `vars` declared and used in templates
- variable names are lower_snake_case

## Runtime Access
Use the content registry helpers:
- `crate::content::message_text(id, interface, vars)`
- `crate::content::prompt_text(id, provider, vars)`

Example usage:
```
// content: messages/en/generic.toml#error-generic
let text = content::message_text("error-generic", None, &[])?;
```

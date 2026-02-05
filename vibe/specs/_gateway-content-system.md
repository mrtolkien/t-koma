# Gateway Content System Spec

## Goal
Create a clean, maintainable system for gateway user-facing content and prompts with:
- Content vs presentation separation for multi-interface rendering.
- Clear scope rules (shared vs interface/provider specific).
- Consistent discoverable file locations inside each crate.
- Easy usage discovery via filename references.
- Build-time validation of template variables (Option 2).
- Readable multiline content with column-0 message bodies.

## Scope
- Gateway messages (currently in `t-koma-gateway/src/deterministic_messages.rs`).
- Gateway prompts (currently in `default-prompts/` and `t-koma-gateway/src/prompt/base.rs`).
- No changes to CLI/TUI strings.
- No changes to `t-koma-core` content ownership.

## Directory Layout (Locality + Discoverability)
All content lives in the owning crate, in predictable folders:
- `t-koma-gateway/messages/` for gateway user-facing messages (TOML).
- `t-koma-gateway/prompts/` for gateway prompts (Markdown + TOML front matter).

These folders are intended to be easy to find with Telescope via `messages/` or `prompts/`.

## Naming Conventions
- Each file name matches its content `id`.
- Use kebab-case for ids and filenames.
- Base/shared content uses bare id:
  - `messages/ghost-created.toml`
  - `prompts/ghost-session.md`
- Interface or provider overrides use suffixes:
  - `messages/ghost-created@discord.toml`
  - `prompts/system@anthropic.md`

## Content Model
### Gateway Messages (TOML)
Format: TOML with explicit separation between content and presentation.

```
id = "ghost-created"
# scope defaults to "shared"
# vars defaults to []

[content]
body = '''
Ghost @{{ghost_name}} is ready.
'''

[presentation.discord]
embed = { color = "#00c8b4" }
```

- `content` is interface-agnostic.
- `presentation` is optional and keyed by interface (e.g., `discord`).
- `vars` is optional; when present it must list all template variables.

### Prompts (Markdown + TOML front matter)
Format: Markdown with TOML front matter delimited by `+++`.

```
+++
id = "ghost-session"
# scope defaults to "shared"
vars = ["ghost_name", "operator_id"]
role = "system"
inputs = { ghost_name = "string", operator_id = "string" }
+++
You are the ghost {{ghost_name}}. The operator is {{operator_id}}.
```

- `vars` is optional, but required if the template uses variables.

## Scope Rules
- `scope: shared` means default for all interfaces/providers.
- Interface-specific override file uses `@interface` suffix.
- Provider-specific override file uses `@provider` suffix.
- Resolution order:
  1. Exact override for interface/provider
  2. Shared base

## Template Validation (Build-Time)
Implement Option 2 validation:
- `vars` lists are optional; missing implies no template variables.
- Build script extracts all `{{var}}` occurrences.
- Build fails if:
  - A referenced var is not declared in `vars`.
  - A declared var is unused.
  - A var name violates naming rules (lower_snake_case).

## Usage & Traceability
- Every message/prompt usage must reference its content ID and file path in code comments.
- Example:
  ```
  // content: messages/ghost-created.toml
  let msg = registry.message("ghost-created")?;
  ```
- Search by filename should lead to all usage sites.

## Implementation Outline
1. Add `t-koma-gateway/messages/` and `t-koma-gateway/prompts/` directories.
2. Define content structs and loaders in `t-koma-gateway/src/content/`.
3. Add `build.rs` to validate templates against `vars`.
4. Migrate existing messages and prompts into new files.
5. Replace existing message/prompt usage with registry lookups by ID.
6. Add tests that validate all content files and resolution rules.

## Acceptance Criteria
- All gateway messages and prompts live in the new folders.
- Gateway code uses registry lookups by ID.
- Content/presentation separation enforced in message structs.
- Build-time validation of template variables via `vars` lists.
- No content stored in `t-koma-core` for this change.
- Tests validate format and resolution behavior.

## Out of Scope
- CLI/TUI text externalization.
- Provider-specific prompt refactors beyond file relocation.
- New UI or runtime behavior changes.

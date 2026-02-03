# T-KOMA/GHOST/OPERATOR Refactor Spec

## Summary

Refactor naming and data model:

- gateway -> T-KOMA (ティーコマ)
- user -> OPERATOR (オペレータ)
- agent -> GHOST (ゴースト)
- OPERATOR owns multiple GHOSTS
- OPERATOR chats with a GHOST via one or many SESSIONS

Introduce multiple SQLite databases:

- T-KOMA DB: stores operators, approvals, ghost registry
- One DB per ghost: stores sessions/messages for that ghost

Reset migrations as needed to align with new model.

## Goals

- Update naming across crates (types, modules, endpoints, UI/CLI messages,
  config) to T-KOMA/GHOST/OPERATOR/SESSION terminology, and include katakana
  secondary names: ティーコマ/ゴースト/オペレータ.
- Split DB responsibilities:
  - T-KOMA DB: operators, approvals, ghost registry
  - GHOST DB: sessions/messages for that ghost
- Implement flow changes:
  - OPERATOR approval is done via management CLI
  - GHOST creation is initiated via chat (Discord) with a machine-like prompt
    requesting the name
  - The GHOST sends the first message in the session
- Keep deterministic, non-AI logic in T-KOMA; AI runs in GHOST runtime (future).
- WebSocket connections should refer to the operator as the Puppet Master.
- Operators can have multiple interfaces (Discord, TUI).
- First interaction on an interface must ask if it belongs to an existing or
  new operator.
- Existing-operator flow is not implemented yet (send message + TODO).
- First-time interaction messaging must ask the user to trust the Puppet Master
  before creating a GHOST, noting the Puppet Master has full access to GHOST
  info.
- Add `default-prompts/BOOTSTRAP.md` in repo root; its contents are passed as the first user
  message to the GHOST.
- Update active documentation to reflect new concepts.

## Non-Goals (for now)

- Implement GHOST runtime or AI orchestration.
- Multi-tenant hosting or external API changes beyond naming/flow.

## Terminology Mapping

- gateway -> T-KOMA (ティーコマ)
- user -> OPERATOR (オペレータ)
- session/chat -> SESSION
- agent/tool loop -> GHOST (ゴースト) (future runtime)

## Data Model

### T-KOMA DB

Tables (new/renamed):

- operators
  - id (string)
  - display_name
  - status (Pending/Approved/Denied)
  - platform (Discord/Api/Cli)
  - created_at
- ghosts
  - id (string)
  - name (unique within a T-KOMA)
  - owner_operator_id (FK operators.id)
  - created_at
- interfaces
  - id (string)
  - operator_id (FK operators.id)
  - platform (Discord/Api/Cli)
  - external_id (platform user id)
  - display_name
  - created_at
- operator_ghosts (optional if many-to-many later; for now one-to-many via
  owner_operator_id)

### GHOST DB (per ghost)

Tables:

- sessions
  - id
  - operator_id
  - name
  - created_at
- messages
  - id
  - session_id
  - role
  - content
  - created_at
  - blocks (if using content blocks)

## Storage Layout

- T-KOMA DB location stays in platform data dir (new filename `koma.sqlite3`).
- Each ghost has its own folder: `t-koma/ghosts/{ghost_name}/`
  - `db.sqlite3` inside that folder
  - The ghost's workspace/safe house CWD is this same folder

## Code Touchpoints

- `t-koma-db`
  - rename User -> Operator, Session -> Session (same), Message -> Message
  - add GhostRepository and ghost schema
  - split DB pools or add new `KomaDbPool` + `GhostDbPool`
  - update migrations, reset files
- `t-koma-gateway`
  - `AppState` to hold T-KOMA DB pool + Ghost DB access (by ghost name)
  - server/discord flows: OPERATOR approval + GHOST selection/creation
  - WebSocket + HTTP routes should reference T-KOMA terminology
  - ToolManager/SessionChat likely to operate within a ghost context
- `t-koma-cli`
  - approval flow (management CLI)
  - interface binding prompt for TUI
  - ghost creation happens in chat (Discord)
  - update messaging/UI text
  - update any CLI prompts and command outputs
- `t-koma-core`
  - config types, message enums, any references to user/gateway -> operator/koma
  - update system prompts referencing gateway/user
- Docs
  - README.md, AGENTS.md (if structure changed), and any other active docs in
    `vibe/` or `default-prompts/skills/` that mention old terms

## Migration Strategy

- Remove existing migrations, create new initial schema for T-KOMA DB.
- Add ghost DB schema (sessions/messages) as separate migration set or explicit
  SQL bootstrap.
- Update `test_helpers` to create T-KOMA DB and GHOST DB fixtures.

## CLI Flow (Detailed)

1. Operator connects.
2. T-KOMA prompts for approval in management CLI.
3. On first interface interaction, T-KOMA asks if it is NEW/EXISTING operator.
4. In chat (Discord), T-KOMA issues a machine-like prompt for GHOST name.
4. Create GHOST, then start a session with GHOST sending the first message.
5. First-time interaction must warn about trusting the Puppet Master.

## Risks/Notes

- Existing data will be lost due to migration reset.
- Multi-db path handling must be carefully scoped to avoid path traversal.
- Ghost names must be sanitized for folder names.

## Test Plan

- `cargo check --all-features --all-targets`
- `cargo clippy --all-features --all-targets`
- `cargo test`
- Add unit tests for ghost path sanitization and repository ops

## Open Questions

- Ghost names are unique within a T-KOMA (no duplicates).
- Ghost IDs are internal UUIDs (name remains the primary human-facing handle).
- Ghost deletion: not implemented yet.

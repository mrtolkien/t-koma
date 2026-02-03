# Spec: Remove Legacy Pending Users + Adjust CLI Gateway Usage

## Goal

1. Remove legacy pending users storage in `t-koma-core` and rely on DB-backed
   user management.
2. Adjust CLI so it only attempts to auto-start the gateway for chat mode, not
   other modes.
3. Review and improve OpenRouter model fetching behavior to avoid failures when
   the user is not approved or the gateway disconnects.

## Why

- The TOML-based pending users module is obsolete now that SQLite-backed user
  management exists.
- CLI should not start gateway for log/admin/config modes.
- Current OpenRouter model fetch flow fails when the gateway disconnects or user
  is pending; we should fall back cleanly to local config selection and provide
  clear guidance.

## Scope

- Remove `t-koma-core/src/pending_users.rs` and its exports.
- Update docs to reflect DB-backed user management only.
- Update `t-koma-db::UserRepository::prune_pending` to log prune events or
  otherwise capture audit trail.
- Adjust CLI startup flow so gateway auto-spawn is only for chat.
- Make provider config flow robust if gateway is down or user is pending, with a
  clean fallback to local config selection.

## Non-Goals

- No changes to gateway authentication policy.
- No changes to approval workflow logic.

## Proposed Changes

- Remove `pending_users` module from `t-koma-core/src/lib.rs` and delete file.
- Update `AGENTS.md` to remove references to legacy pending users and config
  paths as needed.
- Add event logging for prune operations in `t-koma-db`.
- Move gateway auto-start into `run_chat_mode`, not `main`.
- In provider config flow, if gateway fails or responds with pending approval,
  fallback to local config selection without attempting further WebSocket sends.

## Tests

- Run `cargo test -p t-koma-db` (optional) for user repository changes.
- Manual: launch CLI, choose Manage provider config with gateway down, ensure
  local selection works.

## Open Questions

- For pruning audit, should we log a single summary event or per-user events?

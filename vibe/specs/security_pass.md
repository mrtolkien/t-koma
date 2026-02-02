# Security Pass Specification

## Overview
Enhance t-koma security with user approval system, local-only access, and secure config storage.

## Goals
- Local-only gateway access by default
- Per-service user approval system
- Persistent config in XDG directories
- Admin CLI for user management

## Threat Model

### Assumptions
- Gateway runs on user's local machine
- Discord bot token is sensitive
- Anthropic API key is sensitive
- Not a multi-user system (single owner)

### Risks
- Unauthorized local access to gateway
- Unauthorized Discord bot usage
- Credential exposure in config files

## Configuration Storage

### Location
- **Linux**: `~/.config/t-koma/config.toml`
- **macOS**: `~/Library/Application Support/t-koma/config.toml`
- **Windows**: `%APPDATA%\t-koma\config.toml`

Use `dirs` crate for cross-platform paths.

### Config Format (TOML)
```toml
# t-koma config file
# Do not share this file - contains sensitive tokens

[auth]
# Auto-generate on first run
secret_key = "random-32-byte-hex"

[discord]
token = "..."
enabled = true

[[discord.approved_users]]
id = "123456789"
name = "username#1234"
approved_at = "2026-02-02T10:00:00Z"

[[discord.pending_users]]
id = "987654321"
name = "otheruser#5678"
requested_at = "2026-02-02T11:00:00Z"

[api]
# Localhost-only by default
host = "127.0.0.1"
port = 3000

[[api.approved_users]]
# For HTTP/WebSocket API users
id = "local-user-1"
name = "Local User"
approved_at = "2026-02-02T10:00:00Z"
```

## Security Features

### 1. Local-Only Gateway (DONE - already defaults to 127.0.0.1)
Verify `GATEWAY_HOST` defaults to `127.0.0.1` and document this clearly.

### 2. User Approval System

**Per-Service Approval:**
- `discord`: Discord user IDs
- `api`: For future HTTP auth (currently localhost only)

**Approval States:**
- `pending`: User requested access, waiting for approval
- `approved`: User can use the service
- `denied`: User explicitly rejected

**Discord Flow:**
1. New user messages bot
2. Bot checks if user is approved
3. If not approved → add to pending, send: "Your request is pending approval. The bot owner will review it."
4. If approved → process message normally
5. If denied → send: "Your access request was denied."

### 3. Admin CLI Commands

New CLI mode: `t-koma-cli admin`

```
$ t-koma-cli admin

=== t-koma Admin ===

Pending Discord Users:
  1. otheruser#5678 (ID: 987654321) - requested 2 min ago

Commands:
  approve discord <id>    Approve a Discord user
  deny discord <id>       Deny a Discord user
  list discord            List all Discord users
  list api                List all API users
  help                    Show help
  quit                    Exit

admin> approve discord 987654321
Approved otheruser#5678 for Discord access.
```

### 4. Config File Permissions

On Unix: `chmod 600` (owner read/write only)
On Windows: Set file ACLs appropriately

## Implementation Plan

### Phase 1: Config Module
1. Add `dirs` dependency
2. Create `t-koma-core/src/persistent_config.rs`
3. Implement `PersistentConfig` struct with serde TOML
4. Handle file permissions on save

### Phase 2: User Management
1. Add `User` struct with id, name, status, timestamps
2. Add `UserManager` for CRUD operations
3. Implement approval check logic

### Phase 3: Discord Integration
1. Update Discord handler to check approval
2. Add pending user on first message
3. Send appropriate responses

### Phase 4: Admin CLI
1. Add `admin` mode to CLI
2. Implement interactive commands
3. Display pending users nicely

### Phase 5: Security Hardening
1. Set restrictive file permissions on config save
2. Verify localhost-only binding
3. Add audit logging

## Dependencies

```toml
# t-koma-core
dirs = "6.0"
toml = "0.8"

# For secret generation
rand = "0.8"
```

## Additional Security Considerations

### Audit Logging
- Log all approval/denial actions
- Log Discord messages (content truncated)
- Store in separate log file (not config)

### Input Validation
- Sanitize Discord usernames (max length, chars)
- Validate user IDs are numeric (Discord)

### Rate Limiting (Future)
- Per-user message rate limits
- Configurable per service

### HTTP Auth (Future)
- When non-localhost access needed
- API key or JWT auth

## Migration Notes

- Existing `.env` continues to work
- Config file is additive
- If config exists, use it; else create from `.env` defaults

# Security Pass V2 - Revised Requirements

## Changes from V1

### 1. Admin in Main CLI
- Remove separate `admin` arg mode
- Add "Admin - Manage users" as option 3 in main menu
- Only admin uses CLI anyway

### 2. Interactive Approval Flow
```
=== Pending Discord Users ===

1. @alice (ID: 123456) - 5 min ago
2. @bob (ID: 789012) - 2 min ago

Enter number to approve, 'd <num>' to deny, 'q' to quit: _
```

### 3. Pending Users in Separate File
- **Approved users**: `~/.config/t-koma/config.toml` (persistent)
- **Pending users**: `~/.config/t-koma/pending.toml` (temporary)

### 4. Auto-Prune Pending Users
- Add `created_at` timestamp to pending users
- On read: remove entries older than 1 hour
- Save pruned list back

### 5. Welcome Message on Approval
- When approving user, send Discord DM: "Hello! You now have access to t-koma."
- Use Discord's DM channel

## File Formats

### config.toml (approved users only)
```toml
secret_key = "..."

[discord.users."123456"]
name = "alice"
approved_at = "2026-02-02T10:00:00Z"
```

### pending.toml
```toml
[user."789012"]
name = "bob"
requested_at = "2026-02-02T10:30:00Z"
```

## Implementation Tasks

1. Split `PersistentConfig` into:
   - `ConfigFile` (approved, main config)
   - `PendingFile` (pending, auto-pruned)

2. Add auto-prune on read

3. Update CLI menu:
   ```
   1. Chat with t-koma
   2. Follow gateway logs
   3. Manage users
   ```

4. Interactive approval UI

5. Discord DM on approval (requires serenity context)

# Operator Access Levels & Rate Limits

## Access Levels

Operators have two access levels:

- `puppet_master` (admin): not rate limited.
- `standard`: subject to per-operator rate limits unless disabled.

Access levels are stored in `operators.access_level`.

## Rate Limits

Standard operators can have optional rate limits:

- 5-minute window: `rate_limit_5m_max`
- 1-hour window: `rate_limit_1h_max`

Defaults for Standard operators:

- 10 messages per 5 minutes
- 100 messages per hour

Disable rate limits by setting both fields to `NULL`.

## Enforcement

Rate limiting is enforced in the gateway before chat processing:

- `t-koma-gateway/src/state.rs`: `AppState::check_operator_rate_limit()` maintains in-memory per-operator windows.
- WebSocket + Discord both call the check and return the `rate-limited` message on denial.
- The blocked message is stored as a pending message so the operator can send `continue` after the window clears without retyping.

If you add new operator-facing message entry points (e.g., sub-agents), call the same rate limit check and store the pending message for `continue` retries.

## Workspace Escape

Standard operators are blocked from `change_directory` outside their workspace by default.

- DB column: `operators.allow_workspace_escape` (0/1).
- Puppet Masters are always allowed regardless of the flag.
- TUI exposes a toggle for this flag.

Enforcement is in `t-koma-gateway/src/tools/change_directory.rs`, which denies outside-workspace paths unless the operator is allowed.

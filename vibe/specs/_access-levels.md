# Access Levels + Operator Rate Limits

## Summary
Add operator access levels with a Puppet Master (admin) tier and Standard tier. Standard operators may have per-operator rate limits (5m + 1h windows) with defaults of 10/5m and 100/1h. Allow Standard operators to opt out of rate limits. Enforce limits in gateway request flow with a user-facing retry message that preserves the pending message until the limit clears. All operator management must be available in the TUI.

## Goals
- Persist operator access level and optional rate limits in the database.
- Enforce rate limits for Standard operators at the gateway boundary.
- Provide a clear “rate limited” message and allow retry without re-typing.
- Expose access level and rate limit controls in the TUI operator management UI.
- Document the rate limit policy in a durable knowledge doc for future features (e.g., sub-agents).

## Non-Goals
- Introducing external rate limiting services.
- Rate limiting tool invocations (only operator-originated messages).
- Admin/CLI tooling beyond the TUI if not already present.

## Data Model
- Add `access_level` to `operators` with values: `puppet_master`, `standard`.
- Add optional rate limit fields to `operators`:
  - `rate_limit_5m_max` (nullable int)
  - `rate_limit_1h_max` (nullable int)
- Default for Standard: 10 per 5 minutes, 100 per 1 hour.
- Puppet Master ignores rate limits.
- Standard can explicitly opt out by setting both rate limit fields to null.

## Enforcement
- Implement a per-operator rate limiter in gateway (in-memory or in DB if necessary).
- Apply only to operator messages entering chat (pre-tool loop).
- When rate limited:
  - Respond with a clear rate limit message including remaining wait time.
  - Preserve the pending message so the operator can retry once allowed without retyping.

## TUI
- Operators list shows access level and rate limit summary.
- Operator detail/action panel supports:
  - Set access level (Puppet Master/Standard).
  - Toggle “no rate limits”.
  - Edit rate limit values for 5m and 1h.

## Content
- Add gateway user-facing message for rate limited responses.

## Tests
- DB: CRUD for access level + rate limits.
- Gateway: rate limiting allows messages under limit, blocks over limit, retry succeeds after window.
- TUI: operator management flows update and display values (minimal smoke coverage).

## Open Questions
- Should access level default for new operators be Standard? (assume yes)
- Should limits apply to WebSocket + Discord consistently? (assume yes)

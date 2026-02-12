# Multi-Model Fallback

The multi-model fallback system lets operators configure a chain of models so t-koma
automatically rotates to the next one when rate limits or server errors are hit.

## Config Format

`default_model` and `heartbeat_model` accept a single alias (string) or an ordered list
(array). Both forms are valid:

```toml
# Single model (backwards compatible):
default_model = "kimi25"

# Ordered fallback chain:
default_model = ["kimi25", "gemma3", "qwen3"]
heartbeat_model = ["alpha", "kimi25"]
```

Every alias in the list must exist in `[models]` and pass model validation.

## Architecture

### ModelAliases (`t-koma-core/src/config/settings.rs`)

`ModelAliases(Vec<String>)` with custom serde that accepts `"string"` or `["list"]`.
Methods: `first()`, `iter()`, `is_empty()`, `len()`.

### Circuit Breaker (`t-koma-gateway/src/circuit_breaker.rs`)

Cooldown-based (not classical open/half-open/closed). Shared across all sessions because
rate limits are account-global.

| Failure type                    | Cooldown  |
| ------------------------------- | --------- |
| Rate-limited (429)              | 1 hour    |
| Server error (5xx / overloaded) | 5 minutes |

Key methods:

- `is_available(alias)` — check if cooldown expired
- `record_failure(alias, reason)` — set cooldown
- `record_success(alias)` — clear cooldown
- `first_available(aliases)` — first non-cooled-down alias

### Interactive Chat Fallback (`t-koma-gateway/src/state.rs`)

`AppState::try_chat_with_chain()` iterates the model chain:

1. Skip models on cooldown via circuit breaker.
2. Call `SessionChat::chat()` with the chosen model.
3. On success: `record_success`, return.
4. On retryable `ProviderError`: `record_failure`, set
   `message_already_persisted = true`, advance to next model.
5. On non-retryable error: return immediately (no fallback).
6. If all models fail: return `ChatError::AllModelsExhausted`.

The `message_already_persisted` flag prevents duplicate DB writes when retrying — the
operator message is persisted on the first attempt and skipped on subsequent ones.

### Background Jobs (`t-koma-gateway/src/heartbeat.rs`)

Background jobs don't have an inner fallback loop. Instead:

- At each tick, pick the best available model via
  `circuit_breaker.first_available(chain)`.
- If the tick fails with a retryable error, the circuit breaker records it.
- The next tick naturally picks the next available model.

### Error Classification (`t-koma-gateway/src/providers/provider.rs`)

`ProviderError` has three classification methods:

- `is_rate_limited()` — HTTP 429
- `is_server_error()` — HTTP 5xx or "overloaded" body
- `is_retryable()` — superset: rate-limited + server error + transport failures

Only retryable errors trigger chain fallback. Client errors (400, 404, etc.) are
returned immediately.

## Key Files

- `t-koma-core/src/config/settings.rs` — `ModelAliases` type and serde
- `t-koma-core/src/config/mod.rs` — alias list validation and accessors
- `t-koma-gateway/src/circuit_breaker.rs` — circuit breaker module
- `t-koma-gateway/src/state.rs` — chain resolution and fallback loop
- `t-koma-gateway/src/session.rs` — `ChatError::Provider`, `message_already_persisted`
- `t-koma-gateway/src/heartbeat.rs` — per-tick model selection
- `t-koma-gateway/tests/multi_model_live.rs` — live fallback test

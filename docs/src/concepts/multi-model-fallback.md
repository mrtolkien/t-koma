# Multi-Model Fallback

The multi-model fallback system lets you configure a chain of models so T-KOMA
automatically rotates to the next one when rate limits or server errors are hit.

## Configuration

`default_model` and `heartbeat_model` accept a single alias or an ordered list:

```toml
# Single model (backwards compatible):
default_model = "kimi25"

# Ordered fallback chain:
default_model = ["kimi25", "gemma3", "qwen3"]
heartbeat_model = ["alpha", "kimi25"]
```

Every alias in the list must exist in `[models]` and pass model validation.

## Circuit Breaker

T-KOMA uses a cooldown-based circuit breaker (shared across all sessions, since rate
limits are account-global):

| Failure Type                    | Cooldown  |
| ------------------------------- | --------- |
| Rate-limited (429)              | 1 hour    |
| Server error (5xx / overloaded) | 5 minutes |

The circuit breaker automatically clears cooldowns when they expire, making the model
available again.

## Interactive Chat Fallback

When you send a message, `AppState::try_chat_with_chain()` iterates the model chain:

1. Skip models on cooldown
2. Call the provider with the chosen model
3. On success: record success, return response
4. On retryable error (429, 5xx): record failure, try next model
5. On non-retryable error (400, 404): return immediately (no fallback)
6. If all models fail: return `AllModelsExhausted` error

The OPERATOR's message is persisted on the first attempt and skipped on retries to
prevent duplicates.

## Background Jobs

Background jobs don't have an inner fallback loop. Instead, each tick picks the best
available model via the circuit breaker. If the tick fails with a retryable error, the
next tick naturally picks the next available model.

## Error Classification

Provider errors are classified into three categories:

- **Rate-limited** — HTTP 429
- **Server error** — HTTP 5xx or "overloaded" in response body
- **Retryable** — superset: rate-limited + server error + transport failures

Only retryable errors trigger chain fallback. Client errors (400, 404) are returned
immediately.

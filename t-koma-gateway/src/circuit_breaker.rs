//! Per-model circuit breaker for multi-model fallback chains.
//!
//! When a provider returns a retryable error (rate limit or server error),
//! the circuit breaker puts that model on cooldown so subsequent requests
//! skip it and try the next model in the chain.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Why a model was placed on cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownReason {
    /// HTTP 429 — back off for a long time.
    RateLimited,
    /// 5xx or "overloaded" — shorter cooldown.
    ServerError,
}

impl CooldownReason {
    fn cooldown_duration(self) -> Duration {
        match self {
            Self::RateLimited => Duration::from_secs(60 * 60), // 1 hour
            Self::ServerError => Duration::from_secs(5 * 60),  // 5 minutes
        }
    }
}

struct CooldownEntry {
    available_at: Instant,
    reason: CooldownReason,
}

/// Shared, lock-based circuit breaker tracking per-model cooldowns.
///
/// Thread-safe via `RwLock`; contention is low because writes only happen
/// on failures and reads are fast.
pub struct CircuitBreaker {
    states: RwLock<HashMap<String, CooldownEntry>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with no models on cooldown.
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
        }
    }

    /// Whether `alias` is currently available (not on cooldown or cooldown expired).
    pub fn is_available(&self, alias: &str) -> bool {
        let states = self.states.read().expect("CircuitBreaker lock poisoned");
        match states.get(alias) {
            None => true,
            Some(entry) => Instant::now() >= entry.available_at,
        }
    }

    /// Record a failure for `alias`, placing it on cooldown.
    pub fn record_failure(&self, alias: &str, reason: CooldownReason) {
        let mut states = self.states.write().expect("CircuitBreaker lock poisoned");
        states.insert(
            alias.to_string(),
            CooldownEntry {
                available_at: Instant::now() + reason.cooldown_duration(),
                reason,
            },
        );
    }

    /// Record a success for `alias`, clearing any cooldown.
    pub fn record_success(&self, alias: &str) {
        let mut states = self.states.write().expect("CircuitBreaker lock poisoned");
        states.remove(alias);
    }

    /// Return the first available alias from `aliases`, or `None` if all are on cooldown.
    pub fn first_available<'a>(&self, aliases: &'a [String]) -> Option<&'a str> {
        let states = self.states.read().expect("CircuitBreaker lock poisoned");
        let now = Instant::now();
        aliases.iter().find_map(|alias| {
            let available = match states.get(alias.as_str()) {
                None => true,
                Some(entry) => now >= entry.available_at,
            };
            available.then_some(alias.as_str())
        })
    }

    /// Return the cooldown reason for a model, if it is currently on cooldown.
    pub fn cooldown_reason(&self, alias: &str) -> Option<CooldownReason> {
        let states = self.states.read().expect("CircuitBreaker lock poisoned");
        states.get(alias).and_then(|entry| {
            if Instant::now() < entry.available_at {
                Some(entry.reason)
            } else {
                None
            }
        })
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_model_is_available() {
        let cb = CircuitBreaker::new();
        assert!(cb.is_available("model_a"));
    }

    #[test]
    fn failure_makes_model_unavailable() {
        let cb = CircuitBreaker::new();
        cb.record_failure("model_a", CooldownReason::RateLimited);
        assert!(!cb.is_available("model_a"));
    }

    #[test]
    fn success_clears_cooldown() {
        let cb = CircuitBreaker::new();
        cb.record_failure("model_a", CooldownReason::ServerError);
        assert!(!cb.is_available("model_a"));
        cb.record_success("model_a");
        assert!(cb.is_available("model_a"));
    }

    #[test]
    fn first_available_picks_non_cooled_model() {
        let cb = CircuitBreaker::new();
        let chain = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        assert_eq!(cb.first_available(&chain), Some("a"));

        cb.record_failure("a", CooldownReason::RateLimited);
        assert_eq!(cb.first_available(&chain), Some("b"));

        cb.record_failure("b", CooldownReason::ServerError);
        assert_eq!(cb.first_available(&chain), Some("c"));
    }

    #[test]
    fn all_models_exhausted_returns_none() {
        let cb = CircuitBreaker::new();
        let chain = vec!["a".to_string(), "b".to_string()];

        cb.record_failure("a", CooldownReason::RateLimited);
        cb.record_failure("b", CooldownReason::RateLimited);
        assert_eq!(cb.first_available(&chain), None);
    }

    #[test]
    fn cooldown_reason_reported_correctly() {
        let cb = CircuitBreaker::new();
        assert!(cb.cooldown_reason("a").is_none());

        cb.record_failure("a", CooldownReason::RateLimited);
        assert_eq!(cb.cooldown_reason("a"), Some(CooldownReason::RateLimited));

        cb.record_success("a");
        assert!(cb.cooldown_reason("a").is_none());
    }

    #[test]
    fn different_models_are_independent() {
        let cb = CircuitBreaker::new();
        cb.record_failure("a", CooldownReason::ServerError);
        assert!(!cb.is_available("a"));
        assert!(cb.is_available("b"));
    }
}

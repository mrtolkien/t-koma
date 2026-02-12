//! Live test for multi-model fallback chain (requires --features live-tests).
//!
//! Constructs a [Gemini 2 (deprecated), OpenRouter Kimi-K2.5] chain.
//! Gemini 2 is expected to fail (404 — model removed), which forces the
//! chain to advance to OpenRouter's Kimi-K2.5 as fallback.
//!
//! Run with: cargo test --features live-tests --test multi_model_live

#[cfg(feature = "live-tests")]
mod tests {
    use std::sync::Arc;

    use t_koma_gateway::circuit_breaker::{CircuitBreaker, CooldownReason};
    use t_koma_gateway::providers::Provider;
    use t_koma_gateway::providers::gemini::GeminiClient;
    use t_koma_gateway::providers::openrouter::OpenRouterClient;

    struct ChainEntry {
        alias: String,
        client: Arc<dyn Provider>,
    }

    /// Build a model chain: Gemini 2 (will fail) → OpenRouter Kimi-K2.5.
    ///
    /// Skips providers whose API keys are missing so the test gracefully
    /// degrades in environments with partial credentials.
    fn build_chain() -> Option<Vec<ChainEntry>> {
        t_koma_core::load_dotenv();

        let mut chain: Vec<ChainEntry> = Vec::new();

        // Gemini 2 (deprecated — expected to 404)
        if let Ok(key) = std::env::var("GEMINI_API_KEY")
            && !key.trim().is_empty()
        {
            let client = GeminiClient::new(&key, "gemini-2.0-flash");
            chain.push(ChainEntry {
                alias: "gemini2".into(),
                client: Arc::new(client),
            });
        }

        // OpenRouter Kimi-K2.5 (expected to succeed)
        if let Ok(key) = std::env::var("OPENROUTER_API_KEY")
            && !key.trim().is_empty()
        {
            let client =
                OpenRouterClient::new(&key, "moonshotai/kimi-k2.5", None, None, None, None);
            chain.push(ChainEntry {
                alias: "kimi-k2.5".into(),
                client: Arc::new(client),
            });
        }

        if chain.is_empty() {
            eprintln!(
                "Neither GEMINI_API_KEY nor OPENROUTER_API_KEY set; \
                 skipping multi-model live test."
            );
            return None;
        }

        eprintln!(
            "Multi-model chain: [{}]",
            chain
                .iter()
                .map(|e| e.alias.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Some(chain)
    }

    /// Walk the chain trying each model until one succeeds, recording
    /// failures in the circuit breaker — mirroring `AppState::try_chat_with_chain`.
    #[tokio::test]
    async fn test_multi_model_chain_fallback() {
        let Some(chain) = build_chain() else {
            return;
        };

        let breaker = CircuitBreaker::new();
        let mut succeeded = false;

        for entry in &chain {
            if !breaker.is_available(&entry.alias) {
                eprintln!("'{}': skipped (on cooldown)", entry.alias);
                continue;
            }

            eprintln!("'{}': trying...", entry.alias);
            let response = entry
                .client
                .send_message("Reply with exactly: 'chain ok'")
                .await;

            match response {
                Ok(resp) => {
                    breaker.record_success(&entry.alias);
                    let text = t_koma_gateway::extract_all_text(&resp);
                    eprintln!("'{}': success — {}", entry.alias, text.trim());

                    assert!(
                        !text.trim().is_empty(),
                        "Expected non-empty response from '{}'",
                        entry.alias
                    );
                    succeeded = true;
                    break;
                }
                Err(e) => {
                    let reason = if e.is_rate_limited() {
                        CooldownReason::RateLimited
                    } else {
                        CooldownReason::ServerError
                    };
                    breaker.record_failure(&entry.alias, reason);
                    eprintln!("'{}': failed — {}", entry.alias, e);
                }
            }
        }

        assert!(succeeded, "All models in chain exhausted without success");
        eprintln!("Multi-model fallback test passed");
    }
}

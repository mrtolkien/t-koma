use crate::anthropic::AnthropicClient;

/// Shared application state
pub struct AppState {
    /// Anthropic API client
    pub anthropic: AnthropicClient,
}

impl AppState {
    pub fn new(anthropic: AnthropicClient) -> Self {
        Self { anthropic }
    }
}

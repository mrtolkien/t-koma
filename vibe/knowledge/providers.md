# Provider Integration Guide

Use this file when adding a new model provider to the gateway.

## Steps

1. Create a new module in `t-koma-gateway/src/providers/` (e.g., `my_provider/`).
2. Implement the `Provider` trait.
3. Export the provider in `t-koma-gateway/src/providers/mod.rs`.
4. Initialize the provider in `t-koma-gateway/src/main.rs`.
5. Add provider wiring in `t-koma-core/src/message.rs` and `t-koma-core/src/config/` (secrets/settings/validation).
6. Update tests/helpers that build provider clients (`t-koma-gateway/tests/common/mod.rs`).
7. Update CLI provider selection in `t-koma-cli`.

## Skeleton

```rust
use crate::providers::provider::{Provider, ProviderError, ProviderResponse};

#[derive(Clone)]
pub struct MyProviderClient {
    // fields
}

#[async_trait::async_trait]
impl Provider for MyProviderClient {
    fn name(&self) -> &str { "my_provider" }
    fn model(&self) -> &str { &self.model }

    async fn send_conversation(
        &self,
        system: Option<Vec<SystemBlock>>,
        history: Vec<ApiMessage>,
        tools: Vec<&dyn Tool>,
        new_message: Option<&str>,
        message_limit: Option<usize>,
        tool_choice: Option<String>,
    ) -> Result<ProviderResponse, ProviderError> {
        // Implementation
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}
```

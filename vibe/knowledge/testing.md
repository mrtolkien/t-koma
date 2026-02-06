# Testing Guide

This file contains the detailed testing patterns referenced by `AGENTS.md`.

## Snapshot Testing

We use `insta` for snapshot testing. AI agents must NEVER accept or update
snapshots. If a snapshot test fails, report it and wait for a human review.

## Integration Test Structure

`t-koma-gateway/tests/`

- `snapshot_tests.rs`: module declarations
- `client/`: API client tests
- `conversation/`: full-stack tests (AppState + DB)

## Running Tests

```bash
cargo test
```

Live tests are human-only:

```bash
cargo test --features live-tests
```

## Example: Client Test

```rust
// t-koma-gateway/tests/client/my_feature.rs
#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::models::Provider;
#[cfg(feature = "live-tests")]
use crate::common;

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_my_api_feature() {
    t_koma_core::load_dotenv();
    let default_model = common::load_default_model();

    let response = default_model
        .client
        .send_message("My test prompt")
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "my_feature",
        response,
        {
            ".id" => "[id]"
        }
    );
}
```

## Example: Conversation Test

```rust
// t-koma-gateway/tests/conversation/my_feature.rs
#[cfg(feature = "live-tests")]
use t_koma_db::{GhostDbPool, GhostRepository, OperatorRepository, SessionRepository};
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    models::anthropic::history::build_api_messages,
    models::prompt::build_system_prompt,
    prompt::SystemPrompt,
    tools::{shell::ShellTool, Tool},
};
#[cfg(feature = "live-tests")]
use crate::common;

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_my_conversation_feature() {
    t_koma_core::load_dotenv();

    let koma_db = t_koma_db::test_helpers::create_test_koma_pool()
        .await
        .expect("Failed to create test database pool");

    let operator = OperatorRepository::create_new(
        koma_db.pool(),
        "Test Operator",
        t_koma_db::Platform::Api,
    )
    .await
    .expect("Failed to create operator");

    let operator = OperatorRepository::approve(koma_db.pool(), &operator.id)
        .await
        .expect("Failed to approve operator");

    let ghost = GhostRepository::create(koma_db.pool(), &operator.id, "TestGhost")
        .await
        .expect("Failed to create ghost");

    let ghost_db = GhostDbPool::new(&ghost.name)
        .await
        .expect("Failed to create ghost DB");

    let session = SessionRepository::create(ghost_db.pool(), &operator.id, Some("Test Session"))
        .await
        .expect("Failed to create session");

    let default_model = common::load_default_model();
    let state = common::build_state_with_default_model(koma_db.clone());

    let system_prompt = SystemPrompt::new();
    let system_blocks = build_system_prompt(&system_prompt);
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];
    let model = default_model.model.as_str();

    let user_message = "Hello";
    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text { text: user_message.to_string() }],
        None,
    )
    .await
    .expect("Failed to save message");

    let history = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages = build_api_messages(&history, Some(50));

    let _response = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks,
            api_messages,
            tools,
            Some(user_message),
            model,
        )
        .await
        .expect("Failed to get response");
}
```

## Best Practices

- Redact dynamic fields in snapshots.
- Use the test helpers in `t-koma-db`.
- Verify DB state after operations.
- Log session IDs for debugging.

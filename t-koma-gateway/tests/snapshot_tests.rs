//! Snapshot tests for the Anthropic API.
//!
//! Run with: cargo test --features live-tests
//!
//! These tests capture real API responses (with insta redactions to handle
//! dynamic fields like `id`). Review the `.snap` files to see actual API output.

#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::anthropic::AnthropicClient;

/// Test a simple greeting query - captures the API response structure
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_simple_greeting() {
    t_koma_core::load_dotenv();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set for live tests");

    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");

    let response = client
        .send_message("Say exactly 'Hello from Claude!' and nothing else.")
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "simple_greeting",
        response,
        {
            ".id" => "[id]"
        }
    );
}

/// Test a factual query - shows how the API responds to simple questions
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_factual_query() {
    t_koma_core::load_dotenv();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set for live tests");

    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");

    let response = client
        .send_message("What is 2+2? Answer with just the number.")
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "factual_query",
        response,
        {
            ".id" => "[id]"
        }
    );
}

/// Test a longer response to see the full structure with usage info
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_longer_response() {
    t_koma_core::load_dotenv();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set for live tests");

    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");

    let response = client
        .send_message("List 3 colors. Be concise.")
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "list_response",
        response,
        {
            ".id" => "[id]"
        }
    );
}

//! Basic query tests for the default configured provider.
//!
//! These tests capture real API responses for simple queries.

#[cfg(feature = "live-tests")]
use crate::common;
#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;

/// Test a simple greeting query - captures the API response structure
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_simple_greeting() {
    t_koma_core::load_dotenv();
    let default_model = common::load_default_model();
    let client = default_model.client;

    let response = client
        .send_message("Say exactly 'Hello from the model!' and nothing else.")
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
    let default_model = common::load_default_model();
    let client = default_model.client;

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
async fn test_list_response() {
    t_koma_core::load_dotenv();
    let default_model = common::load_default_model();
    let client = default_model.client;

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

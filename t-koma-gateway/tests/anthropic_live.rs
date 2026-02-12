//! Live tests for Anthropic provider (requires --features live-tests).
//!
//! Run with: cargo test --features live-tests --test anthropic_live

#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::Provider;
#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::anthropic::AnthropicClient;
#[cfg(feature = "live-tests")]
use t_koma_gateway::tools::{Tool, ToolContext, ToolManager};

#[cfg(feature = "live-tests")]
fn load_anthropic_client() -> Option<AnthropicClient> {
    t_koma_core::load_dotenv();

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("ANTHROPIC_API_KEY not set; skipping Anthropic live test.");
            return None;
        }
    };

    Some(AnthropicClient::new(api_key, "claude-sonnet-4-20250514"))
}

#[cfg(feature = "live-tests")]
struct EchoTool;

#[cfg(feature = "live-tests")]
#[async_trait::async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_tool"
    }

    fn description(&self) -> &str {
        "Echoes the provided text back to the caller."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "The text to echo back"}
            },
            "required": ["text"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: &mut ToolContext,
    ) -> Result<String, String> {
        Ok(args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_anthropic_chat_completion() {
    let Some(client) = load_anthropic_client() else {
        return;
    };

    let response = Provider::send_conversation(
        &client,
        None,
        vec![],
        vec![],
        Some("Reply with exactly one short sentence about Rust programming."),
        None,
        None,
    )
    .await
    .expect("Anthropic chat completion failed");

    let text = t_koma_gateway::extract_all_text(&response);
    assert!(
        !text.trim().is_empty(),
        "Expected non-empty text from Anthropic"
    );
    eprintln!("Anthropic response: {}", text);
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_anthropic_tool_calling() {
    let Some(client) = load_anthropic_client() else {
        return;
    };

    let tool = EchoTool;
    let response = Provider::send_conversation(
        &client,
        None,
        vec![],
        vec![&tool as &dyn Tool],
        Some("Call the echo_tool exactly once with the argument {\"text\": \"hello from anthropic\"}. Do not respond with plain text."),
        None,
        None,
    )
    .await
    .expect("Anthropic tool call request failed");

    let tool_uses = t_koma_gateway::extract_tool_uses(&response);
    assert!(
        !tool_uses.is_empty(),
        "Expected at least one tool call from Anthropic, got: {:?}",
        response.content
    );
    assert_eq!(tool_uses[0].1, "echo_tool");
    eprintln!("Anthropic tool call args: {:?}", tool_uses[0].2);
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_anthropic_accepts_chat_tools() {
    let Some(client) = load_anthropic_client() else {
        return;
    };

    let manager = ToolManager::new_chat(vec![]);
    let tools = manager.get_tools();
    eprintln!("Sending {} chat tools to Anthropic…", tools.len());

    let response = Provider::send_conversation(
        &client,
        None,
        vec![],
        tools,
        Some("Reply with exactly: 'tools accepted'. Do not call any tools."),
        None,
        None,
    )
    .await;

    assert!(
        response.is_ok(),
        "Anthropic rejected chat tools: {:?}",
        response.unwrap_err()
    );
    eprintln!(
        "Anthropic chat tools accepted. Response: {}",
        t_koma_gateway::extract_all_text(&response.unwrap())
    );
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_anthropic_accepts_reflection_tools() {
    let Some(client) = load_anthropic_client() else {
        return;
    };

    let manager = ToolManager::new_reflection(vec![]);
    let tools = manager.get_tools();
    eprintln!("Sending {} reflection tools to Anthropic…", tools.len());

    let response = Provider::send_conversation(
        &client,
        None,
        vec![],
        tools,
        Some("Reply with exactly: 'tools accepted'. Do not call any tools."),
        None,
        None,
    )
    .await;

    assert!(
        response.is_ok(),
        "Anthropic rejected reflection tools: {:?}",
        response.unwrap_err()
    );
    eprintln!(
        "Anthropic reflection tools accepted. Response: {}",
        t_koma_gateway::extract_all_text(&response.unwrap())
    );
}

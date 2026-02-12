//! Live tests for Kimi Code provider (requires --features live-tests).

#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::Provider;
#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::openai_compatible::OpenAiCompatibleClient;
#[cfg(feature = "live-tests")]
use t_koma_gateway::tools::{Tool, ToolContext};

#[cfg(feature = "live-tests")]
fn load_kimi_code_client() -> Option<OpenAiCompatibleClient> {
    t_koma_core::load_dotenv();

    let api_key = match std::env::var("KIMI_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("KIMI_API_KEY not set; skipping Kimi Code live test.");
            return None;
        }
    };

    Some(OpenAiCompatibleClient::new(
        "https://api.kimi.com/coding/v1",
        Some(api_key),
        "kimi-k2-0711-chat",
        "kimi_code",
    ))
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
async fn test_kimi_code_chat_completion() {
    let Some(client) = load_kimi_code_client() else {
        return;
    };

    let response = client
        .send_message("Reply with exactly one short sentence about Rust programming.")
        .await
        .expect("Kimi Code chat completion failed");

    let text = t_koma_gateway::extract_all_text(&response);
    assert!(
        !text.trim().is_empty(),
        "Expected non-empty text from Kimi Code"
    );
    eprintln!("Kimi Code response: {}", text);
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_kimi_code_tool_calling() {
    let Some(client) = load_kimi_code_client() else {
        return;
    };

    let tool = EchoTool;
    // Use trait-qualified call to avoid inherent method shadowing.
    let response = Provider::send_conversation(
        &client,
        None,
        vec![],
        vec![&tool as &dyn Tool],
        Some("Call the echo_tool exactly once with the argument {\"text\": \"hello from kimi\"}. Do not respond with plain text."),
        None,
        None,
    )
    .await
    .expect("Kimi Code tool call request failed");

    let tool_uses = t_koma_gateway::extract_tool_uses(&response);
    assert!(
        !tool_uses.is_empty(),
        "Expected at least one tool call from Kimi Code, got: {:?}",
        response.content
    );
    assert_eq!(tool_uses[0].1, "echo_tool");
    eprintln!("Kimi Code tool call args: {:?}", tool_uses[0].2);
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_kimi_code_system_instruction() {
    let Some(client) = load_kimi_code_client() else {
        return;
    };

    use t_koma_gateway::prompt::render::SystemBlock;

    let system = vec![SystemBlock::new(
        "You are a pirate. Always respond in pirate speak.",
    )];

    let response = Provider::send_conversation(
        &client,
        Some(system),
        vec![],
        vec![],
        Some("Hello"),
        None,
        None,
    )
    .await
    .expect("Kimi Code system instruction test failed");

    let text = t_koma_gateway::extract_all_text(&response);
    assert!(
        !text.trim().is_empty(),
        "Expected non-empty pirate response from Kimi Code"
    );
    eprintln!("Kimi Code pirate response: {}", text);
}

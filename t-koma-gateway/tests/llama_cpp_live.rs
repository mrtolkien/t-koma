//! Live tests for llama.cpp provider (requires --features live-tests).

#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::Provider;
#[cfg(feature = "live-tests")]
use t_koma_gateway::providers::llama_cpp::LlamaCppClient;
#[cfg(feature = "live-tests")]
use t_koma_gateway::tools::{Tool, ToolContext};

#[cfg(feature = "live-tests")]
struct EchoTool;

#[cfg(feature = "live-tests")]
#[async_trait::async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_tool"
    }

    fn description(&self) -> &str {
        "Echoes the provided text."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string"}
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
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string())
    }
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_llama_cpp_chat_completion() {
    t_koma_core::load_dotenv();

    let base_url = match std::env::var("LLAMA_CPP_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("LLAMA_CPP_URL not set; skipping live llama.cpp test.");
            return;
        }
    };
    let model_name = match std::env::var("LLAMA_CPP_MODEL_NAME") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("LLAMA_CPP_MODEL_NAME not set; skipping live llama.cpp test.");
            return;
        }
    };

    let client = LlamaCppClient::new(
        base_url,
        std::env::var("LLAMA_CPP_API_KEY").ok(),
        model_name,
    );

    let response = client
        .send_message("Reply with one short line about Rust.")
        .await
        .expect("llama.cpp chat completion failed");

    let text = t_koma_gateway::extract_all_text(&response);
    assert!(
        !text.trim().is_empty(),
        "Expected non-empty text from llama.cpp"
    );
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_llama_cpp_tool_calling() {
    t_koma_core::load_dotenv();

    let base_url = match std::env::var("LLAMA_CPP_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("LLAMA_CPP_URL not set; skipping live llama.cpp tool-call test.");
            return;
        }
    };
    let model_name = match std::env::var("LLAMA_CPP_MODEL_NAME") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("LLAMA_CPP_MODEL_NAME not set; skipping live llama.cpp tool-call test.");
            return;
        }
    };

    let client = LlamaCppClient::new(
        base_url,
        std::env::var("LLAMA_CPP_API_KEY").ok(),
        model_name,
    );

    let tool = EchoTool;
    let response = client
        .send_conversation(
            None,
            vec![],
            vec![&tool],
            Some(
                "Call the echo_tool exactly once with JSON arguments {\"text\":\"ping\"}. Do not answer in plain text.",
            ),
            None,
            None,
        )
        .await
        .expect("llama.cpp tool call request failed");

    let tool_uses = t_koma_gateway::extract_tool_uses(&response);
    assert!(
        !tool_uses.is_empty(),
        "Expected at least one tool call from llama.cpp, got: {:?}",
        response.content
    );
    assert_eq!(tool_uses[0].1, "echo_tool");
}

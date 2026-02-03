//! Snapshot tests for the Anthropic API.
//!
//! Run with: cargo test --features live-tests
//!
//! These tests capture real API responses (with insta redactions to handle
//! dynamic fields like `id`). Review the `.snap` files to see actual API output.

#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::models::anthropic::AnthropicClient;
#[cfg(feature = "live-tests")]
use t_koma_gateway::tools::{shell::ShellTool, Tool};

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

/// Test tool use - asks Claude to use the shell tool
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_tool_use_shell() {
    t_koma_core::load_dotenv();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set for live tests");

    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];

    let response = client
        .send_message_with_tools("List the files in the current directory using the shell tool.", tools)
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "tool_use_shell",
        response,
        {
            ".id" => "[id]",
            ".content[].id" => "[tool_use_id]"
        }
    );
}

/// Test that asks Claude to run pwd and validates the tool is used correctly
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_pwd_tool_execution() {
    t_koma_core::load_dotenv();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set for live tests");

    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");
    
    // Build system prompt with tool instructions
    let system_prompt = t_koma_gateway::prompt::SystemPrompt::new();
    let system_blocks = t_koma_gateway::models::anthropic::prompt::build_anthropic_system_prompt(&system_prompt);
    
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];

    // First request - ask Claude to run pwd
    let response = client
        .send_conversation(
            Some(system_blocks.clone()),
            vec![],
            tools.clone(),
            Some("Run the pwd command and tell me what directory you're in."),
            None,
            None,
        )
        .await
        .expect("API call failed");

    // Check that Claude used the tool
    let has_tool_use = response.content.iter().any(|b| matches!(b, 
        t_koma_gateway::models::anthropic::ContentBlock::ToolUse { name, .. } if name == "run_shell_command"
    ));
    
    assert!(has_tool_use, "Expected Claude to use run_shell_command tool");

    // Collect tool uses to process
    let tool_uses: Vec<_> = response.content.iter().filter_map(|b| {
        if let t_koma_gateway::models::anthropic::ContentBlock::ToolUse { id, name, input } = b {
            Some((id.clone(), name.clone(), input.clone()))
        } else {
            None
        }
    }).collect();
    
    // Build conversation history
    let mut messages = vec![];
    messages.push(t_koma_gateway::models::anthropic::history::ApiMessage {
        role: "user".to_string(),
        content: vec![t_koma_gateway::models::anthropic::history::ApiContentBlock::Text {
            text: "Run the pwd command and tell me what directory you're in.".to_string(),
            cache_control: None,
        }],
    });
    
    // Add assistant's response with tool_use
    let assistant_content: Vec<t_koma_gateway::models::anthropic::history::ApiContentBlock> = response.content.iter().map(|b| match b {
        t_koma_gateway::models::anthropic::ContentBlock::Text { text } => 
            t_koma_gateway::models::anthropic::history::ApiContentBlock::Text { text: text.clone(), cache_control: None },
        t_koma_gateway::models::anthropic::ContentBlock::ToolUse { id, name, input } => 
            t_koma_gateway::models::anthropic::history::ApiContentBlock::ToolUse { 
                id: id.clone(), name: name.clone(), input: input.clone() 
            },
    }).collect();
    
    messages.push(t_koma_gateway::models::anthropic::history::ApiMessage {
        role: "assistant".to_string(),
        content: assistant_content,
    });
    
    // Process tool results
    let mut tool_results = Vec::new();
    for (id, name, input) in tool_uses {
        assert_eq!(name, "run_shell_command");
        
        // Execute the shell command
        let result = shell_tool.execute(input).await;
        assert!(result.is_ok(), "Shell command should succeed");
        
        let output = result.unwrap();
        
        // Verify the output looks like a path (starts with /)
        assert!(output.starts_with('/'), "pwd output should be an absolute path, got: {}", output);
        
        tool_results.push(t_koma_gateway::models::anthropic::history::ToolResultData {
            tool_use_id: id,
            content: output,
            is_error: None,
        });
    }
    
    // Add tool result message
    messages.push(t_koma_gateway::models::anthropic::history::build_tool_result_message(tool_results));
    
    // Get final response from Claude
    let final_response = client
        .send_conversation(
            Some(system_blocks),
            messages,
            tools,
            None,
            None,
            None,
        )
        .await
        .expect("Second API call failed");
    
    // Verify the response mentions the directory
    let text = t_koma_gateway::models::anthropic::AnthropicClient::extract_all_text(&final_response);
    assert!(!text.is_empty(), "Final response should not be empty");
    
    // The response should mention being in a directory (contains /)
    assert!(text.contains('/'), "Response should mention directory path, got: {}", text);
    
    println!("Claude's response about pwd: {}", text);
}
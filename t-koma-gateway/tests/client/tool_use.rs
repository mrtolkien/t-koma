//! Tool use tests for the default configured provider.
//!
//! These tests verify that the configured model can correctly use tools.

#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::{extract_all_text, ProviderContentBlock};
#[cfg(feature = "live-tests")]
use t_koma_gateway::tools::{file_edit::FileEditTool, shell::ShellTool, Tool};

#[cfg(feature = "live-tests")]
use crate::common;

/// Test tool use - asks the model to use the shell tool
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_tool_use_shell() {
    let default_model = common::load_default_model();
    let client = default_model.client;
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];

    let response = client
        .send_conversation(
            None,
            vec![],
            tools,
            Some("List the files in the current directory using the shell tool."),
            None,
            None,
        )
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

/// Test that asks the model to run pwd and validates the tool is used correctly
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_pwd_tool_execution() {
    let default_model = common::load_default_model();
    let client = default_model.client;

    // Build system prompt with tool instructions
    let system_prompt = t_koma_gateway::prompt::SystemPrompt::new();
    let system_blocks =
        t_koma_gateway::models::prompt::build_system_prompt(&system_prompt);

    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];

    // First request - ask the model to run pwd
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

    // Check that the model used the tool
    let has_tool_use = response.content.iter().any(|b| matches!(
        b,
        ProviderContentBlock::ToolUse { name, .. } if name == "run_shell_command"
    ));

    assert!(
        has_tool_use,
        "Expected the model to use run_shell_command tool"
    );

    // Collect tool uses to process
    let tool_uses: Vec<_> = response
        .content
        .iter()
        .filter_map(|b| match b {
            ProviderContentBlock::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect();

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
    let assistant_content: Vec<t_koma_gateway::models::anthropic::history::ApiContentBlock> =
        response
            .content
            .iter()
            .map(|b| match b {
                ProviderContentBlock::Text { text } => {
                    t_koma_gateway::models::anthropic::history::ApiContentBlock::Text {
                        text: text.clone(),
                        cache_control: None,
                    }
                }
                ProviderContentBlock::ToolUse { id, name, input } => {
                    t_koma_gateway::models::anthropic::history::ApiContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
                ProviderContentBlock::ToolResult { .. } => {
                    t_koma_gateway::models::anthropic::history::ApiContentBlock::Text {
                        text: String::new(),
                        cache_control: None,
                    }
                }
            })
            .collect();

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
        assert!(
            output.starts_with('/'),
            "pwd output should be an absolute path, got: {}",
            output
        );

        tool_results.push(t_koma_gateway::models::anthropic::history::ToolResultData {
            tool_use_id: id,
            content: output,
            is_error: None,
        });
    }

    // Add tool result message
    messages.push(
        t_koma_gateway::models::anthropic::history::build_tool_result_message(tool_results),
    );

    // Get final response from the model
    let final_response = client
        .send_conversation(Some(system_blocks), messages, tools, None, None, None)
        .await
        .expect("Second API call failed");

    // Verify the response mentions the directory
    let text = extract_all_text(&final_response);
    assert!(!text.is_empty(), "Final response should not be empty");

    // The response should mention being in a directory (contains /)
    assert!(
        text.contains('/'),
        "Response should mention directory path, got: {}",
        text
    );

    println!("Model response about pwd: {}", text);
}

/// Test tool use - asks the model to use the file edit tool
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_tool_use_file_edit() {
    let default_model = common::load_default_model();
    let client = default_model.client;
    let file_edit_tool = FileEditTool;
    let tools: Vec<&dyn Tool> = vec![&file_edit_tool];

    // We use a hypothetical path. The model should try to edit it.
    let response = client
        .send_conversation(
            None,
            vec![],
            tools,
            Some("Change the text 'hello' to 'world' in the file '/tmp/test_file.txt'."),
            None,
            None,
        )
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "tool_use_file_edit",
        response,
        {
            ".id" => "[id]",
            ".content[].id" => "[tool_use_id]"
        }
    );
}

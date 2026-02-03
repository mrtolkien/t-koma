//! Skill usage integration test.
//!
//! This test verifies that the model can discover and use skills through
//! the load_skill tool.

#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    models::anthropic::AnthropicClient,
    tools::{load_skill::LoadSkillTool, Tool},
};

/// Test that the model can discover and load a skill.
///
/// This test:
/// 1. Sets up a test skill
/// 2. Tells the model about available skills
/// 3. Asks the model to use the skill
/// 4. Verifies the model calls load_skill tool
/// 5. Processes the tool result
/// 6. Verifies the model's final response
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_skill_discovery_and_load() {
    t_koma_core::load_dotenv();
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY must be set for live tests");

    // Set up temp directory with test skill
    let temp_dir = tempfile::TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir(&skills_dir).unwrap();

    // Create test skill
    let test_skill_dir = skills_dir.join("test-echo");
    std::fs::create_dir(&test_skill_dir).unwrap();
    std::fs::write(
        test_skill_dir.join("SKILL.md"),
        r#"---
name: test-echo
description: A simple test skill that echoes messages. Use when asked to demonstrate skill usage.
---

# Test Echo Skill

This skill demonstrates echo functionality.

## Usage

When asked to use this skill, simply confirm that you have loaded it successfully.

## Example

User: "Use the test-echo skill"
You: "Successfully loaded and using the test-echo skill!"
"#,
    )
    .unwrap();

    // Create client
    let client = AnthropicClient::new(api_key, "claude-sonnet-4-5-20250929");

    // Set up tools
    let load_skill_tool = LoadSkillTool::new(skills_dir.clone());
    let tools: Vec<&dyn Tool> = vec![&load_skill_tool];

    // Build system prompt with skills listed
    let system_prompt_text = format!(
        r#"You are a helpful assistant with access to skills.

Available skills:
- test-echo: A simple test skill that echoes messages. Use when asked to demonstrate skill usage.

You have access to the load_skill tool. When a user asks you to use a skill that you haven't loaded yet,
use the load_skill tool with the skill_name parameter to load the skill content.

Skill directory: {:?}
"#,
        skills_dir
    );

    // First request - ask the model to use the skill
    let response = client
        .send_conversation(
            Some(vec![t_koma_gateway::models::prompt::SystemBlock::new(&system_prompt_text)]),
            vec![],
            tools.clone(),
            Some("I have the test-echo skill available. Please use it to demonstrate skill loading."),
            None,
            None,
        )
        .await
        .expect("API call failed");

    // Snapshot the initial response
    assert_json_snapshot!(
        "skill_usage_initial",
        response,
        {
            ".id" => "[id]",
            ".content[].id" => "[tool_use_id]"
        }
    );

    // Check if the model used the load_skill tool
    let has_load_skill = response.content.iter().any(|b| matches!(b,
        t_koma_gateway::models::anthropic::ContentBlock::ToolUse { name, .. } if name == "load_skill"
    ));

    println!("Model requested skill load: {}", has_load_skill);
    assert!(has_load_skill, "Model should have requested to load the skill");

    // Build conversation history
    let mut messages = vec![];
    messages.push(t_koma_gateway::models::anthropic::history::ApiMessage {
        role: "user".to_string(),
        content: vec![t_koma_gateway::models::anthropic::history::ApiContentBlock::Text {
            text: "I have the test-echo skill available. Please use it to demonstrate skill loading.".to_string(),
            cache_control: None,
        }],
    });

    // Add assistant's response with tool_use
    let assistant_content: Vec<t_koma_gateway::models::anthropic::history::ApiContentBlock> =
        response
            .content
            .iter()
            .map(|b| match b {
                t_koma_gateway::models::anthropic::ContentBlock::Text { text } => {
                    t_koma_gateway::models::anthropic::history::ApiContentBlock::Text {
                        text: text.clone(),
                        cache_control: None,
                    }
                }
                t_koma_gateway::models::anthropic::ContentBlock::ToolUse { id, name, input } => {
                    t_koma_gateway::models::anthropic::history::ApiContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
            })
            .collect();

    messages.push(t_koma_gateway::models::anthropic::history::ApiMessage {
        role: "assistant".to_string(),
        content: assistant_content,
    });

    // Process load_skill tool
    let tool_uses: Vec<_> = response
        .content
        .iter()
        .filter_map(|b| {
            if let t_koma_gateway::models::anthropic::ContentBlock::ToolUse { id, name, input } = b
            {
                Some((id.clone(), name.clone(), input.clone()))
            } else {
                None
            }
        })
        .collect();

    let mut tool_results = Vec::new();
    for (id, name, input) in tool_uses {
        if name == "load_skill" {
            let result = load_skill_tool.execute(input).await;
            println!("Load skill result: {:?}", result.is_ok());
            tool_results.push(
                t_koma_gateway::models::anthropic::history::ToolResultData {
                    tool_use_id: id,
                    content: result.unwrap_or_else(|e| format!("Error: {}", e)),
                    is_error: None,
                },
            );
        }
    }

    // Add tool result message
    assert!(!tool_results.is_empty(), "Should have tool results");
    messages.push(
        t_koma_gateway::models::anthropic::history::build_tool_result_message(
            tool_results,
        ),
    );

    // Get final response from the model
    let final_response = client
        .send_conversation(
            Some(vec![t_koma_gateway::models::prompt::SystemBlock::new(&system_prompt_text)]),
            messages, 
            tools, 
            None, 
            None, 
            None
        )
        .await
        .expect("Second API call failed");

    // Snapshot final response
    assert_json_snapshot!(
        "skill_usage_final",
        final_response,
        {
            ".id" => "[id]"
        }
    );

    // Verify the response mentions the skill
    let text = AnthropicClient::extract_all_text(&final_response);
    assert!(
        text.to_lowercase().contains("skill") || text.to_lowercase().contains("echo"),
        "Response should mention skill or echo, got: {}",
        text
    );

    println!("Model's final response: {}", text);
}

//! Multi-turn conversation tests through the gateway.
//!
//! These tests verify that the full gateway stack (AppState + Database) correctly
//! handles multi-turn conversations with context preservation.

#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_db::SessionRepository;
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    chat::history::build_history_messages,
    prompt::SystemPrompt,
    prompt::render::build_system_prompt,
    tools::{Tool, shell::ShellTool},
};
#[cfg(feature = "live-tests")]
use uuid::Uuid;

#[cfg(feature = "live-tests")]
use crate::common;

/// Helper struct to capture conversation turn results
#[cfg(feature = "live-tests")]
#[derive(Debug, serde::Serialize)]
struct ConversationTurn {
    turn: usize,
    user_message: String,
    assistant_response: String,
    session_id: String,
    message_count: i64,
}

/// Helper struct for the full conversation
#[cfg(feature = "live-tests")]
#[derive(Debug, serde::Serialize)]
struct Conversation {
    session_title: String,
    operator_id: String,
    turns: Vec<ConversationTurn>,
    total_messages: i64,
}

/// Test a multi-turn conversation where we:
/// 1. Ask the model to tell us a short story
/// 2. Ask it to repeat the same story
///
/// This verifies that:
/// - Session is created and persisted
/// - Messages are stored in the database
/// - Context is preserved across turns
/// - Tool use works through AppState
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_multi_turn_story_conversation() {
    t_koma_core::load_dotenv();

    let ghost_name = format!("test-ghost-{}", Uuid::new_v4());
    let env = common::setup_test_environment("Test Operator", &ghost_name)
        .await
        .expect("Failed to set up test environment");
    let state = common::build_state_with_default_model(env.koma_db.clone()).await;
    let ghost_db = env.ghost_db;
    let operator = env.operator;
    let ghost = env.ghost;

    // Create a session
    let session =
        SessionRepository::create(ghost_db.pool(), &operator.id, Some("Multi-turn Story Test"))
            .await
            .expect("Failed to create session");

    println!("Created session: {}", session.id);

    // Set up system prompt and tools
    let system_prompt = SystemPrompt::new(&[
        ("ghost_identity", ""),
        ("ghost_diary", ""),
        ("ghost_projects", ""),
        ("ghost_skills", ""),
        ("system_info", ""),
    ]);
    let system_blocks = build_system_prompt(&system_prompt);
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];
    let model = state.default_model().model.as_str();

    let mut conversation_turns = vec![];

    // === TURN 1: Ask for a short story ===
    let turn1_message = "Tell me a very short story (2-3 sentences) about a robot learning to paint. Remember this story exactly.";

    // Save user message
    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: turn1_message.to_string(),
        }],
        None,
    )
    .await
    .expect("Failed to save turn 1 user message");

    // Get conversation history and build API messages
    let history1 = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages1 = build_history_messages(&history1, Some(50));

    // Send to model through AppState
    let response1 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages1,
            tools.clone(),
            Some(turn1_message),
            model,
        )
        .await
        .expect("Failed to get turn 1 response");

    println!("Turn 1 response:\n{}\n", response1);

    // Count messages after turn 1
    let msg_count1 = SessionRepository::count_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to count messages");

    conversation_turns.push(ConversationTurn {
        turn: 1,
        user_message: turn1_message.to_string(),
        assistant_response: response1.clone(),
        session_id: session.id.clone(),
        message_count: msg_count1,
    });

    // === TURN 2: Ask to repeat the same story ===
    let turn2_message = "Now tell me the exact same story again, word for word.";

    // Save user message
    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: turn2_message.to_string(),
        }],
        None,
    )
    .await
    .expect("Failed to save turn 2 user message");

    // Get updated conversation history
    let history2 = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages2 = build_history_messages(&history2, Some(50));

    // Send to model through AppState
    let response2 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages2,
            tools.clone(),
            Some(turn2_message),
            model,
        )
        .await
        .expect("Failed to get turn 2 response");

    println!("Turn 2 response:\n{}\n", response2);

    // Count messages after turn 2
    let msg_count2 = SessionRepository::count_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to count messages");

    conversation_turns.push(ConversationTurn {
        turn: 2,
        user_message: turn2_message.to_string(),
        assistant_response: response2.clone(),
        session_id: session.id.clone(),
        message_count: msg_count2,
    });

    // === TURN 3: Verify context with a follow-up question ===
    let turn3_message = "What was the main character in that story?";

    // Save user message
    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: turn3_message.to_string(),
        }],
        None,
    )
    .await
    .expect("Failed to save turn 3 user message");

    // Get updated conversation history
    let history3 = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages3 = build_history_messages(&history3, Some(50));

    // Send to model through AppState
    let response3 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks,
            api_messages3,
            tools,
            Some(turn3_message),
            model,
        )
        .await
        .expect("Failed to get turn 3 response");

    println!("Turn 3 response:\n{}\n", response3);

    // Count messages after turn 3
    let msg_count3 = SessionRepository::count_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to count messages");

    conversation_turns.push(ConversationTurn {
        turn: 3,
        user_message: turn3_message.to_string(),
        assistant_response: response3.clone(),
        session_id: session.id.clone(),
        message_count: msg_count3,
    });

    // Build final conversation snapshot
    let conversation = Conversation {
        session_title: session.title,
        operator_id: operator.id,
        turns: conversation_turns,
        total_messages: msg_count3,
    };

    // Take snapshot for human review
    assert_json_snapshot!(
        "multi_turn_story_conversation",
        conversation,
        {
            ".turns[].session_id" => "[session_id]",
            ".operator_id" => "[operator_id]",
        }
    );

    // Verify that we have the expected number of messages
    // Turn 1: user + assistant = 2
    // Turn 2: user + assistant = 2 (+ 2 = 4)
    // Turn 3: user + assistant = 2 (+ 2 = 6)
    assert_eq!(
        msg_count3, 6,
        "Expected 6 messages total (3 turns x 2 messages)"
    );

    println!("\n✅ Multi-turn conversation test completed successfully!");
    println!("Session ID: {}", session.id);
    println!("Total messages: {}", msg_count3);
    println!("Ghost: {}", ghost.name);
}

/// Test multi-turn conversation with tool use through the gateway
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_multi_turn_with_tool_use() {
    // Set up in-memory test database
    let ghost_name = format!("test-ghost-{}", Uuid::new_v4());
    let env = common::setup_test_environment("Test Operator", &ghost_name)
        .await
        .expect("Failed to set up test environment");
    let state = common::build_state_with_default_model(env.koma_db.clone()).await;
    let ghost_db = env.ghost_db;
    let operator = env.operator;
    let ghost = env.ghost;

    // Create a session
    let session =
        SessionRepository::create(ghost_db.pool(), &operator.id, Some("Multi-turn Tool Test"))
            .await
            .expect("Failed to create session");

    // Set up system prompt and tools
    let system_prompt = SystemPrompt::new(&[
        ("ghost_identity", ""),
        ("ghost_diary", ""),
        ("ghost_projects", ""),
        ("ghost_skills", ""),
        ("system_info", ""),
    ]);
    let system_blocks = build_system_prompt(&system_prompt);
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];
    let model = state.default_model().model.as_str();

    // === TURN 1: Ask to run pwd ===
    let turn1_message = "What directory are we in? Use the shell tool to find out.";

    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: turn1_message.to_string(),
        }],
        None,
    )
    .await
    .expect("Failed to save turn 1 user message");

    let history1 = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages1 = build_history_messages(&history1, Some(50));

    let response1 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages1,
            tools.clone(),
            Some(turn1_message),
            model,
        )
        .await
        .expect("Failed to get turn 1 response");

    println!("Turn 1 response (with tool):\n{}\n", response1);

    // Verify the response mentions a directory
    assert!(
        response1.contains('/'),
        "Response should contain a directory path"
    );

    // === TURN 2: Ask what command was run ===
    let turn2_message = "What command did you just run to find that out?";

    SessionRepository::add_message(
        ghost_db.pool(),
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: turn2_message.to_string(),
        }],
        None,
    )
    .await
    .expect("Failed to save turn 2 user message");

    let history2 = SessionRepository::get_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages2 = build_history_messages(&history2, Some(50));

    let response2 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks,
            api_messages2,
            tools,
            Some(turn2_message),
            model,
        )
        .await
        .expect("Failed to get turn 2 response");

    println!("Turn 2 response (context check):\n{}\n", response2);

    // Verify model remembers it used pwd
    assert!(
        response2.to_lowercase().contains("pwd")
            || response2.to_lowercase().contains("shell")
            || response2.to_lowercase().contains("command"),
        "Response should mention pwd, shell, or command"
    );

    // Verify message count includes tool_use and tool_result blocks
    let msg_count = SessionRepository::count_messages(ghost_db.pool(), &session.id)
        .await
        .expect("Failed to count messages");

    println!("Total messages in session: {}", msg_count);
    println!("Ghost: {}", ghost.name);

    // Should have: user1, assistant1 (with tool_use), user1_tool_result, user2, assistant2
    // That's 5 message rows
    assert!(
        msg_count >= 4,
        "Expected at least 4 messages (including tool interactions)"
    );

    println!("\n✅ Multi-turn tool use test completed successfully!");
}

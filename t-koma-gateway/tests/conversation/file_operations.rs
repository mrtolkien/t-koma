//! File operations tests through the gateway.
//!
//! These tests verify that the full gateway stack (AppState + Database) correctly
//! handles file operations via tool use. Tests use `send_conversation_with_tools()`
//! which includes the complete tool execution loop.
//!
//! Run with: cargo test --features live-tests conversation::file_operations

#[cfg(feature = "live-tests")]
use t_koma_db::SessionRepository;
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    chat::history::build_history_messages,
    prompt::SystemPrompt,
    prompt::render::build_system_prompt,
    tools::{Tool, file_edit::FileEditTool, shell::ShellTool},
};
#[cfg(feature = "live-tests")]
use uuid::Uuid;

#[cfg(feature = "live-tests")]
use crate::common;

/// Test file operations workflow:
/// 1. Create a file with initial content
/// 2. Edit the file with replace tool
/// 3. Delete the file
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_file_create_edit_delete_workflow() {
    let ghost_name = format!("test-ghost-{}", Uuid::new_v4());
    let env = common::setup_test_environment("Test Operator", &ghost_name)
        .await
        .expect("Failed to set up test environment");
    let koma_db = env.koma_db;
    let state = common::build_state_with_default_model(koma_db.clone()).await;
    let operator = env.operator;
    let ghost = env.ghost;

    // Create a session
    let session = SessionRepository::create(koma_db.pool(), &ghost.id, &operator.id)
        .await
        .expect("Failed to create session");

    println!("Created session: {}", session.id);

    // Create temp file path
    let temp_file = format!("/tmp/t_koma_test_{}.txt", uuid::Uuid::new_v4());

    // Set up system prompt and tools
    let shell_tool = ShellTool;
    let file_edit_tool = FileEditTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool, &file_edit_tool];
    let system_prompt = SystemPrompt::new(&[
        ("ghost_identity", ""),
        ("ghost_diary", ""),
        ("ghost_skills", ""),
        ("system_info", ""),
    ]);
    let system_blocks = build_system_prompt(&system_prompt);
    let model = state.default_model().model.as_str();

    // === STEP 1: Create a file ===
    println!("\n=== STEP 1: Creating file ===");
    let create_message = format!(
        "Create a file at '{}' with the content 'Hello, World!' using the shell tool. \
         Use echo command to write the content.",
        temp_file
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: create_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save create message");

    let history1 = SessionRepository::get_messages(koma_db.pool(), &session.id)
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
            Some(&create_message),
            model,
        )
        .await
        .expect("Failed to create file");

    println!("Create response: {}", response1);

    // Verify file was created
    let verify_created = tokio::fs::read_to_string(&temp_file).await;
    assert!(
        verify_created.is_ok(),
        "File should have been created: {:?}",
        verify_created
    );
    assert_eq!(
        verify_created.unwrap().trim(),
        "Hello, World!",
        "File content should match"
    );
    println!("✅ File created successfully with correct content");

    // === STEP 2: Edit the file ===
    println!("\n=== STEP 2: Editing file ===");
    let edit_message = format!(
        "Change the content of '{}' from 'Hello, World!' to 'Hello, Rust!' \
         using the replace tool. The old_string should be 'Hello, World!' and \
         the new_string should be 'Hello, Rust!'.",
        temp_file
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: edit_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save edit message");

    let history2 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages2 = build_history_messages(&history2, Some(50));

    let response2 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages2,
            tools.clone(),
            Some(&edit_message),
            model,
        )
        .await
        .expect("Failed to edit file");

    println!("Edit response: {}", response2);

    // Verify file was edited
    let verify_edited = tokio::fs::read_to_string(&temp_file).await;
    assert!(
        verify_edited.is_ok(),
        "File should still exist: {:?}",
        verify_edited
    );
    assert_eq!(
        verify_edited.unwrap().trim(),
        "Hello, Rust!",
        "File content should have been updated"
    );
    println!("✅ File edited successfully");

    // === STEP 3: Delete the file ===
    println!("\n=== STEP 3: Deleting file ===");
    let delete_message = format!(
        "Delete the file at '{}' using the shell tool with rm command.",
        temp_file
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: delete_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save delete message");

    let history3 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages3 = build_history_messages(&history3, Some(50));

    let response3 = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks,
            api_messages3,
            tools,
            Some(&delete_message),
            model,
        )
        .await
        .expect("Failed to delete file");

    println!("Delete response: {}", response3);

    // Verify file was deleted
    let verify_deleted = tokio::fs::try_exists(&temp_file).await;
    assert!(
        verify_deleted.is_ok() && !verify_deleted.unwrap(),
        "File should have been deleted"
    );
    println!("✅ File deleted successfully");

    // Verify message count
    let msg_count = SessionRepository::count_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to count messages");

    println!("\n=== Summary ===");
    println!("Session ID: {}", session.id);
    println!("Total messages: {}", msg_count);
    println!("Ghost: {}", ghost.name);
    println!("\n✅ File operations workflow test completed successfully!");
}

/// Test that the replace tool requires exact matching
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_replace_tool_exact_match_requirement() {
    let ghost_name = format!("test-ghost-{}", Uuid::new_v4());
    let env = common::setup_test_environment("Test Operator", &ghost_name)
        .await
        .expect("Failed to set up test environment");
    let koma_db = env.koma_db;
    let state = common::build_state_with_default_model(koma_db.clone()).await;
    let operator = env.operator;
    let ghost = env.ghost;

    // Create a session
    let session = SessionRepository::create(koma_db.pool(), &ghost.id, &operator.id)
        .await
        .expect("Failed to create session");

    // Create temp file with multiline content
    let temp_file = format!("/tmp/t_koma_test_exact_{}.txt", uuid::Uuid::new_v4());
    let initial_content = "Line 1: Hello\nLine 2: World\nLine 3: Foo\nLine 4: Bar";
    tokio::fs::write(&temp_file, initial_content)
        .await
        .expect("Failed to create test file");

    println!("Created test file at: {}", temp_file);

    // Set up system prompt and tools
    let shell_tool = ShellTool;
    let file_edit_tool = FileEditTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool, &file_edit_tool];
    let system_prompt = SystemPrompt::new(&[
        ("ghost_identity", ""),
        ("ghost_diary", ""),
        ("ghost_skills", ""),
        ("system_info", ""),
    ]);
    let system_blocks = build_system_prompt(&system_prompt);
    let model = state.default_model().model.as_str();

    // Ask the model to edit the file
    let edit_message = format!(
        "Read the file at '{}' and change 'Line 2: World' to 'Line 2: Rust' \
         using the replace tool. Make sure to include enough context in old_string.",
        temp_file
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: edit_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save edit message");

    let history = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages = build_history_messages(&history, Some(50));

    let response = state
        .send_conversation_with_tools(
            &ghost.name,
            state.default_model().client.as_ref(),
            &session.id,
            system_blocks,
            api_messages,
            tools,
            Some(&edit_message),
            model,
        )
        .await
        .expect("Failed to edit file");

    println!("Response: {}", response);

    // Verify the edit
    let content = tokio::fs::read_to_string(&temp_file)
        .await
        .expect("Failed to read file");

    assert!(
        content.contains("Line 2: Rust"),
        "File should contain the updated line"
    );
    assert!(
        !content.contains("Line 2: World"),
        "File should not contain the old line"
    );

    // Cleanup
    let _ = tokio::fs::remove_file(&temp_file).await;

    println!("Ghost: {}", ghost.name);
    println!("✅ Exact match test completed successfully!");
}

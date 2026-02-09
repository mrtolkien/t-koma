//! Comprehensive workflow test for all new coding tools.
//!
//! This test exercises all the new tools together:
//! - list_dir: Explore directory structure
//! - find_files: Locate specific file types
//! - create_file: Create new source files
//! - read_file: Read file contents
//! - search: Find patterns in code
//! - replace: Edit existing files
//!
//! Run with: cargo test --features live-tests conversation::new_tools_workflow

#[cfg(feature = "live-tests")]
use t_koma_db::SessionRepository;
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    chat::history::build_history_messages, prompt::SystemPrompt,
    prompt::render::build_system_prompt, tools::manager::ToolManager,
};
#[cfg(feature = "live-tests")]
use uuid::Uuid;

#[cfg(feature = "live-tests")]
use crate::common;

/// Assert that the last tool used in the session matches the expected tool name.
/// Optionally validates that the tool input contains a specific substring.
#[cfg(feature = "live-tests")]
async fn assert_last_tool_used(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    expected_tool: &str,
    expected_input_contains: Option<&str>,
) {
    let (tool_name, tool_input) = SessionRepository::get_last_tool_use(pool, session_id)
        .await
        .expect("Failed to query tool uses")
        .expect("Expected a tool to have been used, but none found");

    assert_eq!(
        tool_name, expected_tool,
        "Expected tool '{}' but got '{}'",
        expected_tool, tool_name
    );

    if let Some(expected_substring) = expected_input_contains {
        let input_str = tool_input.to_string();
        assert!(
            input_str.contains(expected_substring),
            "Tool input should contain '{}', but was: {}",
            expected_substring,
            input_str
        );
    }
}

/// Assert that a specific tool was used at least once in the session.
#[cfg(feature = "live-tests")]
async fn assert_tool_used(pool: &sqlx::SqlitePool, session_id: &str, expected_tool: &str) {
    let tool_uses = SessionRepository::get_tool_uses(pool, session_id)
        .await
        .expect("Failed to query tool uses");

    let found = tool_uses.iter().any(|(name, _)| name == expected_tool);
    assert!(
        found,
        "Expected tool '{}' to have been used, but it wasn't. Tools used: {:?}",
        expected_tool,
        tool_uses
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );
}

/// Assert that a specific tool was used at least once after a given index.
#[cfg(feature = "live-tests")]
async fn assert_tool_used_since(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    expected_tool: &str,
    start_index: usize,
) {
    let tool_uses = SessionRepository::get_tool_uses(pool, session_id)
        .await
        .expect("Failed to query tool uses");

    let found = tool_uses
        .iter()
        .skip(start_index)
        .any(|(name, _)| name == expected_tool);
    assert!(
        found,
        "Expected tool '{}' to have been used after index {}, but it wasn't. Tools used: {:?}",
        expected_tool,
        start_index,
        tool_uses
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );
}

/// Comprehensive workflow test:
/// 1. List directory contents
/// 2. Find all Rust files in the project
/// 3. Create a new Rust module
/// 4. Read an existing file
/// 5. Search for function definitions
/// 6. Edit a file with the replace tool
/// 7. Verify the changes
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_comprehensive_coding_workflow() {
    t_koma_core::load_dotenv();

    let ghost_name = format!("test-ghost-{}", Uuid::new_v4());
    let env = common::setup_test_environment("Test Operator", &ghost_name)
        .await
        .expect("Failed to set up test environment");
    let default_model = common::load_default_model();
    let koma_db = env.koma_db;
    let state = common::build_state_with_default_model(koma_db.clone()).await;
    let operator = env.operator;
    let ghost = env.ghost;

    // Create a session
    let session = SessionRepository::create(koma_db.pool(), &ghost.id, &operator.id)
        .await
        .expect("Failed to create session");

    println!("Created session: {}", session.id);

    // Set up system prompt and tools
    let tool_manager = ToolManager::new(vec![]);
    let tools = tool_manager.get_all_tools();
    let system_prompt = SystemPrompt::new(&[
        ("ghost_identity", ""),
        ("ghost_diary", ""),
        ("ghost_skills", ""),
        ("system_info", ""),
    ]);
    let system_blocks = build_system_prompt(&system_prompt);
    let model = default_model.model.as_str();

    // Create a temporary project directory for testing
    let temp_dir = format!("/tmp/t_koma_test_project_{}", uuid::Uuid::new_v4());
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .expect("Failed to create temp directory");

    // Create initial project structure
    let src_dir = format!("{}/src", temp_dir);
    tokio::fs::create_dir_all(&src_dir)
        .await
        .expect("Failed to create src directory");

    // Create an initial main.rs file
    let main_rs = format!("{}/main.rs", src_dir);
    tokio::fs::write(
        &main_rs,
        "fn main() {\n    println!(\"Hello, World!\");\n}\n",
    )
    .await
    .expect("Failed to create main.rs");

    // Create a utils.rs file
    let utils_rs = format!("{}/utils.rs", src_dir);
    tokio::fs::write(
        &utils_rs,
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n",
    )
    .await
    .expect("Failed to create utils.rs");

    println!("\n=== Created test project at: {} ===", temp_dir);

    // === STEP 1: List directory contents ===
    println!("\n=== STEP 1: Listing directory contents ===");
    let list_message = format!(
        "Use the list_dir tool to list the contents of '{}' to see the project structure.",
        temp_dir
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: list_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save list message");

    let history1 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages1 = build_history_messages(&history1, Some(50));

    let _response1 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages1,
            tools.clone(),
            Some(&list_message),
            model,
        )
        .await
        .expect("Failed to list directory");

    // Verify: Check database for tool use
    assert_last_tool_used(koma_db.pool(), &session.id, "list_dir", Some(&temp_dir)).await;
    println!("✅ list_dir tool was used correctly");

    // === STEP 2: Find all Rust files ===
    println!("\n=== STEP 2: Finding all Rust files ===");
    let find_message = format!(
        "Use the find_files tool to find all Rust (*.rs) files in '{}'.",
        temp_dir
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: find_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save find message");

    let history2 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages2 = build_history_messages(&history2, Some(50));

    let _response2 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages2,
            tools.clone(),
            Some(&find_message),
            model,
        )
        .await
        .expect("Failed to find files");

    // Verify: Check database for tool use
    assert_last_tool_used(koma_db.pool(), &session.id, "find_files", Some("*.rs")).await;
    println!("✅ find_files tool was used correctly");

    // === STEP 3: Read an existing file ===
    println!("\n=== STEP 3: Reading existing file ===");
    let read_message = format!(
        "Use the read_file tool to read the contents of '{}'.",
        utils_rs
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: read_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save read message");

    let history3 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages3 = build_history_messages(&history3, Some(50));

    let _response3 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages3,
            tools.clone(),
            Some(&read_message),
            model,
        )
        .await
        .expect("Failed to read file");

    // Verify: Check database for tool use
    assert_last_tool_used(koma_db.pool(), &session.id, "read_file", Some(&utils_rs)).await;
    println!("✅ read_file tool was used correctly");

    // === STEP 4: Search for function definitions ===
    println!("\n=== STEP 4: Searching for function definitions ===");
    let search_message = format!(
        "Use the search tool to find all function definitions (lines starting with 'fn ' or 'pub fn ') in '{}'.",
        src_dir
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: search_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save search message");

    let history4 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages4 = build_history_messages(&history4, Some(50));

    let _response4 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages4,
            tools.clone(),
            Some(&search_message),
            model,
        )
        .await
        .expect("Failed to search");

    // Verify: Check database for tool use
    assert_last_tool_used(koma_db.pool(), &session.id, "search", Some(&src_dir)).await;
    println!("✅ search tool was used correctly");

    // === STEP 5: Create a new file ===
    println!("\n=== STEP 5: Creating new file ===");
    let new_module = format!("{}/math.rs", src_dir);
    let create_message = format!(
        "Use the create_file tool to create a new file at '{}' with the following content:\n\n\
         pub fn multiply(a: i32, b: i32) -> i32 {{\n    a * b\n}}\n\n\
         pub fn divide(a: i32, b: i32) -> Option<i32> {{\n    if b == 0 {{\n        None\n    }} else {{\n        Some(a / b)\n    }}\n}}",
        new_module
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

    let history5 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages5 = build_history_messages(&history5, Some(50));

    let _response5 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages5,
            tools.clone(),
            Some(&create_message),
            model,
        )
        .await
        .expect("Failed to create file");

    // Verify: Check database for tool use
    assert_last_tool_used(
        koma_db.pool(),
        &session.id,
        "create_file",
        Some(&new_module),
    )
    .await;

    // Also verify the file was actually created on disk
    let math_content = tokio::fs::read_to_string(&new_module).await;
    assert!(math_content.is_ok(), "math.rs should exist");
    let content = math_content.unwrap();
    assert!(
        content.contains("multiply"),
        "Should have multiply function"
    );
    assert!(content.contains("divide"), "Should have divide function");
    println!("✅ create_file tool was used correctly and file was created");

    // === STEP 6: Edit existing file ===
    println!("\n=== STEP 6: Editing existing file ===");
    let edit_message = format!(
        "Use the replace tool to add a new function to '{}'. \
         Add this function after the 'greet' function:\n\n\
         pub fn goodbye(name: &str) -> String {{\n    format!(\"Goodbye, {{}}!\", name)\n}}",
        utils_rs
    );

    let tool_uses_before_edit = SessionRepository::get_tool_uses(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get tool uses before edit")
        .len();

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

    let history6 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages6 = build_history_messages(&history6, Some(50));

    let _response6 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages6,
            tools.clone(),
            Some(&edit_message),
            model,
        )
        .await
        .expect("Failed to edit file");

    // Verify: replace should have been used during this step, even if a read follows.
    assert_tool_used_since(
        koma_db.pool(),
        &session.id,
        "replace",
        tool_uses_before_edit,
    )
    .await;

    // Also verify the edit was actually applied
    let updated_content = tokio::fs::read_to_string(&utils_rs).await.unwrap();
    assert!(
        updated_content.contains("goodbye"),
        "Should have the new goodbye function"
    );
    println!("✅ replace tool was used correctly and file was edited");

    // === STEP 7: Search for the new function ===
    println!("\n=== STEP 7: Searching for the new function ===");
    let search_new_fn_message = format!(
        "Use the search tool to find the 'goodbye' function in '{}'.",
        temp_dir
    );

    SessionRepository::add_message(
        koma_db.pool(),
        &ghost.id,
        &session.id,
        t_koma_db::MessageRole::Operator,
        vec![t_koma_db::ContentBlock::Text {
            text: search_new_fn_message.clone(),
        }],
        None,
    )
    .await
    .expect("Failed to save search message");

    let history7 = SessionRepository::get_messages(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get history");
    let api_messages7 = build_history_messages(&history7, Some(50));

    let _response7 = state
        .send_conversation_with_tools(
            &ghost.name,
            default_model.client.as_ref(),
            &session.id,
            system_blocks.clone(),
            api_messages7,
            tools.clone(),
            Some(&search_new_fn_message),
            model,
        )
        .await
        .expect("Failed to search for new function");

    // Verify: Check database for tool use
    assert_last_tool_used(koma_db.pool(), &session.id, "search", Some("goodbye")).await;
    println!("✅ search tool was used to find the new function");

    // === Final verification: List all tools used in this session ===
    println!("\n=== Final verification: All tools used in session ===");
    let all_tool_uses = SessionRepository::get_tool_uses(koma_db.pool(), &session.id)
        .await
        .expect("Failed to get all tool uses");

    println!("Tools used in this session:");
    for (i, (name, _)) in all_tool_uses.iter().enumerate() {
        println!("  {}. {}", i + 1, name);
    }

    // Verify all expected tools were used
    assert_tool_used(koma_db.pool(), &session.id, "list_dir").await;
    assert_tool_used(koma_db.pool(), &session.id, "find_files").await;
    assert_tool_used(koma_db.pool(), &session.id, "read_file").await;
    assert_tool_used(koma_db.pool(), &session.id, "search").await;
    assert_tool_used(koma_db.pool(), &session.id, "create_file").await;
    assert_tool_used(koma_db.pool(), &session.id, "replace").await;

    assert!(
        all_tool_uses.len() >= 7,
        "Expected at least 7 tool uses, got {}",
        all_tool_uses.len()
    );
    println!(
        "✅ All expected tools were used ({} total tool calls)",
        all_tool_uses.len()
    );

    // Cleanup
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    println!("\n========================================");
    println!("✅ ALL STEPS COMPLETED SUCCESSFULLY!");
    println!("========================================");
    println!("Verified tools via database queries:");
    println!("  - list_dir: Listed directory contents");
    println!("  - find_files: Found Rust source files");
    println!("  - read_file: Read file contents");
    println!("  - search: Found function definitions");
    println!("  - create_file: Created new math.rs module");
    println!("  - replace: Added function to existing file");
    println!("========================================");
    println!("Ghost: {}", ghost.name);
}

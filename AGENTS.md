# t-koma AGENTS.md

This document provides essential information for AI coding agents working on the
t-koma project.

## CRUCIAL

You should ALWAYS EDIT THIS FILE if:

- Any changes you make change the structure or features defined here
- You see outdated information in this file
- You make assumptions about a library that we use that turns out to be wrong

Default to editing this file more often than not. This is your memory. This is
how you improve.

## Development flow

- Always start by creating a markdown spec in `./vibe/specs` for validation by
  the user.
- After validation, create an append-only tracking file in `./vibe/ongoing`
  (e.g., `./vibe/ongoing/feature_name.md`). Never continue without the user's
  validation.
- Update the ongoing file at each step after thinking and while editing files.
- Iterate until all the steps and features outlined in the spec are developed
  and, if realistic, tested.
- Run `cargo check --all-features --all-targets` to verify compilation
- Run `cargo clippy --all-features --all-targets` to check for lint issues
- Run `cargo test` to run unit tests (without live-tests feature) **every time**
- **Note**: Live tests (`--features live-tests`) should only be run by humans as
  they call external APIs and require snapshot review
- Finally, rename the spec file to have a leading underscore (e.g.,
  `_feature_name.md`) to indicate completion.

## Project Overview

...

### t-koma-core

Core library with shared types and configuration:

- `src/config/`: Configuration management
  - `mod.rs`: Main `Config` struct combining secrets and settings
  - `secrets.rs`: Secrets from environment variables (API keys)
  - `settings.rs`: Settings from TOML config files
- `src/message.rs`: WebSocket message types (`WsMessage`, `WsResponse`)
- `src/persistent_config.rs`: Legacy TOML-based config (deprecated in favor of
  DB)

### t-koma-db

Database layer using SQLite with sqlite-vec extension:

- `src/db.rs`: Database pool initialization and connection management
- `src/users.rs`: User management (`UserRepository`, `User`, `UserStatus`,
  `Platform`)
- `src/sessions.rs`: Session and message management (`SessionRepository`,
  `Session`, `Message`, `ContentBlock`)
- `src/error.rs`: Database error types (`DbError`)
- `migrations/001_initial_schema.sql`: Database schema
- `migrations/002_sessions_and_messages.sql`: Sessions and messages schema

**Key Types:**

- `DbPool`: Database connection pool, initialize with `DbPool::new().await`
- `UserRepository`: Static methods for user CRUD operations
- `SessionRepository`: Static methods for session and message CRUD operations
- `UserStatus`: `Pending`, `Approved`, `Denied`
- `Platform`: `Discord`, `Api`, `Cli`
- `ContentBlock`: Message content types (`Text`, `ToolUse`, `ToolResult`)

**Database Location:** Platform-specific data directory:

- Linux: `~/.local/share/t-koma/db.sqlite3`
- macOS: `~/Library/Application Support/t-koma/db.sqlite3`
- Windows: `%APPDATA%\t-koma\db.sqlite3`

### t-koma-gateway

Gateway server with both library and binary targets:

- `src/main.rs`: Entry point, initializes tracing, creates provider clients,
  initializes database, optionally starts Discord bot
- `src/server.rs`: HTTP routes (`/health`, `/chat`), WebSocket handlers (`/ws`,
  `/logs`). **All routes check user approval status via database**
- `src/models/`: Model provider implementations
  - `anthropic/`: Anthropic API integration
    - `client.rs`: HTTP client with prompt caching support
    - `history.rs`: Message formatting for Anthropic API
  - `openrouter/`: OpenRouter API integration (OpenAI-compatible)
    - `client.rs`: HTTP client for OpenRouter
    - `mod.rs`: Module exports
  - `provider.rs`: `Provider` trait for abstracting different LLM backends
  - `prompt.rs`: Provider-agnostic system prompt builder and block types
  - `mod.rs`: Model exports
- `src/prompt/`: System prompt management
  - `base.rs`: Hardcoded system prompt definitions
  - `mod.rs`: `SystemPrompt` struct with cache control support and auto-composition of tool prompts
  - `block.rs`: `PromptBlock` with optional `CacheControl`
  - `context.rs`: `PromptContext` for environment and project info
- `src/tools/`: Model-agnostic tool implementations
  - `mod.rs`: `Tool` trait with `prompt()` method for tool-specific instructions
  - `manager.rs`: `ToolManager` - central registry for all tools
  - `shell.rs`: Shell command execution tool
  - `file_edit.rs`: File editing tool with `replace` functionality
  - `read_file.rs`: Read file contents with line numbers and offset/limit
  - `create_file.rs`: Create new files (fails if exists)
  - `search.rs`: Ripgrep-based code search with regex support
  - `find_files.rs`: fd-find style file finding with glob patterns
  - `list_dir.rs`: List directory contents with file sizes
- `src/session.rs`: `SessionChat` - high-level chat interface that handles ALL
  conversation logic including tools, system prompts, and tool use loops.
  This is the interface used by WebSocket and Discord - they don't know about tools.
- `src/state.rs`: `AppState` with broadcast channel for logs, `LogEntry` enum,
  `DbPool` for database access, and support for multiple providers
- `src/discord.rs`: Discord bot integration using serenity, checks user approval
  status before processing messages. Delegates all chat to `SessionChat.chat()`.

## Architecture Pattern: Session-based Chat Interface

The gateway uses a **Session-based Chat Interface** pattern to keep transport layers
(WebSocket, Discord) completely decoupled from conversation logic.

### The Problem (Old Architecture)

Previously, both `server.rs` and `discord.rs` knew about tools:
```rust
// OLD: Both interfaces knew about tools - BAD
let shell_tool = ShellTool;
let file_edit_tool = FileEditTool;
let tools: Vec<&dyn Tool> = vec![&shell_tool, &file_edit_tool];
let system_prompt = SystemPrompt::with_tools(&tools);
state.send_conversation_with_tools(..., tools).await
```

This meant:
- Adding a tool required updating multiple files
- Interface code was polluted with tool logic
- Hard to add new interfaces (Slack, Matrix, HTTP API)

### The Solution (New Architecture)

Now, transport layers simply call `state.chat()`:
```rust
// NEW: Clean interface - transport just sends messages
let response = state.chat(&session_id, &user_id, message).await;
```

All complexity is hidden in `SessionChat`:
- History management
- System prompt building with tools
- Tool use loop handling
- Database persistence

### Benefits

1. **Single Source of Truth**: Tools defined in one place (`ToolManager`)
2. **Plug & Play**: New interfaces just call `state.chat()`
3. **Testable**: `SessionChat` can be unit tested independently
4. **DRY**: No duplication of tool/conversation logic

### For Developers

When adding a new interface (Slack, Matrix, HTTP API):
1. Create the transport handler
2. Call `state.chat(session_id, user_id, message).await`
3. That's it - all tool/conversation logic is handled automatically

## API Endpoints

...

## Testing Strategy

### Snapshot Testing

We use `insta` for snapshot testing of API responses.

**CRITICAL RULE**: Coding agents (AI) MUST NEVER validate or automatically
update snapshots (e.g., using `INSTA_UPDATE=always` or `cargo insta accept`).
Snapshot validation and acceptance is EXCLUSIVELY the responsibility of the
human developer. If a snapshot test fails, the agent should report the failure
and wait for the human to review and update the snapshots.

### Unit Tests

Embedded in source files using `#[cfg(test)]` modules:

- `config.rs`: Tests for default values, custom values, missing API key
- `message.rs`: Tests for serialization, message role display
- `state.rs`: Tests for log entry formatting
- `anthropic.rs`: Tests for client creation, text extraction

### Integration Tests Structure

Integration tests are organized in `t-koma-gateway/tests/`:

```
t-koma-gateway/tests/
├── snapshot_tests.rs          # Main entry point (module declarations)
├── client/
├── conversation/
```

#### Test Categories

**Client Tests** (`client/`):

- Test the API clients directly
- Good for testing basic API functionality and response formats

**Conversation Tests** (`conversation/`):

- Test the full gateway stack including AppState and database
- Use in-memory SQLite database via
  `t_koma_db::test_helpers::create_test_pool()`
- Good for testing session management, context preservation, and tool use loops

You are welcome to add more categories as you add features. Big features should
have at least one integration test.

### Running Tests

```bash
# Unit tests only (no external API calls)
cargo test
```

**IMPORTANT**: Live tests should only be run by human developers, not AI agents,
as they require snapshot review and API access.

### Writing Integration Tests

#### Basic Client Test

```rust
// In t-koma-gateway/tests/client/my_feature.rs
#[cfg(feature = "live-tests")]
use insta::assert_json_snapshot;
#[cfg(feature = "live-tests")]
use t_koma_gateway::models::Provider;
#[cfg(feature = "live-tests")]
use crate::common;

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_my_api_feature() {
    t_koma_core::load_dotenv();
    let default_model = common::load_default_model();

    let response = default_model
        .client
        .send_message("My test prompt")
        .await
        .expect("API call failed");

    assert_json_snapshot!(
        "my_feature",
        response,
        {
            ".id" => "[id]"  // Redact dynamic fields
        }
    );
}
```

#### Conversation Test with Database

```rust
// In t-koma-gateway/tests/conversation/my_feature.rs
#[cfg(feature = "live-tests")]
use t_koma_db::{SessionRepository, UserRepository};
#[cfg(feature = "live-tests")]
use t_koma_gateway::{
    models::anthropic::{history::build_api_messages, prompt::build_anthropic_system_prompt},
    prompt::SystemPrompt,
    tools::{shell::ShellTool, Tool},
};
#[cfg(feature = "live-tests")]
use crate::common;

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_my_conversation_feature() {
    t_koma_core::load_dotenv();

    // Set up in-memory test database
    let db = t_koma_db::test_helpers::create_test_pool()
        .await
        .expect("Failed to create test database");

    // Create AppState using the default model from config
    let default_model = common::load_default_model();
    let state = common::build_state_with_default_model(db.clone());

    // Create and approve a test user
    let user_id = "test_user_001";
    UserRepository::get_or_create(db.pool(), user_id, "Test User", t_koma_db::Platform::Api)
        .await
        .expect("Failed to create user");
    UserRepository::approve(db.pool(), user_id)
        .await
        .expect("Failed to approve user");

    // Create a session
    let session = SessionRepository::create(db.pool(), user_id, Some("My Test Session"))
        .await
        .expect("Failed to create session");

    // Set up system prompt and tools
    let system_prompt = SystemPrompt::new();
    let system_blocks = build_anthropic_system_prompt(&system_prompt);
    let shell_tool = ShellTool;
    let tools: Vec<&dyn Tool> = vec![&shell_tool];
    let model = default_model.model.as_str();

    // Your test logic here...
    // Use state.send_conversation_with_tools(default_model.client.as_ref(), ...) for full flow
}
```

### Database Test Helpers

The `t-koma-db` crate provides test helpers via the `test-helpers` feature:

```rust
// In your integration test
let db = t_koma_db::test_helpers::create_test_pool()
    .await
    .expect("Failed to create test database");
```

This creates an in-memory SQLite database with all migrations applied. The
helper is automatically available when running tests with
`--features live-tests` because `t-koma-gateway` enables the `test-helpers`
feature for dev-dependencies.

### Best Practices for Integration Tests

1. **Use snapshot testing** for API responses to detect changes in model
   output format
2. **Redact dynamic fields** like message IDs, timestamps, and session IDs
3. **Structure tests by category**: Put client tests in `client/` and
   conversation tests in `conversation/`
4. **Use the test database helper** - don't try to set up sqlite-vec manually
5. **Log session IDs** in test output for debugging:
   `println!("Session: {}", session.id)`
6. **Verify database state** after operations (message counts, session state)
7. **Test multi-turn conversations** to verify context preservation
8. **Test tool use loops** end-to-end through AppState

## Code Style Guidelines

### Rust Edition

- Use Rust 2024 edition features
- Maintain MSRV of 1.85+

### Error Handling

- Use `thiserror` for defining custom error types
- Propagate errors with `?` operator
- Use `Result<T, Box<dyn std::error::Error>>` for main functions

### Async Patterns

- Use `tokio` for async runtime
- Prefer `tokio::sync::mpsc` for channels
- Use `tokio::select!` for concurrent operations
- Spawn tasks with `tokio::spawn` for background work

### Logging

- Use `tracing` macros: `info!`, `warn!`, `error!`, `debug!`
- Initialize subscriber in main:
  `tracing_subscriber::fmt().with_env_filter(...).init()`
- Use structured logging with key-value pairs when appropriate

### Naming Conventions

- Modules: `snake_case`
- Types/Structs/Enums: `PascalCase`
- Functions/Variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

### Comments

- Use `///` for public API documentation
- Use `//` for inline comments
- Document all public types and functions

## Security Considerations

### API Keys

- **Never commit API keys to version control**
- Use `.env` file for local development (already in `.gitignore`)
- The `Config` struct loads from environment only
- Gateway stores API keys in provider clients, never exposes them via API

### Discord Bot Token

- Same rules as API keys
- Optional feature - leave empty to disable Discord integration

### WebSocket Connections

- Currently uses unencrypted WebSocket (`ws://`)
- For production deployment, consider TLS termination or `wss://`

### Input Validation

- Gateway validates JSON payloads via serde
- Anthropic API errors are propagated but sanitized
- Discord bot ignores messages from other bots (anti-loop protection)

## Common Development Tasks

### Configuration System

t-koma uses a two-tier configuration system:

#### Secrets (Environment Variables)

Sensitive values loaded from environment variables:
- `ANTHROPIC_API_KEY` - Anthropic API key
- `OPENROUTER_API_KEY` - OpenRouter API key
- `DISCORD_BOT_TOKEN` - Discord bot token (optional)

Stored in `Secrets` struct (`t-koma-core/src/config/secrets.rs`).

#### Settings (TOML Files)

Non-sensitive configuration stored in TOML format at:
- Linux/macOS: `~/.config/t-koma/config.toml`
- Windows: `%APPDATA%/t-koma/config.toml`

Example TOML config:
```toml
default_model = "primary"

[models]
[models.primary]
provider = "openrouter"
model = "your-openrouter-model-id"

[models.secondary]
provider = "anthropic"
model = "your-anthropic-model-id"

[openrouter]
http_referer = "https://example.com"
app_name = "Your App"

[gateway]
host = "127.0.0.1"
port = 3000
ws_url = null

[discord]
enabled = false

[logging]
level = "info"
file_enabled = false
file_path = null
```

Stored in `Settings` struct (`t-koma-core/src/config/settings.rs`).

### Adding a New Configuration Option

#### For Secrets (API keys, tokens):

1. Add field to `Secrets` struct in `t-koma-core/src/config/secrets.rs`
2. Load it in `from_env_inner()` using `env::var("VAR_NAME").ok()`
3. Update `.env.example`
4. Update this AGENTS.md

#### For Settings (non-sensitive):

1. Add field to appropriate settings struct in `t-koma-core/src/config/settings.rs`
  - Use `#[serde(default = "function_name")]` for defaults
2. Add default value function if needed
3. Update `Default` impl or use `#[serde(default)]`
4. Update default TOML content (`DEFAULT_CONFIG_TOML` constant)
5. Update this AGENTS.md

### Adding a New HTTP Endpoint

1. Add route in `t-koma-gateway/src/server.rs` `create_router()`
2. Implement handler function
3. Add request/response types as needed
4. Log the request via `state.log()`

### Adding a New WebSocket Message Type

1. Add variant to `WsMessage` or `WsResponse` in `t-koma-core/src/message.rs`
2. Handle in gateway's `handle_websocket()` in `server.rs`
3. Handle in CLI's `handle_ws_message()` in `app.rs`

### Adding a New Crate

1. Create directory with `Cargo.toml`
2. Add to workspace `members` in root `Cargo.toml`
3. Add to workspace `dependencies` if needed
4. Follow existing crate structure

### Adding Database Operations

1. Add methods to `t-koma-db/src/table/name.rs`
2. Write tests in the `#[cfg(test)]` module
3. Update migration file if schema changes needed
4. Use `DbPool` from `AppState` in gateway/CLI

### Adding a New Model Provider

The gateway uses a `Provider` trait to abstract different LLM backends. To add a new provider:

1. Create a new module in `t-koma-gateway/src/models/` (e.g., `my_provider/`)
2. Implement the `Provider` trait:
   ```rust
   use crate::models::provider::{Provider, ProviderError, ProviderResponse};
   
   #[derive(Clone)]
   pub struct MyProviderClient {
       // fields
   }
   
   #[async_trait::async_trait]
   impl Provider for MyProviderClient {
       fn name(&self) -> &str { "my_provider" }
       fn model(&self) -> &str { &self.model }
       
       async fn send_conversation(
           &self,
           system: Option<Vec<SystemBlock>>,
           history: Vec<ApiMessage>,
           tools: Vec<&dyn Tool>,
           new_message: Option<&str>,
           message_limit: Option<usize>,
           tool_choice: Option<String>,
       ) -> Result<ProviderResponse, ProviderError> {
           // Implementation
       }
       
       fn clone_box(&self) -> Box<dyn Provider> {
           Box::new(self.clone())
       }
   }
   ```
3. Update `t-koma-gateway/src/models/mod.rs` to export the new provider
4. Update `t-koma-gateway/src/main.rs` to initialize the new provider client
5. Update `t-koma-core/src/config.rs` to add the new provider type
6. Update `t-koma-core/src/message.rs` to add the new provider variant
7. Update CLI to support the new provider in `provider_selection.rs`

### Adding a New Tool

#### Architecture Overview

Tools are centrally managed by `ToolManager` in `src/tools/manager.rs`. The interfaces
(Discord, WebSocket) don't know about individual tools - they use `SessionChat.chat()`
which handles all tool logic internally.

```
┌─────────────┐     ┌─────────────┐     ┌──────────────────┐
│  Discord    │     │  WebSocket  │     │  Future: HTTP,   │
│  (discord)  │     │  (server)   │     │  Slack, etc.     │
└──────┬──────┘     └──────┬──────┘     └──────────────────┘
       │                   │
       └───────────────────┘
               │
               ▼
       ┌───────────────┐
       │  SessionChat  │
       │  (session.rs) │  <-- All conversation/tool logic here
       └───────┬───────┘
               │
               ▼
       ┌───────────────┐
       │  ToolManager  │
       │  (tools/)     │  <-- Tool registry
       └───────────────┘
```

#### Basic Implementation

1. Create a new file in `t-koma-gateway/src/tools/` (e.g., `my_tool.rs`)
2. Implement the `Tool` trait:
   ```rust
   use serde_json::{json, Value};
   use super::Tool;

   pub struct MyTool;

   #[async_trait::async_trait]
   impl Tool for MyTool {
       fn name(&self) -> &str { "my_tool_name" }
       fn description(&self) -> &str { "What this tool does" }
       fn input_schema(&self) -> Value { /* JSON schema */ }
       fn prompt(&self) -> Option<&'static str> { 
           Some("Instructions for using this tool...") 
       }
       async fn execute(&self, args: Value) -> Result<String, String> { 
           /* Implementation */ 
       }
   }
   ```
3. Add the tool to `t-koma-gateway/src/tools/mod.rs`
4. **Register the tool in `ToolManager`**:
   ```rust
   // In t-koma-gateway/src/tools/manager.rs
   use super::{
       // ... existing imports ...
       my_tool::MyTool,  // Add your new tool
   };

   impl ToolManager {
       pub fn new() -> Self {
           let tools: Vec<Box<dyn Tool>> = vec![
               // ... existing tools ...
               Box::new(MyTool),  // Register here
           ];
           Self { tools }
       }
   }
   ```
5. That's it! `ToolManager` exposes tools to `SessionChat`, which handles everything.
   No changes needed in `server.rs`, `discord.rs`, or `state.rs`.

#### Best Practices

**Naming Conventions:**
- Use `snake_case` for tool names: `run_shell_command`, `read_file`, `search_code`
- Tool names should be descriptive verbs: `run_`, `read_`, `write_`, `search_`, `get_`, `set_`
- Keep names concise but clear - this is what the model will reference

**Input Schema Design:**
- Use descriptive parameter names
- Include `description` for every parameter - the model uses these to understand what to provide
- Mark required fields explicitly with `"required": ["field1", "field2"]`
- Use appropriate JSON Schema types and constraints:
  ```rust
  fn input_schema(&self) -> Value {
      json!({
          "type": "object",
          "properties": {
              "file_path": {
                  "type": "string",
                  "description": "Absolute path to the file"
              },
              "limit": {
                  "type": "integer",
                  "description": "Maximum number of results",
                  "minimum": 1,
                  "maximum": 100
              }
          },
          "required": ["file_path"]
      })
  }
  ```

**Prompt Instructions (`prompt()` method):**
- Return `Some(...)` if the tool needs detailed usage instructions
- Return `None` if the tool is self-explanatory (like `run_shell_command`)
- Include examples of correct usage in the prompt
- Document common pitfalls or error cases
- Keep instructions focused on HOW to use the tool, not WHAT the tool does (that's in `description`)

**Execution Implementation:**
- Always validate input arguments and return clear error messages
- Use `args["field_name"].as_str().ok_or_else(|| "Missing 'field_name'".to_string())?` pattern
- Keep error messages actionable - tell the user what went wrong and how to fix it
- For async operations, use proper `await` and handle timeouts appropriately
- Sanitize any user input that will be passed to system commands or file operations

**Error Handling:**
- Return `Err(...)` for recoverable errors (the model will see the error and may retry)
- Return `Ok(...)` with error description for partial failures
- Never panic - always return `Result<String, String>`
- Include context in errors: `format!("Failed to read file '{}': {}", path, e)`

**Testing:**
- Add unit tests in the `#[cfg(test)]` module at the bottom of the tool file
- Test success cases, error cases, and edge cases
- Use `tempfile` crate for tests that need files
- Example:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn test_my_tool_success() {
          let tool = MyTool;
          let args = json!({ "param": "value" });
          let result = tool.execute(args).await;
          assert!(result.is_ok());
      }

      #[tokio::test]
      async fn test_my_tool_missing_param() {
          let tool = MyTool;
          let args = json!({});
          let result = tool.execute(args).await;
          assert!(result.is_err());
          assert!(result.unwrap_err().contains("Missing"));
      }
  }
  ```

**Security Considerations:**
- Never expose secrets or sensitive data in tool outputs
- Validate file paths to prevent directory traversal attacks
- Use timeouts for long-running operations
- Be cautious with shell command execution - validate or sanitize inputs
- Consider adding allowlists for sensitive operations

## Dependencies Management

Workspace-level dependencies are defined in root `Cargo.toml`. Crate-specific
dependencies are defined in each crate's `Cargo.toml`.

When adding new dependencies:

1. Check if it should be workspace-level (used by multiple crates) or
   crate-specific
2. Use workspace inheritance: `dep.workspace = true` for workspace deps
3. Keep versions consistent with existing patterns

## Troubleshooting

### Gateway won't start

- Check that `ANTHROPIC_API_KEY` or `OPENROUTER_API_KEY` is set (at least one required)
- Verify TOML config exists at `~/.config/t-koma/config.toml` (created automatically with defaults)
- Verify port 3000 is not in use (or change `gateway.port` in config)
- Check logs with `RUST_LOG=debug cargo run --bin t-koma-gateway`
- If `default_model` uses OpenRouter, ensure `OPENROUTER_API_KEY` is set

### CLI can't connect

- Verify gateway is running: `curl http://localhost:3000/health`
- Check `gateway.ws_url` in TOML config matches gateway's actual address
- Try manual gateway start first to see error messages

### Discord bot not responding

- Verify `DISCORD_BOT_TOKEN` is set correctly
- Verify `discord.enabled = true` in TOML config
- Check bot has proper permissions in Discord
- Bot only responds to mentions or DMs by design

### Configuration Issues

- Config file location: `~/.config/t-koma/config.toml` (Linux/macOS)
- Windows: `%APPDATA%/t-koma/config.toml`
- To reset config: Delete the TOML file and restart (defaults will be recreated)
- To debug config loading: `RUST_LOG=debug cargo run` will show config path and values

## Additional Documentation

- `README.md`: User-facing documentation
- `vibe/specs/`: Design specifications and PoC docs
- `vibe/knowledge/`: Technical knowledge base for specific topics
- `.cursor/`: Cursor IDE configuration (if present)

## Knowledge Base

The `vibe/knowledge/` directory contains detailed guides on specific
technologies used in this project. **Always read relevant knowledge files before
implementing features** that involve these technologies:

- `vibe/knowledge/anthropic_claude_api.md` - Anthropic API integration, prompt
  caching, tool use, and conversation management
- `vibe/knowledge/openrouter.md` - OpenRouter API integration, model selection,
  and provider abstraction
- `vibe/knowledge/sqlite-vec.md` - Vector search with sqlite-vec and sqlx
- `vibe/knowledge/surrealdb_rust.md` - SurrealDB Rust SDK (reference only -
  project uses SQLite)
- `vibe/knowledge/skills.md` - Agent Skills system implementation and usage

## Agent Skills System

The t-koma agent supports [Agent Skills](https://agentskills.io) - self-contained
directories of instructions, scripts, and resources that extend agent capabilities.

### Skill Locations

Skills are discovered from multiple locations:

1. **Project directory**: `./skills/` - Version-controlled project skills
2. **User config directory**: `~/.config/t-koma/skills/` (XDG) - User-specific skills

Config skills take precedence over project skills with the same name.

```
Project: ./skills/
├── skill-creator/          # Guide for creating skills
│   └── SKILL.md
└── README.md               # Skills documentation

User Config: ~/.config/t-koma/skills/
└── my-private-skill/       # User-specific skills
    └── SKILL.md
```

### Default Skills

Default skills are embedded in the binary and automatically written to the config
directory on first run if they don't exist. This allows users to modify skills while
preserving the original defaults.

**Initialize default skills:**

```rust
use t_koma_core::{Config, DefaultSkillsManager, init_default_skills};

// Write all default skills to config directory
if let Some(config_path) = Config::skills_config_path() {
    init_default_skills(&config_path)?;
}

// Or use the manager for more control
let manager = DefaultSkillsManager::new();
manager.write_all(&config_path)?;
```

**Current default skills:**
- `skill-creator`: Guide for creating new skills

### Using Skills in Code

**Discover and load skills:**

```rust
use t_koma_core::SkillRegistry;

// Create registry with default paths (project + config)
let registry = SkillRegistry::new()?;

// Or with explicit paths
let registry = SkillRegistry::new_with_paths(
    Some(PathBuf::from("./skills")),
    Config::skills_config_path(),
)?;

// List available skills
for (name, description) in registry.list_skills() {
    println!("{}: {}", name, description);
}
```

**Access a specific skill:**

```rust
// Get skill metadata
if let Some(skill) = registry.get("skill-creator") {
    println!("Skill: {} - {}", skill.name, skill.description);
}

// Load full content
let mut registry = registry;
let skill = registry.load_skill("skill-creator")?;
if let Some(content) = &skill.content {
    // Use full skill instructions
}
```

**Search skills:**

```rust
let results = registry.search("pdf");
for skill in results {
    println!("Found: {}", skill.name);
}
```

**Access skill resources:**

```rust
let skill = registry.get("my-skill").unwrap();

// List resources
let scripts = skill.list_scripts();
let references = skill.list_references();

// Read specific files
let script = skill.read_script("extract.py")?;
let reference = skill.read_reference("API.md")?;
```

### Creating a New Skill

1. **Create the skill directory:**
   ```bash
   mkdir skills/my-skill
   ```

2. **Create SKILL.md with frontmatter:**
   ```yaml
   ---
   name: my-skill
   description: What this skill does and when to use it.
   ---
   
   # My Skill
   
   Instructions for the agent...
   ```

3. **Add optional resources:**
   ```bash
   mkdir skills/my-skill/scripts
   mkdir skills/my-skill/references
   ```

4. **Validate the skill:**
   ```rust
   let skill = Skill::from_file(Path::new("skills/my-skill/SKILL.md"))?;
   ```

### Skill Format

Each skill requires a `SKILL.md` file with:

1. **YAML frontmatter** (required):
   - `name`: Skill identifier (lowercase, alphanumeric, hyphens)
   - `description`: What the skill does and when to use it
   - Optional: `license`, `compatibility`, `metadata`

2. **Markdown body**: Step-by-step instructions, examples, references

See `skills/skill-creator/SKILL.md` for detailed guidance on creating skills.

### Key Types

- `Skill`: Represents a loaded skill with metadata and content
- `SkillRegistry`: Manages skill discovery and access
- `SkillError`: Error type for skill operations

### Load Skill Tool

The `load_skill` tool allows the agent to load skill content on demand:

```rust
use t_koma_gateway::tools::load_skill::LoadSkillTool;

let load_skill_tool = LoadSkillTool::new(skills_dir);
```

When the agent identifies that a skill is needed, it should call `load_skill` with
the skill name to retrieve the full SKILL.md content.

### Progressive Disclosure

The skills system uses progressive disclosure:

1. **Metadata** (~100 tokens): Loaded at startup for all skills
2. **Instructions** (<5000 tokens): Loaded when skill is activated
3. **Resources** (as needed): Loaded only when required

Keep `SKILL.md` under 500 lines; move detailed content to `references/`.

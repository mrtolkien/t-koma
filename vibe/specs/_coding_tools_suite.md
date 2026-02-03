# Coding Tools Suite Implementation

**Status**: Pending user validation

## Architecture Overview

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
       │  state.rs     │
       │  AppState     │
       │  session_chat │
       └───────┬───────┘
               │
               ▼
       ┌───────────────┐
       │  session.rs   │
       │  SessionChat  │
       │  .chat()      │  <-- ALL conversation logic here
       └───────┬───────┘
               │
       ┌───────┴───────┐
       ▼               ▼
┌─────────────┐  ┌─────────────┐
│ ToolManager │  │  Anthropic  │
│ (tools/)    │  │  Client     │
└─────────────┘  └─────────────┘
```

## Phase 1: Architecture Refactor (CRITICAL)

### Step 1: Create `t-koma-gateway/src/tools/manager.rs`
```rust
pub struct ToolManager { tools: Vec<Box<dyn Tool>> }
impl ToolManager {
    pub fn new() -> Self { /* register all tools */ }
    pub fn get_tools(&self) -> Vec<&dyn Tool>;
    pub async fn execute(&self, name: &str, input: Value) -> Result<String, String>;
}
```

### Step 2: Create `t-koma-gateway/src/session.rs` (NEW)
```rust
pub struct SessionChat { db: DbPool, anthropic: AnthropicClient, tool_manager: ToolManager }

impl SessionChat {
    /// THE ONLY INTERFACE Discord/server should use
    pub async fn chat(&self, session_id: &str, user_id: &str, message: &str) 
        -> Result<String, ChatError> 
    {
        // 1. Verify session ownership
        // 2. Save user message
        // 3. Get history
        // 4. Build system prompt with tools
        // 5. Call Claude with tool loop
        // 6. Save response
        // 7. Return text
    }
}
```

### Step 3: Update `t-koma-gateway/src/state.rs`
- Remove: `send_conversation_with_tools()`
- Remove: `execute_tool_by_name()`
- Remove: `response_has_tool_use()`
- Remove: `execute_tools_from_response()`
- Remove: `save_assistant_response()`
- Remove: `save_tool_results()`
- Remove: `finalize_response()`
- Add: `pub session_chat: SessionChat`

### Step 4: Simplify `t-koma-gateway/src/discord.rs`
**Remove ALL of this:**
```rust
// REMOVE:
use crate::tools::{shell::ShellTool, file_edit::FileEditTool, Tool};
let api_messages = crate::models::anthropic::history::build_api_messages(...);
let shell_tool = ShellTool;
let file_edit_tool = FileEditTool;
let tools: Vec<&dyn Tool> = vec![&shell_tool, &file_edit_tool];
let system_prompt = SystemPrompt::with_tools(&tools);
let system_blocks = build_anthropic_system_prompt(&system_prompt);
self.state.send_conversation_with_tools(&session.id, system_blocks, ...).await;
```

**Replace with:**
```rust
// Discord just sends messages!
match self.state.session_chat.chat(&session.id, &user_id, clean_content).await {
    Ok(response) => msg.channel_id.say(&ctx.http, &response).await,
    Err(e) => msg.channel_id.say(&ctx.http, "Error").await,
}
```

### Step 5: Simplify `t-koma-gateway/src/server.rs`
Same pattern - remove tool/system prompt logic, just call `state.session_chat.chat()`.

## Phase 2: New Tools

### Step 6: Create `t-koma-gateway/src/tools/read_file.rs`
### Step 7: Create `t-koma-gateway/src/tools/create_file.rs`
### Step 8: Create `t-koma-gateway/src/tools/search.rs` (grep crate)
### Step 9: Create `t-koma-gateway/src/tools/find_files.rs` (ignore crate)
### Step 10: Create `t-koma-gateway/src/tools/list_dir.rs`

### Step 11: Update `t-koma-gateway/src/tools/manager.rs`
Register all new tools in `new()` constructor.

## Phase 3: Dependencies

### Step 12: Update `t-koma-gateway/Cargo.toml`
```toml
grep = "0.3"
ignore = "0.4"
grep-regex = "0.1"
```

## Verification

- [ ] `cargo check --all-features --all-targets` passes
- [ ] `cargo clippy --all-features --all-targets` passes
- [ ] `cargo test` passes
- [ ] Discord compiles with NO tool imports
- [ ] server.rs compiles with NO tool imports
- [ ] Both interfaces use only `session_chat.chat()`

## Post-Implementation

- [ ] Update `AGENTS.md` with new architecture
- [ ] Rename spec to `_coding_tools_suite.md`

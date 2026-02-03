# Conversation Mode with Sessions, Prompt Caching & Tool Use

## Overview
Add real conversation mode to t-koma that persists message history per user in "sessions". Supports system prompts with **prompt caching**, **tool use**, and tracks which model generated each response.

## Goals
1. Store message history in database (one row per message) with model tracking
2. Store **tool uses and results** in database
3. Sessions (renamed from conversations) per user
4. Hardcoded system prompt in Rust `prompt` module with **prompt caching optimization**
5. Common prompt code in `gateway/prompt/`, provider-specific in `gateway/models/{provider}/`
6. Support message limiting when sending to API
7. **Tool use loop**: model → tool_use → execute → tool_result → model

## Prompt Caching Strategy

### For Anthropic Claude API

#### Minimum Cacheable Lengths
| Model | Min Tokens |
|-------|-----------|
| Claude Opus 4.5 | 4096 |
| Claude Sonnet 4.5/4, Opus 4.1/4 | 1024 |
| Claude Haiku 4.5 | 4096 |
| Claude Haiku 3.5/3 | 2048 |

#### Cache Structure for Conversations
```json
{
  "model": "claude-sonnet-4-5",
  "max_tokens": 4096,
  "system": [
    {
      "type": "text",
      "text": "<system instructions>"
    },
    {
      "type": "text",
      "text": "<static context if any>",
      "cache_control": {"type": "ephemeral"}
    }
  ],
  "messages": [
    {"role": "user", "content": "Hello"},
    {"role": "assistant", "content": "Hi there!"},
    {"role": "user", "content": "How are you?"},
    {
      "role": "assistant",
      "content": "I'm doing well!",
      "cache_control": {"type": "ephemeral"}
    }
  ]
}
```

#### Incremental Caching for Multi-turn
**Key Strategy**: Mark the LAST assistant message in history with `cache_control`:
- First request: Creates cache of system + full history
- Subsequent requests: Cache hit on system + history, only new user message processed
- The system auto-checks up to 20 blocks before breakpoint for cache hits

#### Multiple Cache Breakpoints (up to 4)
For complex scenarios, use breakpoints to separate:
1. **Tools** - rarely change
2. **System instructions** - rarely change  
3. **RAG/Context** - changes periodically
4. **Conversation history** - changes every turn

This allows independent invalidation - e.g., updating context doesn't invalidate tools cache.

#### Usage Tracking
Response includes:
```json
{
  "usage": {
    "input_tokens": 50,
    "cache_read_input_tokens": 10000,
    "cache_creation_input_tokens": 0,
    "output_tokens": 200
  }
}
```
- `cache_read_input_tokens`: Tokens retrieved from cache
- `cache_creation_input_tokens`: New tokens being cached now
- `input_tokens`: Tokens after last cache breakpoint (new content)

## Tool Use

### Tool Use Flow
```
1. User sends message
2. Claude responds with stop_reason="tool_use" and tool_use block(s)
3. Client executes tool(s) 
4. Client sends tool_result block(s) in new user message
5. Claude responds with final answer
```

### Tool Use Content Blocks

**tool_use** (from assistant):
```json
{
  "type": "tool_use",
  "id": "toolu_01A09q90qw90lq917835lq9",
  "name": "get_weather",
  "input": {"location": "San Francisco, CA", "unit": "celsius"}
}
```

**tool_result** (from user):
```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01A09q90qw90lq917835lq9",
  "content": "15 degrees",
  "is_error": false
}
```

### Parallel Tool Use
- Claude can call multiple tools in parallel
- All `tool_use` blocks in single assistant message
- All `tool_result` blocks in single user message
- Tool results must come FIRST in content array (before any text)

### Tool Definition Format
```json
{
  "name": "get_weather",
  "description": "Get the current weather in a given location",
  "input_schema": {
    "type": "object",
    "properties": {
      "location": {
        "type": "string",
        "description": "The city and state, e.g. San Francisco, CA"
      }
    },
    "required": ["location"]
  }
}
```

## Database Schema

### New Tables

```sql
-- Sessions table (per-user conversation containers)
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_active INTEGER DEFAULT 1 CHECK (is_active IN (0, 1)),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_updated_at ON sessions(updated_at);

-- Messages table (session history with tool support)
CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,  -- JSON array of content blocks
    model TEXT,  -- Which model generated this (NULL for user messages)
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_messages_session_id ON messages(session_id);
CREATE INDEX idx_messages_created_at ON messages(created_at);
```

### Content Block Storage
Content is stored as JSON array:

**Regular text message:**
```json
[{"type": "text", "text": "Hello!"}]
```

**Tool use message:**
```json
[
  {"type": "text", "text": "I'll check the weather."},
  {"type": "tool_use", "id": "toolu_01", "name": "get_weather", "input": {"location": "SF"}}
]
```

**Tool result message:**
```json
[
  {"type": "tool_result", "tool_use_id": "toolu_01", "content": "68°F", "is_error": false}
]
```

## Module Structure

### Common Prompt Module
```
t-koma-gateway/src/prompt/
├── mod.rs           # SystemPrompt struct, common formatting
└── base.rs          # Base prompt definitions, tool definitions
```

### Anthropic Module
```
t-koma-gateway/src/models/anthropic/
├── mod.rs           # Re-exports
├── client.rs        # HTTP client with prompt caching support
├── prompt.rs        # Anthropic-specific prompt building with cache_control
├── history.rs       # Read history and format for Anthropic API with caching
└── tools.rs         # Tool execution framework
```

### prompt/mod.rs
- `SystemPrompt` struct with base instructions
- Common formatting utilities
- `build_full_prompt(context: PromptContext) -> Vec<PromptBlock>`
- `PromptBlock` with optional `cache_control`

### prompt/base.rs
- Hardcoded base system prompt text
- Tool definitions (e.g., shell tool)

### models/anthropic/tools.rs
- `Tool` trait for executable tools
- `ToolRegistry` for managing available tools
- `execute_tool(name, input) -> ToolResult`
- Built-in tools: shell, file operations, etc.

### models/anthropic/prompt.rs
- `build_anthropic_system_prompt(base: &SystemPrompt) -> Vec<SystemBlock>`
- Anthropic-specific formatting with cache_control placement
- SystemBlock: `{type: "text", text: String, cache_control?: {...}}`

### models/anthropic/client.rs
- `AnthropicClient` struct
- `send_conversation(messages, system_blocks, tools, message_limit) -> Response`
- Error types
- Updated `MessagesRequest` to support system as array with cache_control
- **Tool use loop handling**

### models/anthropic/history.rs
- `build_api_messages(messages: Vec<Message>, limit: Option<usize>) -> Vec<ApiMessage>`
- Adds `cache_control` to last assistant message for incremental caching
- Convert internal message format to Anthropic's format
- **Handle tool_use and tool_result blocks**

## WebSocket Protocol

### WsMessage (Client → Gateway)
```rust
enum WsMessage {
    Chat { content: String },  // Uses active session
    ChatWithSession { session_id: String, content: String },
    ListSessions,
    CreateSession { title: Option<String> },
    SwitchSession { session_id: String },
    DeleteSession { session_id: String },
    Ping,
}
```

### WsResponse (Gateway → Client)
```rust
enum WsResponse {
    Response { 
        id: String, 
        content: String, 
        done: bool,
        usage: Option<UsageInfo>,  // includes cache metrics
    },
    ToolUse {  // NEW: Indicates tools need to be executed
        session_id: String,
        tool_uses: Vec<ToolUseRequest>,
    },
    SessionList { sessions: Vec<SessionInfo> },
    SessionCreated { session_id: String, title: String },
    SessionSwitched { session_id: String },
    SessionDeleted { session_id: String },
    Error { message: String },
    Pong,
}

struct ToolUseRequest {
    id: String,
    name: String,
    input: serde_json::Value,
}

struct UsageInfo {
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: Option<u32>,
    cache_creation_tokens: Option<u32>,
}

struct SessionInfo {
    id: String,
    title: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    message_count: i64,
    is_active: bool,
}
```

### Tool Use Flow via WebSocket
```
1. Client sends: WsMessage::Chat { content: "What's the weather?" }
2. Gateway detects tool_use in response
3. Gateway sends: WsResponse::ToolUse { tool_uses: [...] }
4. Client executes tools locally
5. Client sends: WsMessage::ToolResults { results: [...] }
6. Gateway sends final: WsResponse::Response { ... }
```

**Alternative**: Execute tools server-side and continue loop transparently.

## Implementation Plan

### Phase 1: Database Layer
1. Create migration for `sessions` and `messages` tables
2. Update messages.content to store JSON array (content blocks)
3. Create `SessionsRepository` with methods:
   - `create_session(user_id, title)`
   - `get_session(id)`
   - `get_active_session(user_id)`
   - `list_sessions(user_id)`
   - `switch_session(user_id, session_id)`
   - `delete_session(id)`
   - `add_message(session_id, role, content_blocks, model)`
   - `get_messages(session_id)` - ordered by created_at
   - `get_messages_with_limit(session_id, limit)` - most recent N

### Phase 2: Core Types
1. Update `WsMessage` enum with session commands and tool results
2. Update `WsResponse` enum with session responses + usage + ToolUse
3. Add `SessionInfo`, `UsageInfo`, `ToolUseRequest` structs
4. Add content block types (TextBlock, ToolUseBlock, ToolResultBlock)

### Phase 3: Prompt Module
1. Create `gateway/prompt/` directory
2. Create `SystemPrompt` struct with base instructions
3. Add `PromptBlock` struct with `cache_control` support
4. Add hardcoded base prompt
5. **Add tool definitions**

### Phase 4: Anthropic Module Refactor (WITH PROMPT CACHING & TOOLS)
1. Create `models/anthropic/` directory
2. Move existing client to `client.rs`
3. **Update `MessagesRequest` to support system as array with cache_control**
4. **Update `MessagesResponse` usage to include cache fields**
5. Create `models/anthropic/prompt.rs` with cache_control placement logic
6. Create `models/anthropic/history.rs` with incremental caching
7. **Create `models/anthropic/tools.rs` with tool execution framework**

### Phase 5: Gateway Layer
1. Update WebSocket handlers for session management
2. **Implement tool use loop** (model → tool_use → execute → tool_result → model)
3. Update HTTP /chat endpoint
4. Return usage info including cache metrics

### Phase 6: Discord Integration
1. Update Discord bot to use sessions
2. **Handle tool use in Discord context**

## Message Limiting with Caching

The `send_conversation` method accepts `message_limit: Option<usize>`:
- `None` - Send all messages (with cache_control on last assistant message)
- `Some(n)` - Send only the most recent n messages (still with cache_control on last)

**Note**: When limiting messages, we still place cache_control on the last assistant message in the trimmed history to enable caching.

## Edge Cases

1. **No active session**: Auto-create one with default title
2. **First message from user**: Create session automatically  
3. **Empty session**: Allow empty sessions, show placeholder title
4. **Deleted session**: Return error if trying to use deleted session
5. **Message limit > total**: Send all available messages
6. **Cache miss on short prompt**: If under min tokens, API ignores cache_control (handled gracefully)
7. **Concurrent access**: Use transactions when switching active session
8. **Tool use mid-conversation**: Properly resume after tool results
9. **Parallel tool use**: Handle multiple tool_use blocks in single response
10. **Tool error**: Set `is_error: true` in tool_result, let Claude handle it

## Testing

1. Unit tests for `SessionsRepository`
2. Integration tests for WebSocket session flow
3. Test session switching and persistence
4. Test message limiting
5. **Test prompt caching structure in API requests**
6. **Test tool use loop (single and parallel)**

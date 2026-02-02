# Tools and Model Refactor Specification

## Overview

Refactor `t-koma-gateway` to support a more flexible model and tool architecture. Add a shell execution tool to allow the agent to execute system commands.

## Goals

1.  **Refactor Directory Structure**:
    *   Move `t-koma-gateway/src/anthropic.rs` to `t-koma-gateway/src/models/anthropic.rs`.
    *   Create a `t-koma-gateway/src/models/mod.rs` to expose the model client.
    *   Create `t-koma-gateway/src/tools/` directory for tool definitions.

2.  **Abstract Model Interface**:
    *   Currently, `AnthropicClient` is used directly. We will keep using it but move it to the new location.
    *   Update `server.rs` and `main.rs` to import from the new location.

3.  **Implement Tool System**:
    *   Create a `Tool` trait (or similar structure) that tools must implement.
    *   Tools should be passable to the Anthropic API.

4.  **Implement Shell Tool**:
    *   Create `t-koma-gateway/src/tools/shell.rs`.
    *   The tool should take a command string and execute it.
    *   It should return the stdout/stderr.
    *   **Security**: Since this is a local agent, we will allow shell execution, but we should be careful. For this PoC, we will just implement it directly.

5.  **Integrate Tools with Anthropic Client**:
    *   Update `AnthropicClient::send_message` (or create a new method) to accept a list of tools.
    *   Handle `tool_use` content blocks in the response.
    *   Execute the requested tool and send the result back to the model (in a loop or single turn for now). *Note: The spec says "Add a tool... include one live test". It doesn't explicitly mandate a full agent loop in the gateway yet, but to test it "live", the model needs to be able to call it.*

## Architecture

### Directory Structure

```
t-koma-gateway/
├── src/
│   ├── models/
│   │   ├── mod.rs
│   │   └── anthropic.rs (moved from src/anthropic.rs)
│   ├── tools/
│   │   ├── mod.rs
│   │   └── shell.rs
│   ├── main.rs
│   ├── server.rs
│   └── ...
```

### Shell Tool Definition

```rust
pub struct ShellTool;

impl ShellTool {
    pub fn name() -> &'static str {
        "run_shell_command"
    }

    pub fn description() -> &'static str {
        "Executes a shell command on the host system. Use with caution."
    }
    
    pub fn input_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute"
                }
            },
            "required": ["command"]
        })
    }

    pub async fn execute(command: &str) -> Result<String, String> {
        // ... execution logic ...
    }
}
```

### Anthropic Tool Integration

The `AnthropicClient` needs to be updated to support the `tools` parameter in the API request.

```rust
// In models/anthropic.rs

#[derive(Debug, Serialize)]
struct MessagesRequest {
    // ...
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}
```

## Implementation Steps

1.  **Move Files**: Move `anthropic.rs` to `models/anthropic.rs` and fix imports.
2.  **Create Tools Module**: Set up `src/tools/mod.rs` and `src/tools/shell.rs`.
3.  **Implement Shell Tool**: Write the logic for `run_shell_command`.
4.  **Update Anthropic Client**: Add support for tools in the request body.
5.  **Test Shell Tool**: Write a unit test for the shell tool.
6.  **Live Test**: Write a live integration test in `tests/snapshot_tests.rs` that asks Claude to list files (using the shell tool).

## Testing

*   **Unit Tests**: Test `ShellTool::execute` with simple commands like `echo hello`.
*   **Live Tests**:
    *   Ask Claude: "List the files in the current directory using the shell tool."
    *   Claude should return a `tool_use` block.
    *   (Optional for this pass, but good to have) The test should ideally intercept the tool use, execute it, and feed it back, OR just verify that Claude *tried* to use the tool. The requirement says "actually queries the claude API and does snapshot testing". Capturing the `tool_use` response is sufficient to prove the tool definition worked and the model understood it.

## Refinement on Live Test

To fully test "Add a tool... include one live test", checking the `tool_use` response block in a snapshot is a solid start. It proves:
1.  We sent the tool definition correctly.
2.  The model understood the intent and chose to use the tool.

We don't necessarily need to implement the full multi-turn execution loop in `AnthropicClient` right this second if it complicates the refactor, but `AnthropicClient` should at least be able to *send* the tool definitions.

However, the prompt implies "Refactor... src/tools should be tools, used in model calls".

I will prioritize:
1.  Refactoring structure.
2.  Adding Shell Tool implementation.
3.  Updating Anthropic Client to send tools.
4.  Snapshot test proving tool use.

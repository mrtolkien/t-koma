# Tooling Guide

Use this file when adding or modifying tools for the T-KOMA gateway.

## Architecture

Tools are registered in `t-koma-gateway/src/tools/manager.rs`. Transport layers
(Discord, WebSocket) do not manage tools directly.

## Steps

1. Create a new file in `t-koma-gateway/src/tools/`.
2. Implement the `Tool` trait.
3. Export it in `t-koma-gateway/src/tools/mod.rs`.
4. Register it in `ToolManager`.
5. Add unit tests in the tool module.

## Tool Context

- Tools execute with a `ToolContext` (ghost name, workspace root, cwd, allow flag).
- Local file and shell tools must resolve paths relative to `cwd` and enforce the workspace boundary.
- Use the `change_directory` tool to update `cwd`; leaving the workspace requires operator approval handled by the gateway.

## Example

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

## Best Practices

- Validate inputs and return actionable errors.
- Keep tool names short, verb-first, snake_case.
- Use timeouts for long-running operations.
- Never expose secrets or raw file contents unnecessarily.

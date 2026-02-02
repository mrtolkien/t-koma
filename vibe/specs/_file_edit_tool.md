# File Edit Tool Specification

## Goal
Add a reliable file editing tool to `t-koma-gateway` for use by AI agents.

## Implementation Details

### `t-koma-gateway/src/tools/file_edit.rs`
Implement a `FileEditTool` struct implementing the `Tool` trait.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "file_path": {
      "type": "string",
      "description": "The path to the file to modify"
    },
    "old_string": {
      "type": "string",
      "description": "The exact literal text to replace. Must match the file content exactly, including whitespace."
    },
    "new_string": {
      "type": "string",
      "description": "The new text to insert in place of old_string."
    },
    "expected_replacements": {
        "type": "integer",
        "description": "Number of replacements expected. Defaults to 1. Use when you want to replace multiple occurrences.",
        "minimum": 1
    }
  },
  "required": ["file_path", "old_string", "new_string"]
}
```

**Logic:**
1. Read the file at `file_path`.
2. Check if `old_string` exists in the file content.
   - Count occurrences.
   - Compare with `expected_replacements` (default 1).
   - If mismatch, return error with details (found X, expected Y).
3. Replace all occurrences of `old_string` with `new_string`.
4. Write the updated content back to `file_path`.
5. Return success message.

### `t-koma-gateway/src/tools/mod.rs`
Register the new module.

### `t-koma-gateway/src/prompt.rs` (New)
Create a prompt builder helper that includes instructions for using the file edit tool.

**Content:**
- Instructions on how to use `file_edit`.
- Emphasis on providing exact `old_string` with sufficient context.

## Testing
- Unit tests in `file_edit.rs`.
- Live test in `tests/snapshot_tests.rs` asking Claude to use the tool.

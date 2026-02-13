# Add a Tool

Tool count should stay small and purpose-driven. Add tools only when existing tools
cannot cover the workflow.

## Tool Surface Split

- **Chat toolset**: `ToolManager::new_chat(...)` for interactive GHOST sessions
- **Reflection toolset**: `ToolManager::new_reflection(...)` for autonomous reflection

Choose the right surface. Many write/admin tools belong only in reflection.

## Implementation Checklist

1. **Implement tool module.**
   - Add file under `t-koma-gateway/src/tools/`.
   - Follow existing tool input/output patterns and error handling.
   - Keep behavior deterministic and narrow in scope.

2. **Register tool.**
   - Update `t-koma-gateway/src/tools/mod.rs`.
   - Wire into `t-koma-gateway/src/tools/manager.rs` in the appropriate constructor.

3. **Add approval gates if needed.**
   - Use `ApprovalReason` in `t-koma-gateway/src/tools/context.rs`.
   - Follow two-phase approval pattern (`APPROVAL_REQUIRED` then re-exec on approval).
   - Reference: `t-koma-gateway/src/tools/reference_import.rs`.

4. **Preserve workspace safety.**
   - Keep path checks canonicalization-aware.
   - Never silently allow workspace escape.

5. **Update prompt/tool guidance when required.**
   - If tool behavior changes OPERATOR/GHOST expectations, update relevant prompt docs.

6. **Add tests.**
   - Unit tests in tool module for validation and edge cases.
   - Integration coverage where tool touches orchestration.

## Non-Negotiable Rules

- Do not add overlapping tools with ambiguous responsibilities.
- Administrative bulk operations belong in CLI/TUI, not GHOST-facing tools.
- Keep transport layers unaware of tool internals.

## Validation

```bash
just check
just clippy
just test
just fmt
```

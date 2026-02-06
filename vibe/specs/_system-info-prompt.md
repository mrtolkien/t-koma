# System Info In System Prompt (Static)

## Summary
Add a static, cached system info block (OS/version, CPU cores, RAM, GPU) to the system prompt. The info must be computed once at startup (or first use), be lightweight, and remain stable across repeated calls to preserve provider caching.

## Goals
- Include a `# System Info` section in the system prompt.
- Provide OS name + version, CPU cores, total RAM, and GPU info (best-effort).
- Ensure the info is computed once and reused (no per-call probes).
- Keep the collection lightweight and fast.

## Non-Goals
- Real-time updates or dynamic stats.
- Detailed hardware inventory beyond the listed fields.
- Heavy dependencies or platform-specific external commands.

## Approach
- Add a small system info module in `t-koma-gateway` that gathers best-effort data using lightweight crates.
- Compute the info once in `SessionChat::new` and store it in `SessionChat`.
- Add a `system_info` field to `GhostContextVars`, pass it to template variables, and render it in `ghost-context.md`.

## Output Format
```
# System Info
- OS: <name> <version>
- CPU Cores: <count>
- RAM: <total in GB>
- GPU: <vendor/model or "unknown">
```

## Tests
- Update any system prompt tests that assert fixed prompt text.
- No snapshot updates.

## Acceptance
- System prompt includes the System Info section.
- System info is stable across multiple chat calls in the same process.
- No heavy runtime cost per request.

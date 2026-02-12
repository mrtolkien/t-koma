# Developer Specs

This folder holds implementation guides that are too detailed for `AGENTS.md`.

Use `AGENTS.md` for universal rules. Use these docs for feature work:

- `docs/dev/add-provider.md`: add a new LLM provider backend.
- `docs/dev/add-interface.md`: add a new operator interface/transport.
- `docs/dev/add-tool.md`: add or modify ghost/reflection tools.
- `docs/dev/prompts-and-messages.md`: add/update prompt and message content.
- `docs/dev/mcp-usage.md`: MCP usage rules and preferred tooling order.
- `docs/dev/background-jobs.md`: heartbeat + reflection lifecycle and persistence.
- `docs/dev/knowledge-system.md`: scopes, tools, storage, and indexing model.
- `docs/dev/multi-model-fallback.md`: model chain config, circuit breaker, and fallback
  loop.

If you change behavior covered by one of these guides, update that file in the same PR.

# t-koma

t-koma (ティーコマ) is a Rust-based AI gateway system with multi-provider model support.
It consists of a deterministic gateway server and a terminal UI client.

## What is t-koma?

t-koma is an AI system that lets you run personal AI agents ("ghosts") with persistent
memory, background knowledge curation, and multi-provider model fallback. Each ghost has
its own workspace and knowledge base, with ghost-scoped data in the unified database.

The system is composed of five crates:

| Crate              | Purpose                                                       |
| ------------------ | ------------------------------------------------------------- |
| `t-koma-gateway`   | Main server: providers, chat orchestration, tools, transports |
| `t-koma-cli`       | Terminal UI client (ratatui-based)                            |
| `t-koma-core`      | Shared types, config, WebSocket message schema                |
| `t-koma-db`        | SQLite layer for operators, ghosts, interfaces, sessions      |
| `t-koma-knowledge` | Knowledge and memory indexing/search                          |

## Key Features

- **Multi-provider models** with automatic fallback and circuit breaker
- **Persistent knowledge** with notes, references, diary, and embeddings search
- **Background jobs** for session health checks (heartbeat) and knowledge curation
  (reflection)
- **Multiple interfaces**: Discord bot and terminal UI
- **Per-ghost storage**: each ghost has its own workspace and ghost-scoped DB records
- **Tool system**: filesystem, web search/fetch, knowledge operations, and more

## How It Works

```text
┌─────────────┐     WebSocket      ┌─────────────┐     HTTP      ┌────────────┐
│  t-koma-cli │ ◄────────────────► │  t-koma     │ ◄───────────► │ Provider   │
│   (TUI)     │                    │  gateway    │               │    APIs    │
└─────────────┘                    └──────┬──────┘               └────────────┘
                                          │
┌─────────────┐     Discord API    ┌──────┴──────┐
│   Discord   │ ◄────────────────► │  Discord    │
│   Users     │                    │  Transport  │
└─────────────┘                    └─────────────┘
```

The gateway server handles all chat orchestration, tool execution, and provider
communication. Transport layers (TUI, Discord) only handle message delivery — they never
manage tools or chat logic directly.

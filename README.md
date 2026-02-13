# T-KOMA

Deterministic AI gateway in Rust, with persistent agent workspaces, long-term knowledge
system, and multi-model management.

## Status

This project is in an extremely early stage and is experimental. Expect breaking changes
and rough edges.

Only one runtime path is currently supported: run the compiled binaries directly.

## Overview

T-KOMA is built for people who want an agent they can run, inspect, and evolve over
time, instead of a stateless chat window.

## Why T-KOMA

- Strong separation of concerns: transport, orchestration, providers, tools
- Persistent memory model: notes, references, diary, embeddings search
- Background maintenance: heartbeat and reflection jobs with full job transcripts
- Multi-provider model chains with circuit-breaker fallback
- Local-first operational model: SQLite, filesystem workspaces, plain config. While
  strong models are still too big to run locally, at least you are in control of your
  data.

## What You Run

- `t-koma-gateway`: gateway server (providers, sessions, tools, scheduling)
- `t-koma-cli`: terminal UI client

## Quick Start

### Prerequisites

- Rust `1.85+`
- At least one provider API key (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`,
  `GEMINI_API_KEY`, `KIMI_API_KEY`, or `OPENAI_API_KEY` for openai-compatible endpoints)

### Build

```bash
git clone https://github.com/tolki/t-koma
cd t-koma
cargo build --release
cp .env.example .env
# edit .env with your keys
```

### Run

Run gateway + TUI from the compiled binaries:

```bash
./target/release/t-koma-gateway
./target/release/t-koma-cli
```

## OPERATOR and GHOST Flow

1. First contact asks `new` vs `existing` OPERATOR.
2. New OPERATOR flow is implemented: approve the OPERATOR from the TUI.
3. Existing-OPERATOR linking is not fully implemented yet.
4. Approved OPERATORS can create and chat with GHOSTS.

## Configuration

Config file:

- Linux: `~/.config/t-koma/config.toml`
- macOS: `~/Library/Application Support/t-koma/config.toml`
- Windows: `%APPDATA%/t-koma/config.toml`
- Override root with `T_KOMA_CONFIG_DIR`

Data root:

- Linux: `~/.local/share/t-koma/`
- macOS: `~/Library/Application Support/t-koma/`
- Override root with `T_KOMA_DATA_DIR`

Core data paths:

- `koma.sqlite3`: unified DB (OPERATORS, GHOSTS, interfaces, sessions, messages, usage,
  job logs)
- `ghosts/<name>/`: GHOST workspace
- `shared/`: shared knowledge files and index

Example `config.toml`:

```toml
default_model = "primary"
heartbeat_model = ["fallback", "primary"]

[models]
[models.primary]
provider = "openrouter"
model = "anthropic/claude-sonnet-4-5-20250929"
routing = ["anthropic"]

[models.fallback]
provider = "openai_compatible"
model = "your-local-model-id"
base_url = "http://127.0.0.1:8080"

[gateway]
host = "127.0.0.1"
port = 3000

[heartbeat_timing]
idle_minutes = 4
check_seconds = 60
continue_minutes = 30
```

## API Surface

- `GET /health`
- `WS /ws` (interactive session transport)
- `WS /logs` (log streaming)

## Docs (mdBook)

```bash
just doc         # build docs to docs/book
just doc-serve   # local live preview
```

## Development

```bash
just check
just clippy
just test
just fmt
```

Other useful commands:

```bash
just run-gateway
just run-cli
just ci
```

## Architecture Snapshot

```text
CLI / Discord / WS transports
            |
            v
      Session orchestration
            |
            v
      Provider abstraction
            |
            v
      External model APIs
```

Key rule: transports do not orchestrate chat/tool logic. `SessionChat` does.

## Reset Local State

```bash
# Linux
rm ~/.local/share/t-koma/koma.sqlite3

# macOS
rm ~/Library/Application\ Support/t-koma/koma.sqlite3
```

Knowledge and workspace files remain under `shared/` and `ghosts/` unless you remove
them.

# Installation

Status: T-KOMA is extremely early-stage and experimental. Only compiled binary execution
is supported right now.

## Prerequisites

- Rust 1.85+ (2024 edition)
- An API key for at least one supported provider

## Building from Source

```bash
# Clone the repository
git clone https://github.com/tolki/t-koma
cd t-koma

# Build release binaries
cargo build --release
```

This produces two binaries:

- `target/release/t-koma-gateway` — the gateway server
- `target/release/t-koma-cli` — the terminal UI client

## Development Tools

The project uses [just](https://just.systems/) as a task runner and
[dprint](https://dprint.dev/) for non-Rust formatting:

```bash
cargo install just
cargo install dprint
```

## Environment Setup

Create a `.env` file in the project root with your provider API keys:

```bash
cp .env.example .env
# Edit .env and add your provider API keys
```

Example `.env`:

```bash
ANTHROPIC_API_KEY=sk-ant-...
OPENROUTER_API_KEY=sk-or-...
OPENAI_API_KEY=sk-openai-...
GEMINI_API_KEY=...
```

Only the keys for providers you plan to use are required.

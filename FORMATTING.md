# Code Formatting Guide

T-KOMA uses automated formatters for consistent code style across all file types.

## Quick Start

```bash
# Format all files
just fmt

# Check formatting (CI)
just check-fmt
```

## Formatters Used

### Rust - `cargo fmt`
- **Standard**: Built-in Rust formatter (rustfmt)
- **Configuration**: Uses default rustfmt settings
- **Command**: `cargo fmt --all`
- **Files**: All `.rs` files

### Markdown, SQL, JSON, TOML - `dprint`
- **Tool**: [dprint](https://dprint.dev/) - Fast, pluggable formatter
- **Configuration**: `dprint.json`
- **Command**: `dprint fmt`
- **Files**: `.md`, `.sql`, `.json`, `.toml`

**Key Settings** (from `dprint.json`):
- **Line width**: 88 characters (consistent with Python Black)
- **Indent**: 2 spaces
- **Plugins**:
  - Markdown formatter
  - SQL formatter
  - JSON formatter
  - TOML formatter

## Available Commands

### Using Just (Recommended)

```bash
# Format everything
just fmt

# Format individual file types
just fmt-rust    # Only Rust (.rs)
just fmt-other   # Markdown, SQL, JSON, TOML

# Check formatting without modifying files
just check-fmt
just check-fmt-rust   # Only Rust
just check-fmt-other  # Only dprint files

# Full CI pipeline (format check + clippy + tests)
just ci
```

### Using scripts directly

```bash
# All-in-one script
./scripts/format.sh          # Format everything
./scripts/format.sh check    # Check formatting only
```

### Direct tool usage

```bash
# Rust
cargo fmt --all
cargo fmt --all -- --check  # Check only

# Markdown, SQL, JSON, TOML
dprint fmt
dprint check  # Check only
```

## IDE Integration

### VS Code

Install these extensions:
- **Rust**: rust-analyzer (includes rustfmt)
- **dprint**: [dprint VS Code extension](https://marketplace.visualstudio.com/items?itemName=dprint.dprint)

Add to `.vscode/settings.json`:
```json
{
  "editor.formatOnSave": true,
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  },
  "[markdown][json][toml]": {
    "editor.defaultFormatter": "dprint.dprint"
  }
}
```

### Neovim / Vim

With null-ls or conform.nvim:
```lua
require('conform').setup({
  formatters_by_ft = {
    rust = { "rustfmt" },
    markdown = { "dprint" },
    json = { "dprint" },
    toml = { "dprint" },
    sql = { "dprint" },
  },
})
```

## CI/CD Integration

Add to your CI pipeline:

```yaml
- name: Install dprint
  run: cargo install dprint

- name: Check formatting
  run: |
    cargo fmt --all -- --check
    dprint check
```

Or use the justfile:

```yaml
- name: Check formatting
  run: just check-fmt
```

Or use the convenience script:

```yaml
- name: Check formatting
  run: ./scripts/format.sh check
```

## Configuration Files

- **`dprint.json`**: dprint configuration for Markdown, SQL, JSON, TOML
- **`justfile`**: Task runner with format commands
- **`scripts/format.sh`**: All-in-one formatting script

## Installation

### dprint

dprint is a Rust tool installed via cargo:

```bash
cargo install dprint
```

Or download from: https://dprint.dev/install/

### just (optional, for convenience)

```bash
cargo install just
```

## Notes

- **Rust formatting** is enforced by cargo fmt and should always pass
- **dprint** handles multiple file formats with a single tool and configuration
- **Line width of 88**: Chosen for consistency with Python Black formatter
- **Git hooks**: Consider adding a pre-commit hook to run `just check-fmt`

## Excluded Paths

The following paths are excluded from dprint formatting (see `dprint.json`):

- `**/node_modules`
- `**/*-lock.json`
- `**/target`
- `**/.git`
- `**/.ralph/logs`

## Troubleshooting

### "dprint: command not found"

Install dprint:
```bash
cargo install dprint
```

### Formatting conflicts with editor

Ensure your editor uses the project's configuration files:
- `dprint.json` for dprint
- Default rustfmt settings for Rust

Disable competing formatters in your editor settings.

### "File not formatted" error in CI

Run locally to see the issue:
```bash
just check-fmt
```

Then format:
```bash
just fmt
```

Commit the formatted files.

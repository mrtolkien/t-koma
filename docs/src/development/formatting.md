# Code Formatting

T-KOMA uses automated formatters for consistent code style across all file types.

## Quick Start

```bash
# Format all files
just fmt
```

## Formatters

### Rust — `cargo fmt`

- Standard rustfmt with default settings
- Command: `cargo fmt --all`
- Files: all `.rs` files

### Markdown, SQL, JSON, TOML — `dprint`

- Tool: [dprint](https://dprint.dev/)
- Configuration: `dprint.json`
- Command: `dprint fmt`

Key settings (from `dprint.json`):

- Line width: 88 characters
- Indent: 2 spaces
- Plugins: markdown, SQL, JSON, TOML formatters

## Commands

```bash
# Format everything
just fmt

# Format individual file types
just fmt-rust    # Only Rust (.rs)
just fmt-other   # Markdown, SQL, JSON, TOML

# Full CI pipeline (format check + clippy + tests)
just ci
```

## IDE Integration

### VS Code

Install `rust-analyzer` and the
[dprint extension](https://marketplace.visualstudio.com/items?itemName=dprint.dprint),
then add to `.vscode/settings.json`:

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

### Neovim

With conform.nvim:

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

## Excluded Paths

`dprint.json` excludes: `**/node_modules`, `**/*-lock.json`, `**/target`, `**/.git`,
`**/.ralph/logs`.

#!/usr/bin/env bash
# Format all code in the project
# Usage: ./scripts/format.sh [check]

set -e

if [ "$1" = "check" ]; then
    echo "Checking formatting..."

    echo "→ Checking Rust code..."
    cargo fmt --all -- --check

    echo "→ Checking Markdown, SQL, JSON, and TOML files..."
    dprint check

    echo "✓ All files are properly formatted"
else
    echo "Formatting code..."

    echo "→ Formatting Rust code..."
    cargo fmt --all

    echo "→ Formatting Markdown, SQL, JSON, and TOML files..."
    dprint fmt

    echo "✓ All files formatted successfully"
fi

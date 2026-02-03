# Spec: Fix Build Errors After Merge

## Goal

Resolve all build errors introduced after merging changes into `main`, while
keeping behavior consistent with current requirements.

## Scope

- Compile errors across workspace crates.
- Test failures surfaced by `cargo test` (excluding live tests).
- Update docs/config notes if fixes change public behavior or structure.

## Non-Goals

- Refactors unrelated to build/test fixes.
- Running live tests (`--features live-tests`).

## Plan

1. Collect current errors with `cargo check --all-features --all-targets`.
2. Fix compile errors in priority order (core, db, gateway, cli).
3. Run `cargo clippy --all-features --all-targets` and fix lint regressions
   introduced by merge.
4. Run `cargo test` (no live tests) and fix failures.
5. Update AGENTS.md if new assumptions or structural changes are discovered.

## Validation

- `cargo check --all-features --all-targets` passes.
- `cargo clippy --all-features --all-targets` passes.
- `cargo test` passes.

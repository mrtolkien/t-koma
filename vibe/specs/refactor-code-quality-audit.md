# Refactor Spec: Code Quality + Discoverability Sweep

## Context

After rapid iteration, the workspace needs a quality-focused structural
refactor. Primary goals are discoverability, ownership boundaries, and
duplication reduction. Bug/security fixes are secondary but included when high
impact.

Scope update (2026-02-04):
- HTTP `/chat` should be removed (not migrated).
- CLI refactor items are out of scope in this effort (handled by another agent).

## Audit Method

- In-depth per-crate audit using dedicated sub-agents:
  - `t-koma-core`
  - `t-koma-db`
  - `t-koma-gateway`
  - `t-koma-cli`
- Cross-crate architecture audit for boundary/naming/flow consistency.

## High-Priority Findings

### P0 Correctness/Security

1. Session flow duplicates first user turn to provider in gateway chat path.
2. Shell tool enforcement is bypassable; lexical `cd` checks are not real
   sandboxing.
3. Path boundary checks can be escaped through symlinks.
4. HTTP `/chat` bypasses `SessionChat` orchestration and diverges from
   WS/Discord behavior.
5. `t-koma-core` has path traversal in skill file readers.
6. `t-koma-core` has config file collision risk (`PersistentConfig` and
   `Settings` target same TOML path with different schema).
7. `t-koma-db` contains FK lifecycle inconsistency (`operator_events` FK vs
   delete-then-log remove flow).

### P1 Structural/Discoverability

1. `t-koma-gateway/src/models` mixes provider clients with prompt code; rename
   toward `providers` and move prompts under dedicated `prompts` module.
2. Prompt logic is split (`prompt/*` and `models/prompt.rs`), with mixed storage
   strategy (hardcoded strings, runtime filesystem lookups, embedded markdown).
3. Transport-specific approval/session handling duplicated between Discord and
   WS.
4. Provider-neutral interfaces currently leak Anthropic-specific history types.
5. `t-koma-db` has duplicated pool/sqlite-vec bootstrap logic.
6. `t-koma-core` mixes protocol DTOs, runtime config IO, and skill runtime
   responsibilities.

### P2 Cleanup/Naming

1. Provider selection/config is stringly typed though provider enums already
   exist.
2. Duplicate role/session types exist across `core` and `db` with lossy
   conversion paths.

## Proposed Target Architecture

### Crate responsibilities

- `t-koma-core`: shared protocol + config types only.
- `t-koma-db`: typed DB access for KOMA DB and ghost DB with strict pool
  boundaries.
- `t-koma-gateway`: runtime orchestration (chat, providers, tools, interfaces).
- `t-koma-cli`: interface client(s) and admin commands.

### Gateway module layout

- `chat/`: session chat orchestration, history assembly, approval loop state
  machine.
- `providers/` (rename from `models/`): provider trait + anthropic/openrouter
  adapters.
- `prompts/`: prompt assets and composition/rendering, provider-independent
  entrypoint.
- `interfaces/`: ws/discord adapters, all calling shared chat/orchestration
  services.
- `tools/`: tool adapters and manager; inject shared services
  (web/search/fetch).
- `web/`: reusable web fetch/search/cache services only (no per-call tool
  wiring).

### Prompt strategy

- Canonical prompt source in markdown assets embedded at build time
  (`include_str!`) with optional override dir from config.
- Single `PromptComposer` API for provider-independent prompt construction.
- Tool/skill-specific prompt snippets colocated with tool modules when truly
  tool-specific.

## Planned Phases

### Phase 0: Safety + Behavior Alignment

- Fix duplicated first-turn message issue.
- Harden shell/path boundary logic.
- Route HTTP `/chat` through shared chat orchestration or explicitly split it as
  stateless endpoint.
- Remove HTTP `/chat`.
- Fix skill path traversal and config file collision in core.
- Add regression tests for each fix.

### Phase 1: Boundary Refactor

- Extract gateway `chat` module and centralize history + send loop.
- Rename `models` -> `providers`; move prompt rendering out of providers.
- Extract shared transport service for operator/interface/ghost/session
  resolution.
- Introduce provider-neutral history/message types.
- Consolidate db pool/bootstrap internals to reduce duplication.

### Phase 2: Discoverability + Naming Cleanup

- Normalize model/provider naming across core/gateway/cli.
- Unify duplicated role/session types (domain vs transport DTO separation).
- Final pass for file renames/module docs.

## Validation Criteria

- Each user-reported pain point has a clear destination module and ownership
  rule.
- WS/Discord chat entry points use one orchestration path.
- Prompt building has one canonical API and asset strategy.
- Provider internals are not leaking provider-specific types into shared traits.
- DB and CLI critical flows have regression tests for previously identified
  risks.

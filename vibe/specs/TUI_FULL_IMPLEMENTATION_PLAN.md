# T-KOMA TUI Full Spec - Implementation Plan

## Scope

Implement the `vibe/specs/TUI_FULL_SPEC.md` requirements in `t-koma-cli` as a
new multi-pane TUI mode that:

- Works without the gateway for all CRUD/config flows (direct DB/settings I/O)
- Uses WebSocket only for chat and live logs when gateway is available
- Supports external editor flow for config editing
- Replaces the current chat-only TUI mode

## Current Baseline

- Existing TUI is chat-only (`t-koma-cli/src/app.rs`, `t-koma-cli/src/ui.rs`).
- Admin and config flows are currently separate terminal modes (`admin.rs`,
  `model_config.rs`) with reusable business logic.
- Log follow mode exists (`log_follower.rs`) and can be adapted into Gate pane.

## Proposed Delivery Phases

## Phase 0: Foundation and App Shell

- Add a new app module set and migrate `run_chat_mode` to this app:
  - `t-koma-cli/src/tui/mod.rs`
  - `t-koma-cli/src/tui/app.rs`
  - `t-koma-cli/src/tui/layout.rs`
  - `t-koma-cli/src/tui/theme.rs`
  - `t-koma-cli/src/tui/state.rs`
- Implement 2-line header + three-pane main layout.
- Implement focus model (`Categories`, `Options`, `Content`) with keybinds:
  `h/l`, arrow keys, `Tab`, `Enter`.
- Add shared traits for pane behavior:
  - `PaneView` (render)
  - `PaneEvents` (key handling)
  - `PaneData` (refresh/load)

Acceptance:
- App renders skeleton with focus switching and no panics on resize.

## Phase 1: Categories and Navigation

- Define categories enum: `Gate`, `Config`, `Operators`, `Ghosts`, `Chat`.
- Build categories pane with selection + activation behavior.
- Implement options pane switching per category (none for Gate).
- Implement content pane renderer router.

Acceptance:
- Selecting category updates options and content reliably.

## Phase 2: Config Pane (Direct settings I/O)

- Reuse logic from `model_config.rs` in non-interactive callable helpers.
- Add actions:
  - `Add Model`
  - `Set Default`
  - `Toggle Discord`
  - `Edit in Editor` (via `$EDITOR` temp workflow with validation)
  - `Reload`
  - `Save`
- Add TOML content viewer (line-wrapped and scrollable).

Acceptance:
- Save/reload round-trip works and validates `Settings`.
- Invalid edited TOML is rejected, previous config stays active, and a backup
  config file is created with an in-TUI restore option.

## Phase 3: Operators Pane (Direct DB CRUD)

- Reuse `OperatorRepository` logic from `admin.rs` in pane controller.
- Actions:
  - `List All`
  - `Add Operator` (operator record only; no interface linkage; default status:
    `approved`)
  - `Pending Approvals` (approve/deny inline shortcuts)
- Add status/icon rendering and filtered list state.

Acceptance:
- Operator lifecycle operations persist in DB and refresh correctly.

## Phase 4: Ghosts Pane (Direct DB CRUD)

- Use `GhostRepository` and `InterfaceRepository` for listing/creation/deletion.
- Actions:
  - `List All`
  - `New Ghost`
  - `Delete` (two-step confirmation before destructive delete)
- Render model/provider metadata in content pane.

Acceptance:
- Create/delete updates DB and reflected list.

## Phase 5: Gate Pane (Runtime Controls + Logs)

- Add gateway status widget.
- Add restart action (`r`) wired through WebSocket control message (no local
  process spawning).
- Integrate log tail stream into pane content area.
- Add controls:
  - `/` search mode, `Esc` exit search
  - `Space` pause/resume
  - `c` clear
  - `1-3` source filters
- If gateway unavailable, show explicit degraded-mode state.
- Drop CLI-based gateway process spawning from TUI flow.

Acceptance:
- Live logs work when gateway is running; controls are functional.
- Restart works via WS control path when gateway is connected; degraded state
  shown when disconnected.

## Phase 5a: WS Restart Control Contract

- Extend shared protocol in `t-koma-core`:
  - `WsMessage::RestartGateway`
  - `WsResponse::GatewayRestarting` and/or `WsResponse::GatewayRestarted`
    plus `WsResponse::Error` fallback
- Implement gateway WS handler support for restart command.
- Keep transport-layer rule intact by delegating restart logic in gateway
  service/state layer, not in websocket adapter code.

## Phase 6: Chat Overlay (Deferred)

- Removed for now per current product direction ("chat to ghosts" through ghost
  flows first). Keep WS/session infrastructure ready for later reintroduction.

## Phase 7: Testing and Hardening

- Unit tests:
  - category/options navigation
  - focus transitions
  - pane event handling per category
- Integration tests:
  - settings load/save
  - DB connectivity and CRUD paths
  - enum/category correctness
- Run:
  - `cargo check --all-features --all-targets`
  - `cargo clippy --all-features --all-targets`
  - `cargo test`

Acceptance:
- All checks pass and no snapshot updates are performed.

## Cross-Cutting Design Notes

- Always use `block.inner(area)` for pane content.
- Keep gateway-independent behavior for Config/Operators/Ghosts.
- Keep WebSocket usage limited to Chat + Gate logs.
- Preserve architecture boundaries; do not move provider/tool logic into CLI.

## Validation Status

- Decisions confirmed:
  - Replace current TUI mode.
  - Strict editor validation with backup+restore flow.
  - Add operator creates record only, default status `approved`.
  - Ghost deletion requires two confirmations.
  - Gateway spawn from CLI dropped.
  - Gateway restart is a WS command.

# Add an Interface

This guide covers adding a new OPERATOR messaging interface/transport beyond the
existing Discord and WebSocket/CLI flows.

## Concept Reminder

- An `Interface` is `(platform, external_id)` tied to an OPERATOR in the `interfaces`
  table.
- Transport adapters do transport work only.
- Chat/tool orchestration must remain in `SessionChat` + `operator_flow`.

## Implementation Checklist

1. **Add platform enum support.**
   - Update `t-koma-db/src/operators.rs` (`Platform` enum + `Display` + `FromStr`).
   - Ensure any DB read fallback behavior remains sane in `interfaces.rs`.

2. **Ensure schema compatibility.**
   - If platform storage format changes, add SQL migration under
     `t-koma-db/migrations/`.
   - Keep `interfaces(platform, external_id)` uniqueness semantics.

3. **Add transport adapter module.**
   - Create/extend transport module under `t-koma-gateway/src/` (similar to `discord/`
     or `server.rs` WS handling).
   - Parse inbound interface payloads into gateway actions/messages.
   - Render outbound `GatewayMessage` payloads with interface-specific formatting.

4. **Route through existing orchestration.**
   - Use `operator_flow` and `SessionChat` for chat handling.
   - Do not re-implement tool loops, approval handling, or session lifecycle in the
     transport layer.

5. **Interface identity and OPERATOR binding.**
   - Resolve/create interface records using `InterfaceRepository`.
   - Preserve approval flow semantics (`pending`, `approved`, `denied`).

6. **Content and message rendering.**
   - Add interface-specific message variants in `t-koma-gateway/messages/en/*.toml` if
     needed.
   - Keep plaintext fallback behavior for non-rich renderers.

7. **Add onboarding flow in TUI.**
   - Clear onboarding TUI guiding the user through setup for this interface.

8. **Startup and runtime wiring.**
   - Update `t-koma-gateway/src/main.rs` to start new interface runtime if required.
   - Keep logging + lifecycle parity with existing interfaces.

## Non-Negotiable Rules

- No direct transport-to-provider calls for interactive chat.
- No bypass around `SessionChat`.
- Keep semantic `GatewayMessage` as the outbound contract.

## Validation

```bash
just check
just clippy
just test
just fmt
```

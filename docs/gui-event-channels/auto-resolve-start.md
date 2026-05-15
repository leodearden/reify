# Per-Channel Event Spec: `auto-resolve-start`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task δ (GR-016 Phase 2 proof slice).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §2.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `auto-resolve-start`
- **Rust emit site:** `gui/src-tauri/src/main.rs` — `TauriAutoResolveEmitter::start()` (calls `event_bus::emit_typed(&self.app, "auto-resolve-start", &())`)
- **TS listen site:** `gui/src/bridge.ts` — `onAutoResolveStart(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Payload is unit (`()`): no fields.

```rust
// No payload struct — emitted as `()`.
// Rust: emit_typed(&self.app, "auto-resolve-start", &())
```

```typescript
// No payload interface — callback receives no arguments.
// TS: listen<void>('auto-resolve-start', () => { callback(); })
```

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `gui/src-tauri/src/engine.rs:549-572` — `EngineSession::emit_auto_resolve_if_any`
- **Trigger:** fires once per `EngineSession::check()` call when the constraint solver resolves at least one auto parameter (i.e. `check.resolved_params` is non-empty). Precedes the matching `auto-resolve-iteration` and `auto-resolve-complete` events in the same synchronous call.
- **Frequency:** once per `EngineSession::{load_from_source, set_parameter, update_source}` call that produces a non-empty `resolved_params` result. In the current single-iteration-per-pass solver model this is at most once per user action.

> **Note on emit-trio synthesis location:** The start/iteration/complete trio is synthesised at the GUI layer (`emit_auto_resolve_if_any` in `engine.rs`) rather than inside the `reify-eval` orchestrator. This preserves the kernel-layer dependency direction — `reify-eval` has no Tauri dependency. A future iterative solver can split the three events onto distinct lifecycle points without changing the channel names. See also: design decision recorded in `docs/gui-event-channels.md` §2 for this channel.

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onAutoResolveStart(callback): Promise<UnlistenFn>`
- **Subscribing component/store:** `gui/src/stores/engineStore.ts` — `subscribeToEvents()` (wires `onAutoResolveStart` into the store's auto-resolve state reset path)
- **Unlisten lifecycle owner:** `engineStore.subscribeToEvents` — uses `Promise.allSettled` for batch registration; unlisten functions collected and called by the store's teardown / rollback path
- **Subscription pattern:** global via `engineStore`; `AutoResolvePanel` (`gui/src/panels/AutoResolvePanel.tsx`) reads derived store state rather than subscribing directly

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field). The unit payload carries no version-sensitive fields.

---

## 6. Error semantics

Default per PRD §5:

- **Malformed payload:** N/A — payload is `()` (unit); no fields to validate.
- **Emit failure:** `tracing::warn!` and continue; no surrounding operation failure (§5.3).
- **Missing emitter:** silent; defended by CI roundtrip gate (§5.1 + §6.1).

---

## 7. Test pointers

- **Rust emit-sequence test:** `gui/src-tauri/src/tests/engine_tests.rs:7102+`
  — `RecordingEmitter` captures the event sequence; `engine_session_auto_resolve_emitter_fires_start_iter_complete_when_solver_resolves` asserts `events[0]` matches `EmitEvent::Start`.
- **TS shape test:** existing `onAutoResolveStart` coverage in `gui/src/__tests__/bridge.test.ts`.
- **No separate roundtrip test needed:** payload is `()` — serde has nothing to round-trip.

# Per-Channel Event Spec: `auto-resolve-complete`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task δ (GR-016 Phase 2 proof slice).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §2.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `auto-resolve-complete`
- **Rust emit site:** `gui/src-tauri/src/main.rs` — `TauriAutoResolveEmitter::complete()` (calls `event_bus::emit_typed(&self.app, "auto-resolve-complete", &())`)
- **TS listen site:** `gui/src/bridge.ts` — `onAutoResolveComplete(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Payload is unit (`()`): no fields.

```rust
// No payload struct — emitted as `()`.
// Rust: emit_typed(&self.app, "auto-resolve-complete", &())
```

```typescript
// No payload interface — callback receives no arguments.
// TS: listen<void>('auto-resolve-complete', () => { callback(); })
```

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `gui/src-tauri/src/engine.rs:549-572` — `EngineSession::emit_auto_resolve_if_any`
- **Trigger:** fires once per `EngineSession::check()` call (via `emit_auto_resolve_if_any`) after the matching `auto-resolve-iteration` event when the solver finishes producing resolved values. Bounded by `check()`'s synchronous completion — the GUI can `wait_for_event('auto-resolve-complete')` with confidence per PRD §2.2 user-observable signal.
- **Frequency:** once per `EngineSession::{load_from_source, set_parameter, update_source}` call that produces a non-empty `resolved_params` result.

> **Note on emit-trio synthesis location:** Same rationale as `auto-resolve-start` — trio synthesised at the GUI layer to preserve the kernel-layer dependency direction. See `auto-resolve-start.md §3` note.

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onAutoResolveComplete(callback): Promise<UnlistenFn>`
- **Subscribing component/store:** `gui/src/stores/engineStore.ts` — `subscribeToEvents()` signals `AutoResolvePanel` (`gui/src/panels/AutoResolvePanel.tsx`) to stop the iteration counter / sparkline buffering.
- **Unlisten lifecycle owner:** `engineStore.subscribeToEvents` batch rollback path.
- **Subscription pattern:** global via `engineStore`.

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
  — `engine_session_auto_resolve_emitter_fires_start_iter_complete_when_solver_resolves` asserts `events[2]` matches `EmitEvent::Complete`.
- **TS shape test:** existing `onAutoResolveComplete` coverage in `gui/src/__tests__/bridge.test.ts`.
- **No separate roundtrip test needed:** payload is `()` — serde has nothing to round-trip.

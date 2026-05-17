# Per-Channel Event Spec: `warm-pool-event`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task ε (GR-016 Phase 3).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §2.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `warm-pool-event`
- **Rust emit site:** `gui/src-tauri/src/main.rs` — `TauriWarmPoolEventEmitter::emit()` (calls `event_bus::emit_typed(&self.app, "warm-pool-event", &event)`)
- **TS listen site:** `gui/src/bridge.ts` — `onWarmPoolEvent(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
/// IPC payload for the `warm-pool-event` Tauri channel (GR-016 ε).
///
/// Wire format per PRD §2.2. TS mirror: `gui/src/types.ts::WarmPoolEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarmPoolEvent {
    /// `"evicted"` when a warm state was evicted; `"donated"` when one was donated.
    pub kind: String,
    /// Warm-state size involved in the event, in bytes.
    pub size_bytes: u64,
    /// Stringified `NodeId` of the victim (evicted) or donor (donated) node.
    pub node_id: String,
}
```

Source: `gui/src-tauri/src/types.rs` — `WarmPoolEvent`.

Constructed via `WarmPoolEvent::from_engine_event(&ev)` which maps
`warm_pool::WarmPoolEvent::Evicted { node_id, size_bytes }` → `kind = "evicted"` and
`warm_pool::WarmPoolEvent::Donated { node_id, size_bytes }` → `kind = "donated"`,
casting `size_bytes: usize → u64` and stringifying `node_id` via the `NodeId` `Display` impl
(`crates/reify-eval/src/cache.rs`).

```typescript
/**
 * Payload for the `warm-pool-event` Tauri channel (GR-016 ε).
 *
 * Wire format per PRD §2.2: field names match the Rust IPC struct in
 * `gui/src-tauri/src/types.rs::WarmPoolEvent` exactly — no `serde(rename_all)`.
 */
export interface WarmPoolEvent {
  /** `'evicted'` when a warm state was evicted; `'donated'` when one was donated. */
  kind: 'evicted' | 'donated';
  /** Warm-state size involved in the event, in bytes. */
  size_bytes: number;
  /** Stringified `NodeId` of the victim (evicted) or donor (donated) node. */
  node_id: string;
}
```

Source: `gui/src/types.ts` — `WarmPoolEvent`.

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `gui/src-tauri/src/engine.rs` — `EngineSession::drain_and_emit_warm_pool_events()`, invoked after each engine call boundary (`check`, `edit_check`, `build`, `tessellate_snapshot`, etc.) at the same call sites as `emit_auto_resolve_if_any`.
- **Trigger:** warm pool donation or eviction during an eval/edit cycle. The engine method `Engine::drain_and_record_warm_pool_events()` drains `WarmStatePool::drain_events()`, records each as an `EvalEvent` on the `EventJournal` (wiring M-010), and returns the drained `Vec<WarmPoolEvent>` for the session layer to emit.
- **Frequency:** 0..N events per engine call boundary, bounded by `MAX_BUFFERED_EVENTS` auto-trim in `WarmStatePool`.

### Engine-layer translator contract (journal.rs:53–62)

The translator `translate_warm_pool_event_to_eval_event` preserves the victim/donor `node_id` contract:
- `WarmPoolEvent::Evicted { node_id, size_bytes }` → `EvalEvent { kind: EventKind::Evicted { size_bytes }, node_id: <victim>, ... }`
- `WarmPoolEvent::Donated { node_id, size_bytes }` → `EvalEvent { kind: EventKind::Donated { size_bytes }, node_id: <donor>, ... }`

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onWarmPoolEvent(callback): Promise<UnlistenFn>`
  - Uses a hand-shaped guard (`isPlainObject(p) && (p['kind'] === 'evicted' || p['kind'] === 'donated') && typeof p['size_bytes'] === 'number' && typeof p['node_id'] === 'string'`) rather than `validatePayload`. Rationale: `validatePayload` only validates string-typed fields and cannot verify the numeric `size_bytes`; this matches the `onAutoResolveIteration` precedent at `bridge.ts:620`.
  - Drops malformed payloads with `console.warn('[warm-pool-event] malformed payload; dropping event', p)`.
- **Subscribing component:** `gui/src/debug/WarmPoolDebugPanel.tsx` — subscribes via `onWarmPoolEvent` on mount, tracks evict/donate counts and last `node_id` via Solid signals.
- **Unlisten lifecycle owner:** Solid `onCleanup` in `WarmPoolDebugPanel` — `unlistenPromise.then(fn => fn())`.
- **Subscription pattern:** panel-local (not routed through `engineStore`).

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field).

---

## 6. Error semantics

Default per PRD §5:

- **Malformed payload:** `console.warn` + drop in `onWarmPoolEvent` (hand-shaped guard — see §4).
- **Emit failure:** `tracing::warn!` and continue; no surrounding operation failure (`TauriWarmPoolEventEmitter::emit` logs `"warm-pool-event emit failed: {}"` on error — `gui/src-tauri/src/main.rs`).
- **Missing emitter:** silent; `EngineSession::drain_and_emit_warm_pool_events` early-returns when `warm_pool_event_emitter` is `None`.

---

## 7. Test pointers

> Test pointers use symbol/function-name anchors rather than absolute line numbers (line ranges drift; symbol names are stable). Grep the cited file for the function name.

- **Rust serde roundtrip test:** `gui/src-tauri/src/tests/types_tests.rs`
  — `warm_pool_event_serializes_with_expected_field_set`: constructs `WarmPoolEvent { kind: "evicted", size_bytes: 1024, node_id: "Body.thickness" }`, serializes via `serde_json::to_value`, asserts exact JSON shape `{"kind":"evicted","size_bytes":1024,"node_id":"Body.thickness"}`. Also tests `from_engine_event` mapping (§6.1 gate).
- **Rust engine drain test:** `gui/src-tauri/src/tests/engine_tests.rs`
  — uses `RecordingWarmPoolEventEmitter` to assert that `EngineSession::drain_and_emit_warm_pool_events` emits events with correct `kind`/`size_bytes`/`node_id` after pre-populating the warm pool (§6.1 gate).
- **TS bridge shape test:** `gui/src/__tests__/bridge/warm-pool-event.test.ts`
  — `onWarmPoolEvent (GR-016 ε)` describe block: valid evicted/donated payloads forwarded to callback; malformed payloads (missing `size_bytes`, wrong type, `kind="banana"`) dropped with `console.warn`; unlisten removes handler (§6.2 + §6.3 gate).
- **Panel rendering test:** `gui/src/__tests__/WarmPoolDebugPanel.test.tsx`
  — `WarmPoolDebugPanel` describe blocks: initial zero counts, donated/evicted count increments, most-recent `node_id` display, unlisten-on-unmount.
- **Engine-side translator test:** `crates/reify-eval/src/journal.rs` tests module
  — `test_translate_warm_pool_event_evicted` / `test_translate_warm_pool_event_donated`: pin victim/donor `node_id` contract (journal.rs:53–62).
- **Engine drain+record test:** `crates/reify-eval/src/lib.rs` tests module
  — `test_drain_and_record_warm_pool_events`: drains pool, asserts journal event count, returns `Vec<WarmPoolEvent>`.

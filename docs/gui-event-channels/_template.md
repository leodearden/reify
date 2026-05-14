# Per-Channel Event Spec: `<channel-name>`

> **Purpose:** Per-channel spec template for entries in the GUI event channel inventory
> ([`../gui-event-channels.md`](../gui-event-channels.md)).
>
> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task γ.
>
> **Audience:** GR-016 Phase 2 task δ (3539), Phase 3 tasks ε (3541), ζ (3543), η (3545), θ (3547),
> and GR-024 task ι (3458) for the `mode-shape-frame` channel. Each task instantiates this template
> by copying it to `docs/gui-event-channels/<channel-name>.md` and filling in the placeholders.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `<channel-name>`
- **Rust emit site:** `<crate>/<path/to/file.rs>:<line>` — `<fn_name>()`
- **TS listen site:** `gui/src/bridge.ts` — `on<Name>(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct <PayloadType> {
    // Fill in fields. Use Option<T> for optional fields; serde skip_serializing_if = "Option::is_none".
    // pub field_name: FieldType,
}
```

```typescript
export interface <PayloadType> {
  // Fill in fields matching the Rust struct above.
  // fieldName: FieldType;
  // optionalField?: FieldType;
}
```

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `<crate>/<path/to/file.rs>:<line>` — `<context_fn_or_callback_name>`
- **Trigger:** `<describe the condition that causes the emit — e.g. "at entry to auto-resolve
  orchestrator loop", "at each CG solver iteration boundary", "when case-switch fires in
  multi-load dispatch", "at eval boundary via WarmStatePool::drain_events() translator">`
- **Frequency:** `<once per …, per iteration, per file-change, etc.>`

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `on<Name>(callback): Promise<UnlistenFn>`
- **Subscribing component/store:** `<gui/src/panels/<Panel>.tsx OR gui/src/stores/<store>.ts>`
- **Unlisten lifecycle owner:** `<who calls unlisten — e.g. "panel unmount useEffect cleanup",
  "engineStore.unsubscribeAll()", "rollback in subscribeToClaudeEvents-style batch">`
- **Subscription pattern:** `<panel-local OR global via engineStore — per PRD §7 convention>`

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field). Record deviations only.

> _If this channel deviates from the default (e.g. because it is consumed by external tooling or
> requires a migration that spans releases), describe the `version: u32` field placement and
> dispatch logic here. Otherwise delete this note and leave the line above._

---

## 6. Error semantics

Default per PRD §5:

- **Malformed payload:** hard-fail (throw) in debug builds; `console.warn` + drop in release builds (§5.2).
- **Emit failure:** `tracing::warn!` and continue; no surrounding operation failure (§5.3).
- **Missing emitter:** silent; defended by CI roundtrip gate (§5.1 + §6.1).

Record channel-specific deviations only.

> _If this channel deviates (e.g. malformed payload is fatal in release due to safety invariants),
> describe the divergence here and reference the decision record. Otherwise delete this note._

---

## 7. Test pointers

- **Rust roundtrip test:** `crates/reify-gui-tests/tests/<channel-name>_roundtrip.rs`
  — builds the payload struct, serializes via `serde_json::to_value`, asserts shape against a
  frozen-fixture snapshot (PRD §6.1).
- **TS shape test:** `gui/src/__tests__/bridge/<channel-name>.test.ts`
  — constructs a representative payload, delivers it via `mockTauriEvent`, asserts the `on<Name>`
  callback receives correctly-typed data; for hand-shaped payloads also asserts `validatePayload`
  throwing (debug) and dropping (release) on malformed input (PRD §6.2 / §6.3).

---

## How to use this template

1. **Copy** this file to `docs/gui-event-channels/<channel-name>.md`.
2. **Replace** every `<placeholder>` token with channel-specific content.
3. **Update** the matching row in [`docs/gui-event-channels.md`](../gui-event-channels.md) in the
   **same commit** — the inventory and the per-channel spec must stay in lockstep (PRD §3.3).

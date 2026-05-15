# Per-Channel Event Spec: `auto-resolve-iteration`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task δ (GR-016 Phase 2 proof slice).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §2.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `auto-resolve-iteration`
- **Rust emit site:** `gui/src-tauri/src/main.rs` — `TauriAutoResolveEmitter::iteration()` (calls `event_bus::emit_typed(&self.app, "auto-resolve-iteration", &iter)`)
- **TS listen site:** `gui/src/bridge.ts` — `onAutoResolveIteration(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveParameterValue {
    pub value: f64,
    pub unit: String,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveConstraintProgress {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_lower: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_upper: Option<f64>,
    pub satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoResolveIteration {
    pub iteration: u32,
    pub parameters: HashMap<String, AutoResolveParameterValue>,
    pub constraints: HashMap<String, AutoResolveConstraintProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driving_metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driving_metric_value: Option<f64>,
}
```

Source: `gui/src-tauri/src/types.rs:583-634`.

```typescript
export interface AutoResolveParameterValue {
  value: number;
  unit: string;
  display: string;
}

export interface AutoResolveConstraintProgress {
  name: string;
  value?: number;
  unit?: string;
  target_lower?: number;
  target_upper?: number;
  satisfied: boolean;
}

export interface AutoResolveIteration {
  iteration: number;
  parameters: Record<string, AutoResolveParameterValue>;
  constraints: Record<string, AutoResolveConstraintProgress>;
  driving_metric?: string;
  driving_metric_value?: number;
}
```

Source: `gui/src/types.ts:388-429`.

> **Wire-optionality — `AutoResolveConstraintProgress.value` (resolved):**
> On the Rust side, `value` is `Option<f64>` with `#[serde(skip_serializing_if = "Option::is_none")]` (see the `value` field of `AutoResolveConstraintProgress` in `gui/src-tauri/src/types.rs`) — it is **omitted from the wire payload** whenever the kernel has no observed scalar for the constraint (the common case). The TS interface previously declared `value: number` (required); a consumer reading `constraints[k].value` would compile-pass but receive `undefined` at runtime when the field is absent. Resolved in task 3539: the `value` field of the `AutoResolveConstraintProgress` interface in `gui/src/types.ts` is now `value?: number`, matching actual wire behaviour (a non-breaking type widening; `AutoResolvePanel` already renders absent values as empty).

### §11 Q3 resolution (2026-05-15, task 3539)

PRD §11 Q3 asked whether `parameters` and `constraints` should be `BTreeMap<String, f64>` (simple) or richer types. **Resolution:** HashMap with rich value types — `HashMap<String, AutoResolveParameterValue>` and `HashMap<String, AutoResolveConstraintProgress>` — closely matching the TS interface shapes (see wire-optionality note above for the one known divergence).

**Rationale:** The TS side already defined typed `AutoResolveParameterValue` (with `value`, `unit`, `display`) and `AutoResolveConstraintProgress` (with `name`, `value?`, `unit?`, `target_lower?`, `target_upper?`, `satisfied`). Dropping to `Map<String, f64>` would have either (a) forced a TS breaking change, or (b) created payload divergence between what the backend emits and what the frontend's types describe. Existing tests assert per-field rather than via golden-file snapshots, so HashMap iteration order is not a problem; BTreeMap would only buy deterministic JSON for snapshot tests that don't exist.

---

## 3. Producer site(s) and emission triggers

- **Emit site:** `gui/src-tauri/src/engine.rs:549-572` — `EngineSession::emit_auto_resolve_if_any`
- **Trigger:** fires once per `EngineSession::check()` call when `check.resolved_params` is non-empty; sandwiched between the matching `auto-resolve-start` and `auto-resolve-complete` events in the same synchronous call.
- **Frequency:** once per `EngineSession::{load_from_source, set_parameter, update_source}` call that produces a non-empty `resolved_params` result. `iteration` is always `0` in the current single-iteration-per-pass solver model.

> **Note on emit-trio synthesis location:** Same rationale as `auto-resolve-start` — trio synthesised at the GUI layer to preserve the kernel-layer dependency direction. See `auto-resolve-start.md §3` note.

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onAutoResolveIteration(callback): Promise<UnlistenFn>`
  - Includes `validatePayload`-style structural guard: drops malformed payloads (missing `iteration`, `parameters`, or `constraints`) with `console.warn` rather than propagating a downstream NPE.
- **Subscribing component/store:** `gui/src/stores/engineStore.ts` — `subscribeToEvents()` → `engineStore.applyAutoResolveIteration`; `AutoResolvePanel` (`gui/src/panels/AutoResolvePanel.tsx`) renders the chart from derived store state.
- **Unlisten lifecycle owner:** `engineStore.subscribeToEvents` batch rollback path.
- **Subscription pattern:** global via `engineStore`.

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field). `driving_metric` / `driving_metric_value` are optional fields — omission is the backwards-compatible "no primary metric" signal.

---

## 6. Error semantics

Default per PRD §5, with one channel-specific note:

- **Malformed payload:** `console.warn` + drop in the guard inside `onAutoResolveIteration` (`gui/src/bridge.ts`) — the structural guard validates **top-level shape only**: `typeof p['iteration'] === 'number'`, `isPlainObject(p['parameters'])`, `isPlainObject(p['constraints'])`. It does **not** inspect inner `AutoResolveParameterValue` / `AutoResolveConstraintProgress` entry fields; those are trusted per the serde contract (consistent with the §2 wire-optionality note). Drop-at-top-level is preferred over throw because a gross structural mismatch (e.g. `parameters` absent entirely) should not crash `AutoResolvePanel`; the panel itself renders absent inner fields gracefully (e.g. an omitted `constraints[k].value`).
- **Emit failure:** `tracing::warn!` and continue (§5.3).
- **Missing emitter:** silent (§5.1 + §6.1).

---

## 7. Test pointers

> Test pointers use symbol/function-name anchors rather than absolute line numbers (line ranges drift; symbol names are stable). Grep the cited file for the function name.

- **Rust serde roundtrip tests:** `gui/src-tauri/src/tests/types_tests.rs`
  — `auto_resolve_iteration_serializes_with_expected_field_set`, `auto_resolve_iteration_omits_optional_when_none`, `auto_resolve_constraint_progress_omits_unset_targets_and_unit` cover the per-field shape contract.
- **Rust emit-sequence test:** `gui/src-tauri/src/tests/engine_tests.rs`
  — `engine_session_auto_resolve_emitter_fires_start_iter_complete_when_solver_resolves` asserts `events[1]` matches `EmitEvent::Iteration(_)` with correct parameter payload; `engine_session_auto_resolve_emitter_fires_on_set_parameter_when_solver_present` additionally covers the `set_parameter` path.
- **TS malformed-payload tests:** `gui/src/__tests__/bridge.test.ts`
  — the `onAutoResolveIteration malformed payload` describe block (8 cases from task-3407) covers the validatePayload-style drop semantics including missing `parameters`, missing `constraints`, wrong `iteration` type, etc.

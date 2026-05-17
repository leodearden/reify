# Per-Channel Event Spec: `fea-case-changed`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task η (GR-016 Phase 3).
>
> **Inventory row:** [`docs/gui-event-channels.md`](../gui-event-channels.md) §2.

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `fea-case-changed`
- **Rust emit site (upstream hook):** `gui/src-tauri/src/engine.rs` — `EngineSession::emit_fea_case_if_any(&self, check: &CheckResult)` (scans `check.values` for the first MultiCaseResult-shaped cell; calls `TauriFeaCaseEmitter::changed` when found)
- **Rust emit site (transport):** `gui/src-tauri/src/main.rs` — `TauriFeaCaseEmitter::changed()` (calls `event_bus::emit_typed(&self.app, "fea-case-changed", &payload)`)
- **Rust detector:** `crates/reify-eval/src/multi_load_dispatch.rs` — `detect_multi_case_result(&reify_types::Value) -> Option<DetectedCases>`
- **TS listen site:** `gui/src/bridge.ts` — `onFeaCaseChanged(callback): Promise<UnlistenFn>`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
/// IPC payload for the `fea-case-changed` Tauri channel (GR-016 η).
///
/// Emitted once per check that observes a MultiCaseResult-shaped value in
/// `CheckResult.values`. Field names match the TypeScript `FeaCaseChanged`
/// interface in `gui/src/types.ts` exactly — no `serde(rename_all)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaCaseChanged {
    /// Lexicographically smallest case name (deterministic BTreeMap key order
    /// from the stdlib `extract_cases_map` shape in `reify-stdlib/src/fea.rs`).
    /// The user overrides this via the `FeaCasePickerDropdown` setter.
    pub active_case_id: String,
    /// Sorted list of available load-case names (BTreeMap key order preserved).
    pub available_cases: Vec<String>,
}
```

Source: `gui/src-tauri/src/types.rs` — `FeaCaseChanged`.

```typescript
/**
 * Payload for the `fea-case-changed` Tauri channel (GR-016 η).
 *
 * Wire format per PRD §2.2: field names match the Rust IPC struct in
 * `gui/src-tauri/src/types.rs::FeaCaseChanged` exactly — no `serde(rename_all)`.
 */
export interface FeaCaseChanged {
  /** Lexicographically smallest case name on first emission; user-selected thereafter. */
  active_case_id: string;
  /** Sorted list of available load-case names. */
  available_cases: string[];
}
```

Source: `gui/src/types.ts` — `FeaCaseChanged`.

---

## 3. Producer site(s) and emission triggers

- **Upstream hook:** `gui/src-tauri/src/engine.rs` — `EngineSession::emit_fea_case_if_any(&self, check: &CheckResult)`, co-located at every `emit_auto_resolve_if_any` call site: the 4 production sites (`load_from_source`, `set_parameter`, `load_file`, `update_source`) and the test helper `check_and_emit_for_test`.
- **Transport:** `gui/src-tauri/src/main.rs` — `TauriFeaCaseEmitter::changed()` installed in `setup()` via `EngineSession::set_fea_case_emitter(Arc::new(TauriFeaCaseEmitter { app }))`.
- **Detector:** `crates/reify-eval/src/multi_load_dispatch.rs` — `detect_multi_case_result` keys on the same outer `Map{"cases" -> Map<Value::String, ElasticResult-Map>}` shape contract as the stdlib's private `extract_cases_map` (`crates/reify-stdlib/src/fea.rs`). Returns `Some(DetectedCases { active_case_id, available_cases })` when the shape matches and `available_cases` is non-empty; `None` otherwise.
- **Trigger:** fires once per check that observes a MultiCaseResult-shaped `Value::Map` in `CheckResult.values`. Mirrors auto-resolve fire-every-commit semantics — **NO engine-side dedup**. Re-delivering an unchanged case set is harmless; the frontend `feaModeStore.setActiveCaseId` setter is idempotent.
- **Frequency:** at most once per check (per commit that detects a multi-case value). Zero events emitted when no MultiCaseResult-shaped cell is present (the common state until task 3026 lands `solve_load_cases`).

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onFeaCaseChanged(callback): Promise<UnlistenFn>`
  - Uses the inline structural-shape-guard idiom (`listen<unknown>` + `isPlainObject` + per-field type checks + `console.warn` drop), consistent with `onAutoResolveIteration` (bridge.ts) and `onWarmPoolEvent` (bridge.ts). Guard checks `typeof p['active_case_id'] === 'string'` and `Array.isArray(p['available_cases']) && p['available_cases'].every((s) => typeof s === 'string')`. Drops malformed payloads with `console.warn('[fea-case-changed] malformed payload; dropping event', p)`.
- **Subscribing component:** `gui/src/panels/FeaCasePickerDropdown.tsx` — renders a `<select data-testid="fea-case-picker-dropdown">` bound to `feaModeStore.activeCaseId`; renders nothing (via `<Show>`) when `availableCases` is empty. The component initialises `activeCaseId` to `availableCases[0]` via `createEffect` on mount.
- **Subscription wiring (deferred to task 3026):** `FeaCasePickerDropdown` does not yet subscribe to `onFeaCaseChanged` directly — it only renders from props. The `onCleanup` / `unlistenPromise.then(fn => fn())` lifecycle plumbing will be added by task 3026 (`solve_load_cases`) when the emitter begins producing real `MultiCaseResult` values. Until then, `onFeaCaseChanged` is defined in `bridge.ts` but has no caller in the shipped UI.
- **Subscription pattern (planned):** panel-local (not routed through `engineStore`). Task 3026 will wire `onFeaCaseChanged` inside `FeaCasePickerDropdown` and pass `availableCases` down from the event payload.

---

## 5. Versioning policy

Default per PRD §3.3 (no `version` field).

---

## 6. Error semantics

Default per PRD §5:

- **Malformed payload:** `console.warn` + drop in `onFeaCaseChanged` (inline shape guard — see §4).
- **Emit failure:** `tracing::warn!` and continue; no surrounding operation failure (`TauriFeaCaseEmitter::changed` logs `"fea-case-changed emit failed: {}"` on error — `gui/src-tauri/src/main.rs`).
- **Missing emitter:** silent; `EngineSession::emit_fea_case_if_any` early-returns when `fea_case_emitter` is `None`.

---

## 7. Test pointers

> Test pointers use symbol/function-name anchors rather than absolute line numbers (line ranges drift; symbol names are stable). Grep the cited file for the function name.

- **Rust serde roundtrip test:** `gui/src-tauri/src/tests/types_tests.rs`
  — `fea_case_changed_serializes_to_expected_json_shape`: constructs `FeaCaseChanged { active_case_id: "operating", available_cases: ["operating", "overload", "transport"] }`, serializes via `serde_json::to_value`, asserts exact JSON shape `{"active_case_id":"operating","available_cases":["operating","overload","transport"]}`. Pins PRD §3.2 field-name-exactness (§6.1 gate).
- **Rust engine emitter tests:** `gui/src-tauri/src/tests/engine_tests.rs`
  — `RecordingFeaCaseEmitter` (Arc<Mutex<Vec<FeaCaseChanged>>> recorder pattern):
  - `fea_case_emitter_fires_when_multi_case_value_present`: asserts exactly one recorded event with correct `active_case_id`/`available_cases` when CheckResult contains a multi-case cell.
  - `fea_case_emitter_no_fire_when_no_multi_case`: asserts zero events for an ordinary source.
  - `fea_case_emitter_re_fires_on_each_check`: two checks with the same case set record TWO events — pins the fire-every-commit / no-engine-side-dedup contract.
- **Rust detector unit tests:** `crates/reify-eval/src/multi_load_dispatch.rs` (inline `#[cfg(test)] mod tests`)
  — covers None for non-Map values, None for empty cases map, None for missing/wrong-type "cases" key, Some with correct lex-smallest `active_case_id` and sorted `available_cases` for a 3-case fixture. Uses `reify_test_support::values::multi_case_result_value` fixture builder.
- **TS bridge shape test:** `gui/src/__tests__/bridge/feaCaseChanged.test.ts`
  — happy-path: valid payload forwarded to callback; malformed payloads (missing `available_cases`, `available_cases` not an array, `active_case_id` not a string) dropped with `console.warn` mentioning `fea-case-changed` (§6.2 + §6.3 gate).
- **Panel rendering test:** `gui/src/__tests__/FeaCasePickerDropdown.test.tsx`
  — renders nothing when `availableCases=[]`; renders `<select data-testid="fea-case-picker-dropdown">` with one `<option>` per name when non-empty; `fireEvent.change` updates `store.state.activeCaseId`; `activeCaseId=null` falls back to `availableCases[0]` as defensive default.

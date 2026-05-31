# Per-Channel Event Spec: `solver-progress`

> **Source:** [`docs/prds/v0_3/gui-event-channel-inventory.md`](../prds/v0_3/gui-event-channel-inventory.md) §9 task ζ.
>
> **Owning task:** GR-016 ζ (task 3543).

---

## 1. Channel name + Rust + TS file/symbol locations

- **Channel:** `solver-progress`
- **Rust payload type:** `gui/src-tauri/src/types.rs` — `SolverProgress`
- **Rust kernel callback seam:** `crates/reify-solver-elastic/src/solver.rs` — `cg_loop` (iteration-end
  callback via `solve_cg_with_progress`). The actual `app.emit` call site is wired by the
  engine-boundary follow-on task; both pointers are cited here for traceability.
- **Rust emit wrapper:** `gui/src-tauri/src/event_bus.rs` — `emit_typed` (used by engine-boundary
  emit-call wiring when it lands; follow-on task).
- **TS listen site:** `gui/src/bridge.ts` — `onSolverProgress(callback): Promise<UnlistenFn>`
- **TS cancel command:** `gui/src/bridge.ts` — `cancelSolve(): Promise<void>`
- **Tauri cancel command impl:** `gui/src-tauri/src/commands.rs` — `cancel_solve_impl`

---

## 2. Payload Rust struct + TS interface

Field names match exactly (§3.2 — no `#[serde(rename_all)]`).

```rust
/// IPC payload for the `solver-progress` Tauri event channel (GR-016 ζ).
///
/// Field names match the TS interface exactly (no `serde(rename_all)` per PRD §3.2).
/// `eta_ms` is omitted from the wire payload when unknown (first iteration, or when
/// convergence history is insufficient for estimation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolverProgress {
    /// Solver kind identifier (e.g. `"cg"` for Conjugate Gradient).
    pub solver_kind: String,
    /// 1-based iteration number just completed.
    pub iter: u32,
    /// L2 residual norm at end of this iteration.
    pub residual: f64,
    /// Estimated time to convergence in milliseconds; omitted when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
}
```

```typescript
export interface SolverProgress {
  /** Solver kind identifier (e.g. "cg"). */
  solver_kind: string;
  /** 1-based iteration number just completed. */
  iter: number;
  /** L2 residual norm at end of this iteration. */
  residual: number;
  /** Estimated time to convergence in milliseconds; absent when unknown. */
  eta_ms?: number;
}
```

---

## 3. Producer site(s) and emission triggers

- **Kernel callback seam:** `crates/reify-solver-elastic/src/solver.rs` — end of each CG iteration
  inside `cg_loop`, after the residual-norm update and convergence check. The callback is injected
  via `solve_cg_with_progress(&k, &f, initial_guess, opts, mode, &mut |iter, residual| { … })`.
- **Engine-boundary emit site:** follow-on task (engine-boundary dispatch wiring). When that task
  lands, it will call `event_bus::emit_typed(&app, "solver-progress", &SolverProgress { … })` from
  within the `solve_elastic_static` dispatch path.
- **Trigger:** at the end of every CG iteration during a `solve_elastic_static` dispatch — one
  event per iteration, starting at `iter = 1`.
- **Frequency:** ~10–100 events per typical solve (depends on mesh DOF count, convergence
  tolerance, and preconditioning).
- **Cooperative-cancellation seam:** the iteration callback returns `CgIterationControl::Cancel`
  when `AppState::pending_solve_cancel.is_cancelled()`. The `cancel_solve` Tauri command (§1 above)
  publishes a flag via `pending_solve_cancel`; the engine-boundary wiring translates that flag into
  a `Cancel` return on the next iteration callback invocation (PRD §11 Q2 /
  compute-node-contract §2 SLA).

---

## 4. Consumer site(s) and unlisten lifecycle owner

- **bridge.ts wrapper:** `gui/src/bridge.ts` — `onSolverProgress(callback): Promise<UnlistenFn>`
- **Subscribing component:** `gui/src/panels/SolverProgressOverlay.tsx` (props-driven; does not
  subscribe internally — subscription lifecycle is owned by the consuming store or parent component).
- **Future engineStore subscription:** a follow-on task will wire `onSolverProgress` into
  `engineStore` (or a dedicated `feaProgressStore`) to drive the overlay's `progress` prop.
  When that lands, `unlisten` cleanup belongs to the store's teardown path.
- **Subscription pattern:** panel-local (pure-render component; props-driven per the
  `FeaCasePickerDropdown` precedent — PRD §7 convention).

---

## 5. Versioning policy

Default per PRD §3.3 — no `version` field. No deviation.

---

## 6. Error semantics

Default per PRD §5 with one channel-specific deviation:

- **Malformed payload:** `console.warn` + drop in **both** debug and release builds (deviation from
  the default hard-fail in debug). Rationale: a malformed progress event should not abort a healthy
  in-flight solve. The solver continues; only the overlay is dark for that iteration.
- **Emit failure:** `tracing::warn!` and continue (default §5.3).
- **Missing emitter:** silent (default §5.1 + §6.1).

---

## 7. Test pointers

- **Rust solver callback tests (step-1, step-3):**
  `crates/reify-solver-elastic/src/solver.rs` (tests mod) —
  `solve_cg_with_progress_fires_callback_per_iteration_and_converges` and
  `solve_cg_with_progress_cancel_terminates_iteration_within_one_step`.
- **Rust IPC roundtrip test (step-5):**
  `gui/src-tauri/src/tests/types_tests.rs` —
  `solver_progress_serializes_to_expected_json_shape` (full payload + eta_ms-None + round-trip).
- **TS bridge shape test (step-7):**
  `gui/src/__tests__/bridge/solver_progress.test.ts` —
  happy-path (eta_ms present + absent) and 3 malformed-drop cases.
- **Rust cancel_solve_impl tests (step-9):**
  `gui/src-tauri/src/tests/commands_tests.rs` —
  `cancel_solve_impl_fires_published_handle_and_clears_slot` and
  `cancel_solve_impl_returns_ok_when_slot_empty`.
- **SolverProgressOverlay panel tests (step-11):**
  `gui/src/__tests__/SolverProgressOverlay.test.tsx` —
  null-progress → empty render; non-null → content assertions; Cancel click → onCancel invoked.

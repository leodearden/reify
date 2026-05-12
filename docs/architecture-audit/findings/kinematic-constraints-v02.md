# Audit: Kinematic Constraints — Closed Chains + Filled Joint Zoo (v0.2)

**PRD path:** `docs/prds/v0_2/kinematic-constraints.md`
**Auditor:** audit-kinematic-constraints-v02
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 4

## Top concerns

- **Singularity / over-constraint / under-constraint diagnostics never reach `EvalResult`.** The diagnostic-emitting wrapper `solve_loop_closure_with_diagnostics` exists and is fully unit-tested, but `snapshot()` and `sweep()` deliberately call the bare `solve_loop_closure` (see `snapshot.rs:158-167`). The typed `KinematicSingularity`/`Overconstrained`/`Underconstrained` enum variants therefore round-trip through nothing in user-visible eval output. Snapshot Map has no `is_singular` flag either — PRD §"Resolved design decisions" explicitly calls for `is_singular: true` on the snapshot.
- **Multi-DOF joints (planar, spherical, cylindrical) cannot appear inside a closed chain.** `value_for_joint` and `joint_range_midpoint` return `None` for all three kinds by deliberate design (deferred to task #2670, which is itself marked done — i.e. the deferral is final-for-v0.2). Any closed-chain mechanism whose loop traverses a multi-DOF joint will see `chain_transform → None → extract_loop_closure_chains → None → snapshot Undef`. The PRD's filled-out joint zoo and the loop-closure machinery are wired in parallel but **only intersect on prismatic + revolute + coupling chains**.
- **Per-loop analytic-Jacobian path is partly aspirational.** `joint_jacobian` returns zero-magnitude placeholder columns for `planar` and `spherical` (`joints.rs:785, 800`), which short-circuits `per_joint_jacobian_local` to `None`, which forces the chain Jacobian through finite-difference fallback unconditionally. The PRD prose hints at analytic forms for these as a follow-up; in code they are zero-twist placeholders today.
- **`E_KINEMATIC_CLOSED_CHAIN` is dead reserved.** The variant exists in `DiagnosticCode` for v0.1-strict-mode futures, but no path in the v0.2 builder emits it; `mechanism::body` now records loop closures silently. This is per-PRD intent but worth noting because the diagnostic code lives next to the actively-used ones.

## Mechanisms

### M-001: `mechanism().body(...)` builder accepts closed chains (records `loop_closure` records instead of erroring)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/mechanism.rs:382-398` (`make_loop_closure_record`), `mechanism.rs:416+` (`append_body` closing-edge path); task 2671 done (commit `5d532cb745`); tests `crates/reify-eval/tests/mechanism_builder_smoke.rs`.
- **Blocks:** none
- **Note:** Mechanism Map now carries a `loop_closures: List<Map>` field; each entry has `body_id`, `closing_joint`, `path_a`, `path_b`. Shortest-path-from-world spanning tree convention.

### M-002: `chain_transform(chain, values)` — left-to-right SE(3) composition along a joint chain

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/loop_closure.rs:59-80`.
- **Blocks:** none for prismatic/revolute/coupling/fixed; transitively blocks multi-DOF closed chains via M-007.
- **Note:** Short-circuits to None whenever `value_for_joint` returns None — the gate for the multi-DOF gap.

### M-003: `loop_residual_twist` — `[ω_x,ω_y,ω_z,v_x,v_y,v_z]` residual between two chains

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `loop_closure.rs:89-110`; tests `crates/reify-eval/tests/kinematic_loop_closure_machinery.rs`.
- **Blocks:** none
- **Note:** Single canonical twist ordering reused across solver and Jacobian.

### M-004: `NewtonConfig` / `NewtonOutcome` / `newton_solve` Gauss-Newton solver

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/loop_closure_solver.rs:63-87, 114-158, 339-490`; PRD defaults 1µm / 1µrad / 50 iters present; LDLᵀ singular-pivot detection; monotonic-divergence guard (`DIVERGENCE_LIMIT=3`).
- **Blocks:** none
- **Note:** Bare Gauss-Newton; no Levenberg-Marquardt damping, no Armijo line search. PRD §"Robustness scope" explicitly accepts this MVP.

### M-005: Warm-start strategy (`StartStrategy::WarmStart` / `Midpoint`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `loop_closure_solver.rs:95-101`; `solve_loop_closure_with_diagnostics:864-881` honours both branches; `crates/reify-eval/tests/kinematic_sweep_closed_chain.rs` tests cross-step warm-start.
- **Blocks:** none
- **Note:** `joint_range_midpoint` returns None for multi-DOF kinds (planar/spherical/cylindrical) by design — `Midpoint` strategy can't seed a free multi-DOF joint.

### M-006: `solve_loop_closure(chain_a, vals_a, chain_b, vals_b_initial, free_b, strategy, config)` convenience wrapper

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `loop_closure_solver.rs:492+` (signature); called from `snapshot.rs:296-304`.
- **Blocks:** none
- **Note:** This is the bare path; the diagnostic-emitting sibling is M-009.

### M-007: Multi-DOF joints (planar / spherical / cylindrical) inside loop-closure chains

- **State:** PARTIAL
- **Failure mode:** F2 (designed gap; PRD prose hedges via "FD fallback ... until analytic forms are derived" but the f64-per-joint chain signature itself is the blocker, not analytic vs FD)
- **Evidence:** `loop_closure.rs:322-356` `value_for_joint` returns `None` for `planar`/`spherical`/`cylindrical`; `loop_closure.rs:142-200` `joint_range_midpoint` same; `joints.rs:785, 800` analytic-Jacobian placeholders. Tracking task 2670 marked done with this gap accepted as "FD fallback for multi-DOF kinds — deferred to PRD v0.2 kinematic task 2".
- **Blocks:** any closed-chain mechanism whose spanning-tree-residual loop traverses a multi-DOF joint will snapshot to Undef.
- **Note:** All seven new joint types are wired as standalone open-chain primitives (transform_at, joint_jacobian for analytic single-DOF, zero placeholder for multi-DOF). The closed-chain solver explicitly excludes them. End-to-end coverage gap.

### M-008: `extract_loop_closure_chains` — per-loop solver-input bundle from a `loop_closures` record

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `loop_closure.rs:409+`; consumed by `snapshot.rs:265-275`.
- **Blocks:** none for single-DOF chains
- **Note:** Strips world sentinel; resolves per-joint SI values from bindings or midpoint; partitions free vs bound positions in `chain_b`.

### M-009: `solve_loop_closure_with_diagnostics` — typed-diagnostic-emitting wrapper

- **State:** PARTIAL
- **Failure mode:** F4 (mechanism exists and is unit-tested but the production path bypasses it)
- **Evidence:** `loop_closure_solver.rs:820-938` — full implementation; tests in `crates/reify-constraints/tests/loop_closure_diagnostics_tests.rs` and `crates/reify-eval/tests/kinematic_diagnostics_e2e.rs`. `snapshot.rs:158-167` documents the explicit choice NOT to call this wrapper, citing the over-constrained pre-check (`free_b.len() < 6`) being too strict for the simple 1-DOF fixtures.
- **Blocks:** PRD task 10 §"Resolved design decisions" — singularity surfacing in snapshot output.
- **Note:** The wrapper is mechanistically complete (Overconstrained pre-check, Underconstrained pre-check, Singular post-process all emit typed `Diagnostic`s). It is decoupled from eval. The cited blocker (over-constrained pre-check on rotational-zero residuals) is a real design issue: low-DOF prismatic-only loops are physically well-posed but get flagged as over-constrained by the 6-residual count.

### M-010: `KinematicSingularity` / `KinematicOverconstrained` / `KinematicUnderconstrained` typed `DiagnosticCode` variants

- **State:** PARTIAL
- **Failure mode:** F4 (typed variants reserved + emitted by wrapper but never reach `EvalResult.diagnostics`)
- **Evidence:** `crates/reify-types/src/diagnostics.rs:769, 790, 812`; each variant's rustdoc carries an explicit "TODO: surfaced through the snapshot / sweep API in PRD task 10". `grep -r "KinematicSingularity"` in `crates/reify-eval/src` returns only a passing comment in `dispatcher.rs:167` (advisory-warning comparison).
- **Blocks:** any consumer (LSP/MCP/IDE) that wants to surface kinematic numerical issues to the user.
- **Note:** The variants are reserved for future emission; the wrapper would feed them. Connecting the wires is the unfinished step.

### M-011: `is_singular: true` flag on Snapshot Map (per PRD §"Singularity, over/under-constraint diagnostics")

- **State:** FICTION
- **Failure mode:** F1 (PRD prose calls for it explicitly; no code carries it)
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:772-784` `make_snapshot` emits exactly `{bodies, free_values, kind}` — no `is_singular` field. PRD text: "the snapshot accessor reports `is_singular: true`" and "`is_singular: true` flag" (resolved decisions).
- **Blocks:** any user-facing surface (CAD UI) that wants to distinguish singular from converged snapshots.
- **Note:** `LoopClosureReport::is_singular()` exists as a Rust method on the report struct, but the Reify-language Snapshot Map carries no equivalent field.

### M-012: Prismatic / revolute joints (v0.1 baseline)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/joints.rs:14-43`; analytic transform + analytic Jacobian; full validation (axis non-zero finite, range dimensioned + bounded).
- **Blocks:** none
- **Note:** Baseline that v0.2 builds on; PRD explicitly retains these unchanged.

### M-013: Cylindrical joint (2-DOF: prismatic ⊕ revolute on shared axis)

- **State:** WIRED (standalone)
- **Failure mode:** N/A (for open chains; participates in M-007 gap for closed chains)
- **Evidence:** `joints.rs:128-145` constructor, `joints.rs:368-393` `transform_at`, `joints.rs:815-823` `joint_jacobian` returns List<Map> of per-DOF analytic columns. Task 2673 done (commit `835693f51`).
- **Blocks:** see M-007
- **Note:** Motion variable shape is `Value::List<[Length, Angle]>` (the cylindrical-specific 2-tuple). The single-DOF chain-signature `value_for_joint` therefore returns None for cylindrical.

### M-014: Planar joint (3-DOF: 2 prismatic + 1 revolute, axis_x ⊥ axis_y)

- **State:** WIRED (standalone)
- **Failure mode:** N/A (for open chains; participates in M-007 gap for closed chains)
- **Evidence:** `joints.rs:53-96` constructor (with perpendicularity check `|dot|<1e-9`), `joints.rs:239-318` `transform_at` (composition T_x · T_y · T_θ), `joints.rs:785` zero-twist placeholder Jacobian. Task 2674 done (commit `e9120417b6`).
- **Blocks:** see M-007; analytic Jacobian deferred indefinitely per inline comments
- **Note:** Motion variable shape is `Value::List<[Length, Length, Angle]>`. Plane normal computed via cross product of unit axes.

### M-015: Spherical joint (3-DOF, quaternion-internal)

- **State:** WIRED (standalone)
- **Failure mode:** N/A (for open chains; participates in M-007 gap for closed chains)
- **Evidence:** `joints.rs:107-117` constructor (range_angle: ANGLE), `joints.rs:333-353` `transform_at` with quaternion finite check + renormalisation, `joints.rs:800` zero-twist placeholder Jacobian. Task 2672 done (commit `e664d2d5f3`).
- **Blocks:** see M-007
- **Note:** Motion variable is `Value::Orientation` (a unit quaternion). User-facing Euler/axis-angle facade exists via `orient_axis_angle` / `orient_euler` / `orient_to_euler` / `orient_to_axis_angle` constructors and accessors (orientation.rs). PRD §"Resolved design decisions" matches.

### M-016: Fixed joint (0-DOF, identity transform for grouping)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `joints.rs:152-162` constructor (no args), `joints.rs:191-231` `transform_at` 1-arg ergonomic + 2-arg chain forms, `joints.rs:766` zero Jacobian column placeholder. Task 2675 done (commit `4668847666`).
- **Blocks:** none
- **Note:** Single-field Map `{kind: "fixed"}` mirrors world sentinel shape. The PRD's "clearance-pair filtering" downstream consumer is out-of-scope for the joint itself.

### M-017: Coupling specialisations: `screw` / `gear` / `rack_and_pinion`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `joints.rs:541-634` (screw / gear / rack_and_pinion); all three delegate to `couple` via dimensionless `Value::Real(ratio)`. Task 2676 done (commit `d6169828310`).
- **Blocks:** none
- **Note:** No nested couplings (couple rejects coupling parents at construction). `gear` requires strictly positive Int tooth counts. Coupling specialisations are dimension-agnostic at parent-validation time — `couple` accepts either prismatic or revolute, and the wrappers don't second-guess it.

### M-018: Sweep API closed-chain integration with warm-start across steps

- **State:** WIRED
- **Failure mode:** N/A (for prismatic/revolute/coupling closed chains)
- **Evidence:** `crates/reify-stdlib/src/sweep.rs:396-447` warm-start threading; `crates/reify-eval/tests/kinematic_sweep_closed_chain.rs` end-to-end test; test `sweep_threads_warm_start_through_closed_chain_steps` (`sweep.rs:1503`). Task 2678 done (commit `1e99c898c9`).
- **Blocks:** transitively blocked for multi-DOF closed chains via M-007
- **Note:** `previous_free_values → next snapshot's 3rd arg`. Empty outer-List is the open-chain no-op fast path.

## Cross-PRD breadcrumbs

- The Snapshot Map's `{bodies, free_values, kind}` shape and the lack of an `is_singular` field is consumed by GUI clearance/visualisation work (PRD v0.2 mechanism-clearance / printer-build dogfood). Adding `is_singular` is a shape-versioning concern.
- `transform_at` / `transform_compose` / `transform_log` / `transform_exp` were prerequisites of M-002/M-003 and were satisfied by task 2583 — these helpers are now used by FEA-side rigid-body machinery as well (cross-cutting; not unique to kinematics).
- `Coupling` specialisations gear/screw/RnP overlap conceptually with belt/cable physics (PRD explicit out-of-scope) — the printer's Vectran-tendon CoreXY mechanism currently uses `coupling` not `screw`.

## Notes for Phase 3

- The taskmaster decomposition (tasks 2583, 2670–2678) is mostly complete-on-paper; **the gap is concentrated in two places**: (a) emitting kinematic diagnostics into `EvalResult` (snapshot/sweep bypass the wrapper), and (b) multi-DOF joint participation in loop-closure chains (the single-f64-per-joint chain signature is the structural barrier). Both gaps are accepted-and-deferred in the source code with detailed rationale; they read as "PRD intent partially delivered, with a clean follow-up surface" rather than fiction. The `is_singular` snapshot flag (M-011) is a stronger fiction — PRD prose calls for it directly and nothing in code carries it.

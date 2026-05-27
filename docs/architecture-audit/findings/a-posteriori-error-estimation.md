# Audit: A-Posteriori Error Estimation and Auto-Refinement

**PRD path:** `docs/prds/v0_4/a-posteriori-error-estimation.md`
**Auditor:** audit-a-posteriori-error-estimation
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 15 (3 WIRED, 1 PARTIAL, 12 FICTION + 2 TODO transitive)

## Top concerns

- The PRD's foundation (`v0.3` FEA kernel + `solve_elastic_static` ComputeNode dispatch + ElasticResult engine wiring) is itself unbuilt: ComputeNode struct exists but no builder produces one (`crates/reify-eval/src/graph.rs:516-519` "ComputeNodes are not produced by any builder in P3.1"). Until that lands every v0.4 mechanism that says "extend ElasticResult …" or "outer loop solve→estimate→refine" inherits **FICTION** from below it.
- Only one v0.4 mechanism has actually landed: the Z-Z energy-norm indicator kernel math (`crates/reify-solver-elastic/src/error_estimator.rs`, ~679 LOC with closed-form patch tests). It is fully decoupled from the rest of the pipeline — never called from any engine wiring, never lifted into `ElasticResult`, never surfaced in the GUI. It is an island of WIRED code surrounded by FICTION.
- The PRD's `ElasticOptions` extensions (`target_accuracy`, `max_refinement_iterations`, `max_dofs`, `target_quantity_of_interest: Option<QoIDescriptor>`) and `ElasticResult` extensions (`error_indicator`, `global_relative_energy_error`, `convergence_status: ConvergenceStatus`) are **entirely absent** from the actual stdlib (`crates/reify-compiler/stdlib/solver_elastic.ri`). Adding them is non-trivial because the stdlib already documents (lines 17-35) that Reify cannot encode `Number = auto`, `Field<X,Y>` in `param` position, or scientific-notation literals — so the PRD's API sketches need an encoding-deviation pass before lowering.
- **Mesher gap is structural**, not just a missing extension. The Gmsh adapter (`crates/reify-kernel-gmsh`) wires only `Mesh.MeshSizeMin/Max` (single global scalar). The PRD's "Gmsh size-field-driven local refinement" + "per-vertex size hints" requires Gmsh background-field plumbing that the adapter has zero scaffolding for. PRD §"Mesher choice" already hedges with the MMG3D bookmark when the >30% remesh ceiling is hit — that bookmark may end up the actual scope.

## Mechanisms

### M-001: Z-Z energy-norm error indicator (kernel math)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/error_estimator.rs` (679 LOC); public surface `ZzIndicator { per_element: Vec<f64>, global_relative_energy_error: f64 }` + `compute_zz_indicator(elements, mesh, material) -> ZzIndicator`; re-exported `crates/reify-solver-elastic/src/lib.rs:370`; closed-form tests (two-tet fan, uniform-stress textbook patch test, L-corner localisation, zero-energy guard, P2-connectivity panic guard) cover lines 342-609 in the test module.
- **Blocks:** none upstream; M-005 (engine integration) blocks downstream consumption.
- **Note:** Uses volume-weighted average recovery instead of full Z-Z least-squares SPR; module doc cites PRD §"Error indicator" as explicitly permitting either scheme. Asserts P1-tet-only (4-node connectivity); P2 / hex / wedge are out of scope despite being in the v0.3 stdlib.

### M-002: Per-element indicator surfaced as `Field<Element, ScalarPressure>`

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no runtime backing)
- **Evidence:** PRD §"Sketch of approach" + §"Error norm" specify `per_element_indicator: Field<Element, ScalarPressure>`. Type resolver does not accept `Field<X,Y>` in `param` position (TODO(field-in-param), task #3117 — cited in `crates/reify-compiler/stdlib/solver_elastic.ri:230-244`). `Element` and `ScalarPressure` types: no grep hits anywhere in `crates/`. Kernel output is bare `Vec<f64>` (M-001) with no type-level Field<…> wrapping.
- **Blocks:** M-005 (ElasticResult API extension), M-014 (GUI scalar channel) — both assume this resolves cleanly.
- **Note:** Stdlib documents the same workaround already used for `displacement`/`stress`: declare as `Real` placeholder, populate as a Field-typed Map at runtime. The PRD does not anticipate that workaround for the new fields.

### M-003: Global relative energy-norm error as a `Number`

- **State:** PARTIAL
- **Failure mode:** F2 (kernel value present; engine-level surface absent)
- **Evidence:** `ZzIndicator.global_relative_energy_error: f64` exists (M-001). PRD calls for surfacing as `global_relative_energy_error: Number` on `ElasticResult`. `ElasticResult` in `crates/reify-compiler/stdlib/solver_elastic.ri:295-316` has no such field; Rust mirror `crates/reify-eval/src/persistent_cache.rs:450-457` (`pub struct ElasticResult`) has no such field either.
- **Blocks:** M-005, M-009 (CI gate reads this).
- **Note:** Scalar value computed but never escapes the kernel crate.

### M-004: `ConvergenceStatus` enum on `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §"Confidence signaling" specifies `convergence_status: enum { Converged(target), NotConverged { reason: BudgetReason } }`. No `ConvergenceStatus` / `BudgetReason` enum exists anywhere in the workspace (grep across `crates/` returns no hits other than the in-crate `TerminationReason` from `crates/reify-solver-elastic/src/progressive.rs:202-209` — a similar shape but a different name, a different field set, and not exposed through `ElasticResult`). The existing `converged: Bool` field on stdlib `ElasticResult` is a CG-convergence flag, not a refinement-budget flag.
- **Blocks:** M-005; auto-resolve composition (M-013); CI gate (M-009).
- **Note:** `TerminationReason::{BudgetExhausted, NoRefinementRequested}` from progressive.rs is the v0.3 yield-proximity-trigger analogue, NOT the v0.4 refinement-loop status. The PRD's enum shape (with `Converged(target)` carrying the target value and `NotConverged { reason }` carrying budget cause) has no precursor in code.

### M-005: `ElasticResult` API extension (error_indicator, global_relative_energy_error, convergence_status)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Stdlib `structure def ElasticResult` at `crates/reify-compiler/stdlib/solver_elastic.ri:295-316` declares only `displacement, stress, frame, max_von_mises, converged, iterations`. Rust mirror `crates/reify-eval/src/persistent_cache.rs:450-457` declares only `displacement, stress, max_von_mises, converged, iterations, solve_time_ms`. Neither has `error_indicator`, `global_relative_energy_error`, or `convergence_status`. Decomposition task #3 (per PRD §"Task decomposition") owns this extension.
- **Blocks:** M-006 (refinement-loop control reads it), M-014 (GUI), M-009 (CI gate).
- **Note:** Two encoding notes inherited from existing stdlib (already documented at lines 17-35): `Field<X,Y>` in `param` position not yet resolved (TODO(field-in-param), task #3117) — `error_indicator: Field<Element, ScalarPressure>` is NOT encodable today. *(2026-05-27 update: value-default `= auto` on a `Number`-typed param IS supported via `auto_keyword` in grammar.js; it was incorrectly noted as "not in grammar". The param-default `= auto` form has always parsed. Broader binding-site coverage is being addressed by `docs/prds/auto-binding-site-positions.md`, α task 3802 landed.)* PRD's `target_accuracy : Number = 0.05` is encodable.

### M-006: `ElasticOptions` extension (target_accuracy, max_refinement_iterations, max_dofs, target_quantity_of_interest)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Stdlib `structure def ElasticOptions` at `crates/reify-compiler/stdlib/solver_elastic.ri:149-221` declares 11 params (element_order, mesh_size, max_iter, cg_tolerance, threads, shell_threshold, shell_voxel_size, shell_branch_prune_ratio, shell_force, force_tet, require_hex_wedge). None of the four v0.4 fields exist. No `QoIDescriptor` type stub exists.
- **Blocks:** M-007 (loop), M-013 (auto-resolve), M-008 (mesher size hints), M-018 (DWR future-proof).
- **Note:** Grammar limitation (no `1e6` scientific-notation literal — stdlib line 33-34) means `max_dofs : Integer = 5_000_000` is OK (digits + underscores), but `cg_tolerance: 1e-6` had to become `0.000001` — PRD's defaults all encode but will look uglier than the prose.

### M-007: Refinement loop control (outer solve→estimate→mark→refine→re-solve) + Dörfler θ=0.5 marking

- **State:** FICTION
- **Failure mode:** F3 (algorithmic primitive absent; PRD assumes a refinement-loop runtime)
- **Evidence:** Decomposition task #2. The v0.3 `progressive.rs` `should_refine` (`crates/reify-solver-elastic/src/progressive.rs:235-261`) is a single-decision oracle (continue vs terminate), gated only by `RefinementDemand::{None, More}` + `near_constraint_boundary` (yield-stress proximity). It does NOT mark elements, does NOT consume an indicator field, does NOT implement Dörfler. Grep for `dorfler|Dorfler|mark_elements|h_refine|h-refine` across the workspace: zero hits. Stall-termination (PRD: "if global indicator drops <10% iter-over-iter, stop"): no implementation.
- **Blocks:** M-013 (composition), M-014 (per-iteration GUI surface), M-016 (validation).
- **Note:** The v0.3 progressive-solve loop concept is **mesh-tol halving + CG-tol tightening per level** — NOT element-marked refinement. The v0.4 PRD describes a fundamentally different loop: per-element indicator → Dörfler-marked subset → mesher emits refined mesh. The shared name "progressive" papers over real algorithmic divergence.

### M-008: Gmsh size-field-driven local refinement (per-vertex size hints + mesh content hash → cache key)

- **State:** FICTION
- **Failure mode:** F4 (FFI surface absent for the PRD's stated mesher interaction)
- **Evidence:** Decomposition task #4. Gmsh adapter exposes only global scalar `Mesh.MeshSizeMin/Max` (`crates/reify-kernel-gmsh/src/kernel_real.rs:149-150`, `mesh_profile_2d.rs:93-94`). No `Mesh.BackgroundField`, no `gmsh::view::add`, no per-vertex size FFI binding (zero hits for `BackgroundField | setSizeAtPoints | setSizeAtVertex | PostView`). Cache-key composition at `crates/reify-kernel-gmsh/src/cache_key.rs` keys on `(surface_hash, mesh_size: Option<f64>, element_order)` — no input slot for a per-vertex size field, so even adding the FFI surface needs cache-key extension.
- **Blocks:** M-007 (loop has nothing to call); MMG3D bookmark (M-017) is the contingency.
- **Note:** This is the most under-scoped mechanism in the PRD. The "extends task #2925" framing implies an incremental addition; in reality it's a new FFI surface + cache-key bus + content-hash extension. The PRD's >30% remesh-wallclock criterion for the MMG3D swap is a useful escape hatch but doesn't reduce the v0.4 implementation cost.

### M-009: CI gate reads `convergence_status` + measures Z-Z indicator-drop rate vs L-shaped asymptote

- **State:** FICTION
- **Failure mode:** F5 (test-harness primitive absent)
- **Evidence:** Decomposition task #7. Grep `L-shape | plate.with.hole | cantilever.*tip | MMG3D` in tests: one unrelated hit `crates/reify-solver-elastic/tests/shell_benchmarks.rs:1375` (flat-plate-cantilever-shell — not the smooth tet control case the PRD wants); a `plate_with_hole` fixture exists for `reify-mesh-morph` calibration (`crates/reify-mesh-morph/tests/calibration.rs`) but is a fixture for morph, not an FEA convergence-study fixture. No `convergence_status` reader (M-004 absent), no asymptotic-rate measurement, no CI gate.
- **Blocks:** PRD acceptance.
- **Note:** Validation fixtures (L-shape, plate-with-hole, cantilever) can ride on existing geometry but the analytical-rate scaffold + asymptotic-fit code does not exist.

### M-010: Per-element indicator visualised on the surface mesh

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §"Error norm": "visualisable per-element field on the surface mesh". `MeshData.scalar_channels: HashMap<String, Vec<f32>>` (`gui/src-tauri/src/types.rs:193`) is per-vertex and supports arbitrary string keys, so the IPC wire is generic enough — but no engine-side code populates a `errorIndicator` key (grep across full repo for `errorIndicator | error_indicator`: zero hits in non-test code, single hit `crates/reify-solver-elastic/src/error_estimator.rs:37` in a doc comment), and surface-mesh projection of a *per-element* (not per-vertex) field needs an explicit element-to-surface-vertex aggregation rule that is undefined here.
- **Blocks:** M-014 (GUI dropdown).
- **Note:** Per-vertex vs per-element semantics are silently glossed in PRD §"GUI integration"; current scalar_channels are length=`vertices.len()/3` (vertex-count), so a per-element field needs a kernel-side projection helper.

### M-011: `Field<Element, X>` type resolution

- **State:** TODO
- **Failure mode:** F1
- **Evidence:** PRD specifies `error_indicator: Field<Element, ScalarPressure>` and `per_element_indicator: Field<...>`. Stdlib `solver_elastic.ri` (lines 230-244) documents `TODO(field-in-param, task #3117)` and `Element` / `ScalarPressure` types: zero workspace hits. `crates/reify-compiler/src/type_resolution.rs:1340-1398` (cited by stdlib) handles param-position Field only partially.
- **Blocks:** M-002, M-005.
- **Note:** Task #3117 may already cover this; PRD does not even name it as a prerequisite.

### M-012: `QoIDescriptor` stub enum (DWR future-proofing)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §"DWR future-proofing": "`QoIDescriptor` type in v0.4 is a stub enum with no variants". Grep `QoIDescriptor | quantity_of_interest`: zero hits anywhere in the workspace.
- **Blocks:** none today (PRD explicitly accepts-but-ignores); blocks v0.5+ DWR work.
- **Note:** Cheapest mechanism in the PRD to land — a 1-line `enum QoIDescriptor {}` in stdlib — but currently still absent.

### M-013: Per-probe `target_accuracy` contract + auto-resolve "near-boundary" classification gating refinement

- **State:** FICTION
- **Failure mode:** F2 (auto-resolve loop exists in GUI; per-probe accuracy contract absent)
- **Evidence:** Decomposition task #5. Auto-resolve loop exists in GUI (`gui/src/bridge.ts:585-613` — `onAutoResolveStart`, `onAutoResolveIteration`); GUI panel exists (`gui/src/__tests__/AutoResolvePanel.test.tsx`). Backend `AutoResolveIteration` type and emission live in `gui/src-tauri/`. But: no `target_accuracy` field flows through any probe contract; `near_constraint_boundary` (`crates/reify-solver-elastic/src/progressive.rs:160`) reads only `max_von_mises` + `yield_stress` — there is no consumer that reads the auto-resolve loop's "near-boundary" classification to set `target_accuracy = 0.01`. The two systems are conceptually compatible but not connected.
- **Blocks:** M-007 (needs caller to set target accuracy); GUI flow.
- **Note:** PRD assumes auto-resolve has a "near constraint boundary" classification it can hand the FEA solver. The v0.3 yield-proximity heuristic on `PartialElasticResult` is the closest existing match but is internal-only, not exposed to the auto-resolve scheduler.

### M-014: GUI `errorIndicator` scalar channel in FEA-mode dropdown + probe popup readout

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Decomposition task #6. `gui/src/viewport/FeaModeToolbar.tsx:16` declares `DEFAULT_CHANNELS = ['vonMises', 'displacement_magnitude']` — no `errorIndicator`. Probe popup: no `FeaProbe | ProbePopup | fea-probe` matches in `gui/src` (zero hits). PRD task #2964 (probe popup) is the predecessor and is itself not surfaced anywhere in current GUI source.
- **Blocks:** PRD's "small task" framing depends on M-005, M-010 being WIRED first.
- **Note:** The IPC wire (`scalar_channels: HashMap<String, Vec<f32>>`) is open enough that adding the channel is cheap *once* the engine populates it. But the dropdown is currently hard-coded to two channels, not data-driven from incoming `scalar_channels.keys()`.

### M-015: Lazy-refinement timing + morph cache invalidation on refinement

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Decomposition task #5 (second half). Mesh-morphing crate (`crates/reify-mesh-morph/`) exists with `eligibility`, `elasticity`, `laplacian`, `quality` modules. No refinement-triggered invalidation hook (grep `morph.*cache | cache.*morph | invalidate` in mesh-morph: 3 unrelated comment hits, no code path). No "user-pause" signal anywhere.
- **Blocks:** PRD's perf contract; morph composition test in M-016.
- **Note:** Both halves of this mechanism (refinement firing condition + morph cache invalidation contract) are entirely speculative in code today. The morph crate has no knowledge that a refinement step exists.

### M-016: Validation suite — L-shaped re-entrant corner / plate-with-hole / cantilever convergence studies

- **State:** FICTION
- **Failure mode:** F5
- **Evidence:** Decomposition task #7 (overlaps M-009 CI gate). No L-shaped fixture, no asymptotic-rate measurement, no auto-resolve composition test, no morph composition test. The plate-with-hole fixture in `crates/reify-mesh-morph/tests/calibration.rs` is a morph-only fixture.
- **Blocks:** PRD acceptance.
- **Note:** Distinct from M-009 — M-009 is the CI gate (machinery + threshold); this is the test data + execution. Both are absent.

### M-017: MMG3D mesher swap bookmark (deferred)

- **State:** FICTION
- **Failure mode:** F4 (FFI surface absent; intentionally deferred)
- **Evidence:** PRD §"Deferred bookmark". No `mmg3d` / `MMG3D` / `mmg` crate or binding in the workspace. Existing memory marker `procedural_bookmark_task_pattern` already lists task 3003 (MMG3D swap) as a deferred bookmark. Conditional on M-008's wallclock criterion.
- **Blocks:** none today; future MMG3D-based local-remesh path.
- **Note:** Surfaced for completeness. Marked deferred so not currently a "gap" in a load-bearing sense, but it is the implicit Plan B if M-008 turns out worse than expected.

### M-018: ComputeNode dispatch for `solve_elastic_static` (transitive precondition)

- **State:** FICTION
- **Failure mode:** F6 (ComputeNode infrastructure leaned on but absent)
- **Evidence:** GR-001 transitive (structure constructors return `Value::Undef`). `crates/reify-eval/src/graph.rs:516-519`: "ComputeNodes are not produced by any builder in P3.1". No stdlib `fn solve_elastic_static` declaration anywhere (`grep "solve_elastic_static"` returns only a docstring reference in `crates/reify-compiler/stdlib/fea_multi_case.ri:145`). `solver::elastic_static` target string appears only in cache-key tests and the significance-filter allowlist (`crates/reify-eval/src/significance_filter.rs:76, 343` etc.) — there is no dispatch site that produces an `ElasticResult` from the runtime. Compute-node-infrastructure PRD (`docs/prds/v0_3/compute-node-infrastructure.md`) is the gating prereq; tasks 3379/3383/3384 of that PRD remain pending per audit-brief §"Things to take as given".
- **Blocks:** every v0.4 mechanism that says "extend ElasticResult" or "outer loop solve→refine".
- **Note:** Not strictly inside this PRD's scope, but every mechanism above transitively assumes it. Without this, the entire v0.4 PRD is unimplementable even if every M-001 through M-017 mechanism shipped today.

## Cross-PRD breadcrumbs

- **M-002, M-005, M-011** intersect with `structural-analysis-fea.md` (the source of `ElasticResult`) and likely with `structural-analysis-shells.md` (which generalises the same field-shape decision). Same `TODO(field-in-param, task #3117)` blocker.
- **M-008** intersects with `hex-wedge-meshing.md` (also extends task #2925 / the Gmsh adapter) and the deferred MMG3D bookmark (task 3003 / `procedural_bookmark_task_pattern`).
- **M-013, M-015** intersect with `mesh-morphing.md` (lazy refinement / morph cache invalidation contract). The "lazy refinement at decision time" idea is a cross-PRD architectural pattern, not a single-PRD mechanism.
- **M-014** intersects with `fea-gui-rendering.md` (the v0.3 FEA GUI PRD that the PRD says #2961/#2962/#2964 own) and `fea-gui-rendering-shells.md` (v0.4 shells GUI).
- **M-009, M-016** intersect with `multi-load-case-fea.md` — the multi-case PRD's per-case accuracy guarantees mention reusing the refinement budget.
- **M-018 (transitive)** intersects with `compute-node-infrastructure.md` (the gating v0.3 foundational PRD) and with the v0.4 shells PRD that also depends on engine-side `solve_*` dispatch.

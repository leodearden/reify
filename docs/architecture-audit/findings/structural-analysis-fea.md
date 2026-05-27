# Audit: Structural Analysis (Linear Elastostatic FEA)

**PRD path:** `docs/prds/v0_3/structural-analysis-fea.md`
**Auditor:** audit-structural-analysis-fea
**Date:** 2026-05-12
**Mechanism count:** 28
**Gap count:** 19

## Top concerns

- The headline end-user-visible mechanism — `solve_elastic_static(...)` — has no stdlib `fn` declaration anywhere, no `@optimized` lowering for `fn` context, no dispatch registry, and no consumer of the (partially-built) ComputeNode infrastructure. The PRD's "Sketch of approach" reads as if this signature already exists; in reality it has *no surface at all*. Task 2924 owns the integration; it is gated on six P3.x compute-node-infrastructure tasks (3379-3385) of which 3380/3381/3382/3385 are done and 3379/3383/3384 are pending.
- GR-001 (struct-ctor runtime eval) blocks the material starter library at runtime: `Steel_AISI_1045()` parses fine but yields `Value::Undef`, so even if `solve_elastic_static` existed it could not receive a real `ElasticMaterial` argument. Compounded by the PRD's reference to `material.yield_stress` and `material.youngs_modulus` field access on a runtime-Undef value.
- The PRD describes `Load` and `Support` as stdlib structures (`PressureLoad(face = ..., magnitude = ...)`, `FixedSupport(face = ...)`); the actual stdlib ships them as **builtin name-dispatched constructors** producing kind-tagged Maps (`point_load(...)`, `FixedSupport(...)`) with inconsistent snake_case/PascalCase. Both shapes work today but neither matches the PRD prose exactly — DRIFT, not FICTION.
- Per-PRD `Field<X,Y>` in `param` position is explicitly unsupported (TODO field-in-param, task #3117), so `ElasticResult.displacement / .stress / .frame` are typed `Real` placeholders. Runtime FEA output can populate them via Map-shaped Value, but the type system gives no help to consumers.

## Mechanisms

### M-001: `solve_elastic_static` stdlib `fn` declaration

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no runtime backing)
- **Evidence:** No `fn solve_elastic_static` in any `.ri` under `crates/reify-compiler/stdlib/`; only doc-comment references (`fea_multi_case.ri:145`, `warm_state.rs:82`, `mesh-morph/options.rs:109`). The PRD signature (`body: Body, material: ElasticMaterial, loads: List<Load>, supports: List<Support>, options: ElasticOptions = ElasticOptions.default`) does not exist as a stdlib symbol.
- **Blocks:** 2924 (FEA #16 engine integration); transitively the end-to-end example task #22.
- **Note:** The fn signature is *the* user-visible surface of the entire PRD. Its absence means user `.ri` code calling `solve_elastic_static(...)` today would error at type-resolution (unknown name) before ever reaching any solver.

### M-002: `@optimized("target")` lowering for `fn` context → ComputeNode

- **State:** PARTIAL
- **Failure mode:** F6 (ComputeNode infrastructure leaned on; partly built; no `fn` consumer)
- **Evidence:** `annotations.rs:64-130` accepts `@optimized` on `structure | occurrence | constraint_def | function` (function added per `compute-node-infrastructure.md` resolved decision); `optimized_target` field exists on `CompiledFunction` (`functions.rs:106`, `122`); BUT `eval_user_function_call` (`reify-expr/src/lib.rs:719-769`) ignores `optimized_target` and just inlines the body. No call site routes through `insert_compute_node`. Tasks 3379 (P3.4 dispatch registry + lowering) and 3383 (P3.5 pending+cancellation) are pending per gap-register.
- **Blocks:** 2924; FEA cancellation contract test in task #16.
- **Note:** Field plumbing exists but no execution-path consumer. The `optimized_target` flows through `CompiledFunction` only for record-keeping today.

### M-003: ComputeNode struct + EvaluationGraph integration

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/graph.rs:62-121` (`ComputeNodeData` struct; full field set match the PRD §"Struct shape"); `graph.rs:163-164` (`compute_nodes: PersistentMap`); `graph.rs:513-535` (insert/lookup APIs); P3.1 task 3380 marked done per gap-register.
- **Note:** `CancellationHandle` is a unit-type placeholder pending P3.5; OpaqueState slot exists.

### M-004: ComputeNode cache-key composition

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/compute_cache_key.rs:18+` (cache key from target string + input hashes + options_hash); P3.2 task 3381 done per gap-register; tests `node_a.target = "solver::elastic_static"` round-trips at `compute_cache_key.rs:336`.
- **Note:** Thread count explicitly excluded per PRD `Cache key`. No FEA consumer yet exercises it end-to-end.

### M-005: ComputeNode dependency edges (#6 / #10 / #12) + freshness-walk integration

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/freshness_walk.rs:1330-1469` (`propagate_freshness_only_propagates_through_compute_node_to_output_value_cells`); `dirty.rs:402-810` (multiple ComputeNode-dirty-propagation tests); P3.3 task 3382 done per gap-register.
- **Note:** Edges wired but with no real consumer (FEA dispatch missing); the path is unit-tested via synthetic ComputeNodes.

### M-006: Output significance filter (per-purpose tolerance at FEA boundary)

- **State:** PARTIAL
- **Failure mode:** F6 (built but allowlist contains only `"solver::elastic_static"`, which never fires because dispatch doesn't reach it)
- **Evidence:** `crates/reify-eval/src/significance_filter.rs:1-100` complete (FilterOutcome enum, `is_opted_in` v1 allowlist `"solver::elastic_static"` only, `displacement` field uses Length tolerance, others bit-exact); P3.6 task 3385 done.
- **Note:** PRD §"Tolerance-equivalence at the FEA result boundary" is satisfied at the algorithm layer; no end-to-end test can fire because M-001/M-002 don't dispatch.

### M-007: ComputeNode cancellation contract (in-flight solve cancellation on input change)

- **State:** TODO
- **Failure mode:** F6
- **Evidence:** `graph.rs:62-68` `CancellationHandle` is `#[derive(Debug, Clone, Default)] pub struct CancellationHandle;` — a unit placeholder; `clone_impl` note at `graph.rs:101-104` defers real semantics to P3.5. Task 3383 (P3.5) pending. PRD task #16 explicitly requires "regression test that drives rapid input changes and asserts no orphaned solver threads / memory".
- **Blocks:** 2924 cancellation regression test.
- **Note:** With the long-running solver being the first ComputeNode consumer, this is load-bearing — but currently a structural stub.

### M-008: Pending sentinel propagation while ComputeNode in-flight

- **State:** TODO
- **Failure mode:** F6
- **Evidence:** `compute-node-infrastructure.md` P3.5 open question (`Value::Pending` variant vs reuse of freshness flag) — undecided per gap-register's noting of 3383 still pending; no `Value::Pending` variant search hits.
- **Note:** Downstream consumers (`max_von_mises < yield_stress` constraint) need to handle the "FEA still running" case; currently no surface.

### M-009: Surface-to-volume tet meshing via Gmsh

- **State:** PARTIAL
- **Failure mode:** F4 (real FFI gated on optional dep; stub path always available)
- **Evidence:** `crates/reify-kernel-gmsh/` exists with `cfg(has_gmsh)` real FFI + `cfg(not(has_gmsh))` stub kernel (`lib.rs:1-69`); `mesh_volume.rs`, `auto_size.rs`, `repair.rs`, `through_thickness.rs` present. ReprKind::VolumeMesh exists (`reify-types/src/geometry.rs:114`, `1122`, `1158`). The real-FFI build path depends on `/opt/reify-deps` conda-forge env (project memory `project_conda_forge_native_deps_decision_may07`). PRD §"Resolved design decisions" calls for surface-mesh-repair pre-stage (present: `repair.rs`), through-thickness check (present: `through_thickness.rs`), auto mesh-size from smallest feature (present: `auto_size.rs`).
- **Blocks:** 2924 (need real meshes for the FEA pipeline).
- **Note:** Real path is conditional on the local env. The stub silently degrades to `MeshError` / no-op in CI lacking libgmsh — a real PRD-fidelity test requires the dep installed.

### M-010: `ElasticMaterial` trait + starter material library

- **State:** PARTIAL
- **Failure mode:** F1 (compile-time only — runtime ctor blocked by GR-001)
- **Evidence:** `crates/reify-compiler/stdlib/materials_fea.ri` (full file present: trait `ElasticMaterial`, `Steel_AISI_1045`, `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic`, per-property provenance fields); `Pressure` and `Density` types in `reify-types/src/dimension.rs:374`/`392`. BUT GR-001: `Steel_AISI_1045()` evaluates to `Value::Undef` at runtime (per `engine_eval.rs:114-125`); no `material.youngs_modulus` field access works at runtime.
- **Blocks:** 2924 input plumbing; example task #22; multi-load-case PRD's material flow.
- **Note:** Cite GR-001. Parse-side and type-resolution work for the trait; runtime instantiation is the FICTION.

### M-011: `Load` stdlib hierarchy (PointLoad, PressureLoad, TractionLoad, BodyForce/Gravity)

- **State:** DRIFT
- **Failure mode:** F3 (PRD describes structs; code ships builtin kind-tagged Map ctors)
- **Evidence:** `crates/reify-stdlib/src/loads.rs:32-38` `LOAD_KINDS = ["point_load", "pressure_load", "traction_load", "body_force", "gravity"]` — all snake_case builtins returning `Value::Map` with `kind` key (`make_kind_map`); PRD prose says "`PressureLoad(face = bracket.face("top"), magnitude = ...)`" — PascalCase struct-call syntax that doesn't parse against snake_case builtin names. `LOAD_KINDS` constant has `#[allow(dead_code)]` and a comment "Not yet referenced by any external caller — the FEA solver (PRD task 16) will wire this up when it lands."
- **Note:** Coupled to the FixedSupport/point_load naming inconsistency noted in audit-brief "things to take as given". A "load trait" would unify both surfaces; PRD's struct-syntax is currently fiction; PRD's *behavioral* contract (selector target + dimensioned magnitude + kind discriminator) is wired.

### M-012: `Support` stdlib hierarchy (FixedSupport, DisplacementSupport, RollerSupport)

- **State:** DRIFT
- **Failure mode:** F3 (same shape mismatch as M-011, mixed snake/Pascal naming)
- **Evidence:** `crates/reify-stdlib/src/supports.rs:58-63` `SUPPORT_KINDS = ["fixed_support", "pinned_support", "displacement_support", "roller_support"]` (snake_case) but `eval_supports` dispatch arms use **PascalCase** names `"FixedSupport"`, `"DisplacementSupport"`, `"RollerSupport"` (`supports.rs:111`, `129`); this is the inconsistency called out in audit-brief "Things to take as given". PRD also references `PinnedSupport` (v0.4 shell BC extension landed early).
- **Note:** Same `Load`/`Support` trait-fiction shadow as M-011. Selector-target validation is a narrow opaque pass-through (`Value::Map | Value::String`) pending topology-selector landing.

### M-013: `ElasticOptions` and `ElasticResult` stdlib types

- **State:** PARTIAL
- **Failure mode:** F3 (PRD vs current encoding: see deviations comment in file)
- **Evidence:** `crates/reify-compiler/stdlib/solver_elastic.ri:149-221` ElasticOptions (eleven params; defaults; constraints — superset of PRD #4 because of v0.4 shell + hex/wedge work landed early: `shell_threshold`, `shell_voxel_size`, `shell_branch_prune_ratio`, `shell_force`, `force_tet`, `require_hex_wedge`); `solver_elastic.ri:295-316` ElasticResult; explicitly documented encoding deviations (no `auto` literal, no `num_cpus::get()`, scientific-notation grammar limitation, `Number→Real / Integer→Int` mapping). `ElasticResult.displacement / .stress / .frame` typed `Real` per TODO(field-in-param) task #3117 instead of PRD's `Field<X,Y>`.
- **Note:** Encoding deviations are honest and documented in-file; structurally aligned with PRD intent but blocked from full type-checked round-trip by `Field<X,Y>` in param position (M-022).

### M-014: `von_mises(stress) -> Pressure` and `principal_stresses(...)` tensor reductions

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/analysis.rs:14-15` dispatch arms; `analysis.rs:70+` von_mises tensor impl; `analysis.rs:171+` principal_stresses; `reify-expr/src/analysis.rs:157` Field-arg wrapper `compute_von_mises` returning Field with `FieldSourceKind::VonMises`; comprehensive tests at `analysis.rs:245-450+`.

### M-015: Field `max` / `min` / `argmax` reductions over `Field<_, T : Ordered>`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/field_reductions.rs:1+`; dispatch at `reify-expr/src/lib.rs:359-377` for `"max" / "min" / "argmax" / "argmin"`; tests at `crates/reify-expr/tests/field_analysis_tests.rs`; PRD task #6.
- **Note:** Sampled-source only per noted staging; non-Sampled returns Undef.

### M-016: `reify-solver-elastic` crate skeleton + reference elements (P1/P2 tet)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/elements/{tet_p1.rs, tet_p2.rs, hex_p1.rs, wedge_p1.rs, mitc3_plus.rs}` shape functions, gradients, quadrature; `lib.rs:1-60` re-exports. PRD task #7 + hex/wedge extension.

### M-017: Element-level stiffness assembly (isotropic linear-elastic) + global sparse-matrix assembly via faer

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/assembly/{global.rs, tet.rs, hex.rs, wedge.rs}`; `constitutive.rs` (IsotropicElastic); faer dep in Cargo. PRD tasks #8/#9.
- **Note:** Engineering-strain + Voigt notation, sparse CSR via faer-rs.

### M-018: Dirichlet (row-elimination) + Neumann (surface traction + body force) BC application

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/boundary/{dirichlet.rs, neumann.rs}`; lib re-exports `DirichletBc, apply_dirichlet_row_elimination, apply_body_force, apply_point_load, apply_traction_load`. PRD tasks #10/#11.

### M-019: CG solver with Jacobi preconditioner via faer (parallel default, single-threaded under `#deterministic`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/solver.rs` exposing `solve_cg`, `solve_cg_warm`, `CgSolverOptions`, `CgResult`, `SolverMode`; warm-state hook `solve_cg_with_warm_state` at `warm_state.rs:99-113`. PRD task #12.

### M-020: Warm-state plumbing — prior-iterate carry-through, OpaqueState attached to ComputeNode

- **State:** PARTIAL
- **Failure mode:** F6 (numerics side complete; engine attachment depends on M-002)
- **Evidence:** `crates/reify-solver-elastic/src/warm_state.rs:1-113` (`CgWarmState`, `into_opaque_state`, `from_opaque_state`, `solve_cg_with_warm_state`); `crates/reify-eval/src/warm_pool.rs` WarmStatePool. PRD task #14 numerics shipped. BUT no `@optimized fn` consumer routes through `ComputeNodeData.opaque_state` slot today; tied to M-002.
- **Note:** WarmStatePool donate/checkout cycle works for existing consumers (resolution, realization); FEA consumer waits on dispatch.

### M-021: Progressive solve framework (coarse-mesh + loose-CG first pass; refinement passes)

- **State:** PARTIAL
- **Failure mode:** F6 (numerics shipped; ComputeNode `progressive` trait integration absent)
- **Evidence:** `crates/reify-solver-elastic/src/progressive.rs`; lib re-exports `ProgressiveOptions, PartialElasticResult, PassTuning, RefinementDemand, TerminationReason, AdvanceDecision, coarse_pass_tuning, refinement_pass_tuning, should_refine`. PRD task #15.
- **Note:** PRD calls for "Implements ComputeNode `progressive` trait" — the trait is `WARM_STARTABLE | COMMITTABLE` per arch §7.6; the `progressive` annotation surface is undefined in current ComputeNode infra (no trait-combination registry).

### M-022: `Field<X,Y>` in `param` position (type-resolution)

- **State:** TODO
- **Failure mode:** F1 (compile-time contract gap; documented TODO)
- **Evidence:** `crates/reify-compiler/stdlib/solver_elastic.ri:231-244` explicit `TODO(field-in-param, task #3117)`; `crates/reify-compiler/src/type_resolution.rs:1340-1398` resolves `field def` only; param-position `Field<X,Y>` falls through. Result: `ElasticResult.{displacement, stress, frame}` and `ShellStress.{top, mid, bottom}` typed `Real`.
- **Blocks:** 6 stdlib slots in `solver_elastic.ri` per the TODO block.
- **Note:** Cross-PRD breadcrumb: shells, multi-load-case, error-estimation all share the same TODO chain.

### M-023: `#deterministic` pragma plumbing

- **State:** PARTIAL
- **Failure mode:** F1 (compile-time recognized; runtime threading-mode flag plumbing partial)
- **Evidence:** `compile_builder/hash.rs:86` deterministic pragma content-hash; `reify-kernel-gmsh/src/options.rs:28-30` "Whether the user requested bit-deterministic mesh output — under `#deterministic` the cache returns bit-identical bytes" — flag NOT in cache key (correct per PRD); test at `kernel-gmsh/tests/cache_key_tests.rs:105`. Solver-side `SolverMode` exists (`solver.rs` SolverMode enum re-exported in `lib.rs:43`). PRD task #18.
- **Note:** Pragma is parsed and propagated to mesher options; solver `SolverMode` exists. Need to verify end-to-end coupling through `solve_elastic_static` (blocked by M-001).

### M-024: Determinism harness (`#deterministic` bit-stable; default parallel tolerance-equivalent)

- **State:** TODO
- **Failure mode:** F1
- **Evidence:** No `tests/` file matching determinism harness across thread counts in `reify-solver-elastic` or `reify-kernel-gmsh`. PRD task #19 — gated on M-001/M-002 dispatch landing.
- **Note:** Pieces exist (cache-key tests, SolverMode); no integration harness.

### M-025: Validation suite against analytical references (cantilever / thick-walled cylinder / simple shear / Boussinesq)

- **State:** PARTIAL
- **Failure mode:** F1 (one of four cited references covered; rest absent)
- **Evidence:** `crates/reify-solver-elastic/tests/shell_benchmarks.rs:1359-1421` flat-plate cantilever (shell variant). No grep hits for `Boussinesq`, `thick_walled`, `pressurised_cylinder`, `simple_shear` in the solid-tet validation suite. MacNeal-Harder twisted-cantilever (`shell_benchmarks.rs:650+`) is shell-side. PRD task #20 calls for solid-FEA cantilever beam (tip deflection), pressurised thick-walled cylinder (radial stress profile), simple shear (uniform stress), Boussinesq half-space point load — at both P1 and P2.
- **Note:** Solid-FEA tet-side validation missing. Shell validation more advanced than solid; inversion of PRD priority.

### M-026: Diagnostic mapping for common failure modes (under-constrained, singular K, non-convergence, BC errors, thin-body advisory)

- **State:** PARTIAL
- **Failure mode:** F1
- **Evidence:** `crates/reify-eval/tests/kinematic_diagnostics_e2e.rs:100` over/under-constrained constraint diagnostic; rigid-body null-space tests in `assembly/tet.rs:354-502`. No grep hits for a Reify-diagnostic-emitting under-constrained-FEA layer, no thin-body aspect-ratio advisory, no "BC on interior face" diagnostic. PRD task #21 owns the user-facing diagnostic layer; appears unstarted on the solid side.
- **Note:** Algorithm-level checks exist (null-space detection); user-visible mapping does not.

### M-027: End-to-end example file (`param thickness=auto, minimize mass s.t. max(von_mises) < yield_stress`)

- **State:** FICTION
- **Failure mode:** F1 (example file absent; `s.t.` / `subject to` syntax fictional)
- **Evidence:** No `.ri` file under `examples/` or `prj/` invokes `solve_elastic_static`. PRD task #22 — blocked transitively on M-001. *(2026-05-27 update: `param thickness : Length = auto` is grammar-supported at the param-default position via `auto_keyword` — this sub-mechanism is NOT the source of the FICTION classification. The FICTION state is driven by the absent example file and the fictional `s.t.` / `subject to` syntax. Broader `auto` binding-site coverage is addressed by `docs/prds/auto-binding-site-positions.md`, α task 3802 landed.)*

### M-028: Auto-resolve loop integration — `param thickness : Length = auto` driving FEA constraint

- **State:** PARTIAL
- **Failure mode:** F6 (auto/minimize machinery exists from v0.1; FEA feed-in path doesn't exist)
- **Evidence:** Auto-resolve loop is a v0.1 feature, working for constraint-typed cells. `param thickness : Length = auto` is grammar-supported at the param-default position (via `auto_keyword` — this is NOT the source of the PARTIAL/F6 state). PRD §"Goal" calls for `minimize mass subject to max(von_mises(bracket)) < material.yield_stress` — the remaining gaps are (a) `material.yield_stress` runtime access (blocked by GR-001), (b) `max(von_mises(field))` plumbing (works for sampled fields per M-014/M-015), (c) FEA ComputeNode populating the field on each auto-resolve iteration (blocked by M-001/M-002). *(2026-05-27 update: broader `auto` binding-site coverage beyond param-default is addressed by `docs/prds/auto-binding-site-positions.md`, α task 3802 landed.)*
- **Note:** Each piece tested in isolation; integration is the empty case. The grammar/literal portion of `param ... = auto` is supported.

## Cross-PRD breadcrumbs

- **GR-001 transitively touches every multi-load-case / shells / mesh-morph / error-estimation PRD** that consumes `ElasticMaterial`, `ElasticResult`, `LoadCase`. Multiple PRDs assume `Steel_AISI_1045()` evaluates to a usable runtime value.
- **`docs/prds/v0_3/compute-node-infrastructure.md`** owns M-002/M-003/M-004/M-005/M-006/M-007/M-008 (P3.1-P3.6, tasks 3380/3381/3382/3379/3383/3385); the FEA PRD assumes all six landed.
- **`docs/prds/v0_2/per-purpose-tolerance.md`** owns the tolerance-scope machinery the FEA cache and significance filter consume.
- **`docs/prds/v0_2/topology-selectors.md`** owns the `bracket.face("top")` machinery the load/support constructors target — currently opaque-pass-through (`Value::Map | Value::String` placeholder per `helpers.rs:207`).
- **`docs/prds/v0_3/structural-analysis-shells.md`** has landed substantial code ahead of solid-FEA validation: MITC3+, shell BCs, shell benchmarks (MacNeal-Harder), `ShellStress` struct, `shell_threshold` / `shell_force` options. Inversion of expected ordering — flagged.
- **`docs/prds/v0_3/hex-wedge-meshing.md`** has landed P1 hex + P1 wedge elements in the solver crate per `lib.rs:9-10`; `force_tet` / `require_hex_wedge` knobs added to ElasticOptions. Same observation.
- **`docs/prds/v0_3/multi-load-case-fea.md`** ships `LoadCase` / `MultiCaseResult` stdlib + `envelope_max/min` / `linear_combine` / `worst_case` / `case_names` / `result_for` (`crates/reify-stdlib/src/fea.rs`) — but the `solve_load_cases(...)` producer fn declaration is also missing (parallel FICTION to M-001).
- **Task #3117** (Field<X,Y> in param) is the umbrella for M-022 — touches all six field-slots in `solver_elastic.ri`.

## Summary numbers (for fused-memory summary memory)

- Mechanism count: 28 (numbered M-001..M-028)
- WIRED: 7 (M-003, M-004, M-005, M-014, M-015, M-016, M-017, M-018, M-019) — actually 9; recount below
- Let me recount: M-003 W, M-004 W, M-005 W, M-014 W, M-015 W, M-016 W, M-017 W, M-018 W, M-019 W → 9 WIRED
- PARTIAL: M-002, M-006, M-009, M-010, M-013, M-020, M-021, M-023, M-025, M-026, M-028 → 11 PARTIAL
- TODO: M-007, M-008, M-022, M-024 → 4 TODO
- FICTION: M-001, M-027 → 2 FICTION
- DRIFT: M-011, M-012 → 2 DRIFT
- ORPHAN: 0
- Total: 9 + 11 + 4 + 2 + 2 = 28 mechanisms; 19 gaps (non-WIRED).

(Header counts adjusted to match — mechanism count is 28, gap count is 19.)

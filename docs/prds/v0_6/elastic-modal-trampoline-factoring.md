# Elastic-static and modal trampoline factoring; mesh-morph observability

**Milestone:** v0_6 · **Status:** active (authored 2026-10-14; decomposed 2026-10-15). The leaf ids are in §10 and the sidecar: α #9085, β #9092, γ #9086, δ #9087, ε #9088, ζ #9089, η #9090, θ #9093, ι #9094, κ #9102, λ #9118, μ #9119, ν #9091, ξ #9120, ο #9121, ω #9122. Subsumed and cancelled: #7835 (→ α #9085), #8423 (→ ζ #9089), #8427 (→ θ #9093), #7760 (→ ν #9091). Gate #8612 ← κ #9102, λ #9118 · gap-fill 2026-10-16: K5-1 (C6 result_value row, §2 cycle hazard), K5-3 (§12 OQ2; ξ #9120, ο #9121), K5-4 (ξ #9120 check `boundary-conditions-echo-from-realization`), K5-2 notes (support-realization-policy stream) · **Date:** 2026-10-14 · **Approach:** B + H (contract and two-way boundary tests). FEA and ComputeNode dispatch are overlay-listed load-bearing seams; the blast radius is reify-analysis (after S5), reify-eval, reify-solver-elastic, reify-ir, reify-mesh-morph, reify-compiler's stdlib, reify-cli and gui.

**Program:** `plans/review-all-program-reify-20261012.md`, stream **S16** (wave 3, spawn, size L, risk medium). The stream's remedy, deletable list and seams were ruled in that document's "S16 elastic_static and modal trampoline factoring; mesh-morph observability" decision, and this PRD does not reopen them. Leo ruled this session's three design questions on 2026-10-14/15 (§13).

**Code anchors** were verified against main `1b06f93151` (2026-10-14). Every S16 path is byte-identical between the review's pinned tree `a030ee7528` and `1b06f93151`, except `reify-solver-elastic/src/assembly/global.rs` (#8373) and `gui/src-tauri/src/debug_server.rs` (#7816), neither of which moves an S16 symbol. Upstream S15 and S5 leaves rewrite and move most of these files before S16's elastic and modal leaves dispatch (§8). Cite by symbol, and re-locate each symbol at implementation time.

**Normative substrate:** `docs/legibility/design-invariants.md`.
- Every extracted primitive ships with an adoption-ratchet row (INV candidate 5).
- Every split lands with no new module SCC (INV-ARCH-1; S3's `scripts/assert-module-scc.py`).
- Every new Warning or Error is coded (INV-SF-6). Nothing is dropped silently (INV-SF-5, `nothing-vacuous-and-unowned`).
- The elastic result is written by S15's one builder (INV-PD-2's runtime half).
- Quality heuristics are cited by name from `/home/leo/src/dark-factory/docs/code-quality.md`.

---

## 1. Goal

The elastic-static and modal compute targets become code an agent can change one axis at a time, and mesh-morph's outcomes reach their consumers as values. After this PRD:

1. **The elastic overload is explicit.**
   - Each `.ri` overload of `solve_elastic_static` (dims, Field, body) and of `solve_load_cases` (dims, body) has its own `@optimized` target and its own descriptor row.
   - Each entry decodes its own argument layout into one typed `ElasticSolveRequest`.
   - No code infers the overload by sniffing `Value` variants. The `body_path` flag, its five readers and its copy in `multi_case.rs` are gone.
2. **One route decision.**
   - `plan_elastic_route` turns a request into an `ElasticRoute` (shell, or tet with an adaptive lane), or refuses with a coded diagnostic.
   - The trampoline and the dims row's upstream planner (S5 ε) call the same function, so the inserted shell-extract node and the solve route cannot disagree.
   - Each route has its own solver returning a typed solution. S15's `build_elastic_result` turns either solution into the Value at one site.
3. **Nothing reaching a dims route is dropped or approximated silently.**
   - The shell route declares what it realizes. Some inputs are realized by tet but not by the shell model: a pressure, gravity, the in-plane component of the tip force, a non-isotropic material, or a coordinate kind. A scene with one is routed to tet (`ShellForce::Auto`, coded Warning) or refused (`ShellForce::On`, coded Error).
   - A declared support that no dims route can realize is a coded Error naming it: any target other than `"root"` / `"x_min"`, or a kind other than Fixed at the root. The model would otherwise answer a different structure (§13 R3).
   - An empty supports list keeps today's under-constrained advisory, because the root clamp is the dims overload's documented default.
4. **The cantilever solve has a typed request.** `CantileverSolveRequest` replaces the 14 positional parameters. Tests build it with named fields, not runs of positional `None`.
5. **The two analysis files are partitioned along their measured seams.**
   - `elastic_static.rs` (15,338 lines) and `modal_ops.rs` (12,835 lines) become directory modules whose files each make sense alone.
   - No module SCC is introduced.
   - Supports and damping in modal get narrow seams their tests drive.
   - The FEA `sampled_*` helpers move to a leaf `sampled_channels.rs`.
   - The trampoline reads no private adaptive-problem field.
6. **One copy of each shared primitive.**
   - One structured box mesh generator with one box-face vocabulary, used by modal, elastic-static and the solver tests.
   - One production P2 promotion, so no production code imports `test_support`.
   - One `value_size_estimate`, in `reify_ir::warm` beside `OpaqueState`.
7. **No test-only state on production types.** The `reused*` flags on three run structs are deleted. Cache-hit tests assert through the public trampoline with a poisoned prior.
8. **Dead and misleading surfaces go.**
   - `worst_case_residual_fraction` and its false wiring claims are deleted.
   - mesh-morph's `stats.rs`, the `morph_stats` RPC and the `eligible()` wrapper are deleted, so `compose_morph` is the single public entry. `morph()` itself is deleted by #8527.
9. **Morph outcomes are per build.**
   - `BuildResult` carries a typed per-build morph tally.
   - The CLI prints it, and cross-crate tests assert on it without a process-global lock.
   - The process counters remain only as the telemetry sink behind `mesh_morph_stats`.

**Consumers (G1).**
- The Engine's ComputeNode dispatch (`docs/prds/v0_3/engine-integration-norm.md` §3.4) consumes the descriptor rows of goal 1.
- The S5 ε generic upstream routine consumes the dims row's planner (goal 2).
- `.ri` authors observe goals 3 and 9 through `reify eval` / `reify build --verbose` diagnostics and output.
- The GUI's mesh-morph debug RPC `mesh_morph_stats` and the CLI's `--verbose` morph summary consume goal 9.
- Pending feature tasks consume goals 4–6. They land their next change into one file and one typed field instead of the 1,161-line trampoline or a 14-argument call: coordinate-target (#8253, #8256, #8257), DWR #7453, flexible-modal #7142 and #8247, meshing #8289/#8290, static P2 #7075.

## 2. Background (verified on main `1b06f93151`)

Five read-only scout seats re-measured every finding in `plans/review-all-reify-20261010.json` at main. The facts that carry the design are below.

**The elastic trampoline (fk-3f6ee7ecd393, high).**
- `solve_elastic_static_trampoline` runs 1,161 lines: 573 comment, 36 blank, 552 code. It has 11 `return` tokens plus the tail, and five more exits through `gate_or_fail!`.
- **Overload:** `body_path` is `[1] is GeometryHandle | Undef && [2] is List`. It has five readers: the definition, the dims rebinding, the forced Tet route and two advisory guards.
  - The Field overload is recognised only by `classify_material` returning `Heterogeneous` and by two raw `AsPrintedZones` sniffs.
  - `value_inputs[5]` is indexed unguarded.
  - `multi_case.rs` repeats the identical sniff and calls the trampoline with re-spelled per-case `Value` layouts.
- **Registry:** `ComputeDispatchRegistry.fns` is keyed by target string, and a duplicate registration panics. `ComputeFn` has no overload channel, and the `options` argument is always `Undef`. So an explicit overload needs its own target string.
- **The three `.ri` overloads share one target string.** All three carry `@optimized("solver::elastic_static")` in `solver_elastic.ri`: dims (arity 7 with a defaulted `options`), Field (arity 6, no options) and body (arity 5). `solver::multi_case` has two overloads (dims and body) under one string in `fea_multi_case.ri`.
- **Route, decided twice.**
  - The trampoline decides through `extract_shell_route_params` and `classify_shell`, with body forced to Tet.
  - `engine_eval::insert_shell_extract_upstream` decides again. It returns `None` for fewer than 7 arguments and ignores the material model.
  - For a non-isotropic material classified Shell, the planner inserts the shell-extract node while the trampoline falls back to tet. The planner's doc says the two "always agree".
- **The shell arm** calls `solve_shell_static` → `reify_solver_elastic::solve_flat_plate_shell`.
  - This solves a synthetic 20-column plate with a hard-coded root clamp, pinned drilling DOFs and only the transverse tip force.
  - Pressures and gravity are silently unused. In-plane tip components get a Warning.
  - User supports feed only the under-constrained advisory, through the `"root"` convention `SYNTHETIC_CLAMP_FACE`.
- **Probed at main with `target/debug/reify` (2026-10-12 build; the elastic files are unchanged since `a030ee7528`).** On a shell-classified 50 × 10 × 1 mm plate with a 10 N tip load, `max_von_mises` is 2.143e9 Pa in all four scenes, and none of them gets a diagnostic about its extra input:
  - the root clamp alone;
  - plus 1000 g of `Gravity`;
  - plus a 1 MPa `PressureLoad` on `z_max`;
  - plus `PinnedSupport(target: "x_max")`.

  `[FixedSupport("x_max")]` alone gets the `FeaUnderConstrained` advisory ("the root face is auto-clamped but results may not reflect design intent"). Five evidence fixtures for μ are committed with this PRD in `tests/prd-gate/fixtures/`:
  - `shell_route_gravity_thin_plate.ri`;
  - `shell_route_pressure_thin_plate.ri`;
  - `dims_support_root_plus_xmax.ri`;
  - its thick, tet-routed twin `dims_support_root_plus_xmax_thick.ri`, where `max_von_mises` is 2.518e6 Pa with or without the `x_max` support;
  - `dims_support_xmax_only.ri`.
- **The same plate with `ShellForce.Off` (tet)** realizes both loads:
  - `max_von_mises` is 5.22e7 Pa for the tip load alone, 2.15e8 Pa with gravity and 2.13e9 Pa with pressure;
  - CG stops at its 2,000-iteration cap;
  - the thin-body advisory recommends `ShellForce.On`.

  The tet value is 40× below the shell's for the same tip load, because P1 tets lock on a 1 mm plate (overlay G6 hazard).
- **Supports on the dims tet route** are ignored the same way.
  - `bc_override` exists only with a realized mesh, which only the body overload supplies.
  - Solver-performance's D3 run printed the same displacement for root-only, root plus a tip `PinnedSupport`, and tip-only.
  - Every elastic dims example writes `FixedSupport(target: "root")`, except `examples/multi_load_bracket.ri` (`FixedSupport("mount_face")`, its cantilever mount).
- **Results:** `ElasticResult` is built twice, on the shell arm and the tet arm. Both have the same 16 keys; the arms differ in which are `Undef`.
- **Adaptive lanes.**
  - `AdaptiveLaneOutcome` is a struct. The lane choice is implicit in an `Option<RealizedAdaptiveProblem>` and a `run_uniform_lane` closure.
  - The trampoline reads `CantileverAdaptiveProblem`'s private `grid` and `iterates.{last_n_dofs, last_global_indicator, refine_count}`.
  - After a localized-lane failure with a realized mesh present, the uniform fallback solves the placeholder synthetic box, and its iterate is then dropped.
  - `solve_elastic_static_body_e2e.rs` parses the `elements N -> M` tail of a diagnostic message.
- **Comments:** two `PERF NOTE` blocks and eight more reviewer-addressed blocks.

**The cantilever solve (fk-9facd9401295, weakened).**
- `solve_cantilever_fea` has 14 parameters. Its `allow(too_many_arguments)` comment says "10 args" and argues against a struct.
- It has 3 production callers and 20 test callers; every test call spells at least four positional `None` (109 literal `None` arguments).
- `CantileverFeaSolve`'s `allow(dead_code)` claims fields are test-only. Every field but `tip_nodes` has a production reader (the skeptic's narrowing).
- `realized_solver_mesh` has no caller. `extract_pressure_loads` is `#[cfg(test)]` in the production half. `shell_solve::solve_shell_static`'s `allow(dead_code)` is stale.

**The elastic partition (h14 measurement, 7 partitions).**
- The partition is: trampoline and result channels, realized mesh, result value, cantilever solve, adaptive, value readers, tests.
- A file split as-is fails heuristic 13 in three places:
  - result channels reach back into `compute_targets/mod.rs`'s `sampled_*` helpers;
  - `MaterialModel` / `PressureSpec` close a readers ↔ solve cycle;
  - the trampoline reads adaptive private state.
- The 9,538-line test module glob-imports everything, and `multi_case.rs`'s tests re-copy `make_box_tet_volume_mesh`.
- **`sampled_*` users:** the `sampled_*` helpers are used only by FEA-family files. `flatten_nodal_stress`, `sampled_disp_field` and `sampled_stress_field` also serve `buckling.rs`; `sampled_stress_field` also serves `shell_solve.rs`. They have no modal or multi-case caller.
- **Existing split task:** #8427 (low) plans the same split as `cantilever_solve ← elastic_adaptive ← trampoline`, after #8248 and #8257.

**The modal file (fk-7687550ec80f).**
- `modal_ops.rs` is 12,835 lines: 4,547 production and 8,288 test. It hosts four targets, but its module doc names only `modal::free_vibration`.
- The measured partition holds exactly; every production item is assigned:

  | Partition | Production lines |
  |---|---|
  | core | 1,100 |
  | result_value | 183 |
  | options | 188 |
  | damping | 368 |
  | free_vibration | 490 |
  | mechanism | 688 |
  | transient | 742 |
  | supports | 733 |

- **Cycle hazard (dropped 2026-10-16, gap-fill K5-1):** the damping ↔ free_vibration cycle existed only while `degenerate_modal_result` stayed in free_vibration. S15 ζ₀ #9287 deletes `degenerate_modal_result` and `degenerate_displacement_history` (modal error paths return `ComputeOutcome::Failed`), so the cycle and this rationale go with them.
- **Test imports:** the test module imports 47 names, 30 of them private. About 16 tests call supports internals directly (`face_company`, `face_realization`, `nearest_node`, and `build_dirichlet_bcs` through `BcFixture`), and 4 call damping internals.
- **Existing split task:** #7760 (low) plans a behaviour-preserving extraction.

**Duplicated primitives.**
- **The beam mesh (fk-603bbbab0515).**
  - The six-tet Freudenthal table literal appears 7 times: modal `build_beam_mesh`, the `None =>` mesher arm of `solve_cantilever_fea`, and five tests, three of them in reify-solver-elastic.
  - Grid counts differ by design: elastic clamps `nx` to `NX_MAX = 120`, modal does not.
  - Coordinate-target ζ #8252 (C8) unifies the *counts* into one `synthetic_grid` that keeps exactly today's counts per caller (its BT10). The tet generator stays duplicated.
  - The `x_min`…`z_max` face vocabulary is written twice: modal `face_bound` and elastic `box_face_plane`.
- **P2 promotion (fk-cecdff4f1373).**
  - `modal_ops::promote_beam_mesh_to_p2` and `buckling.rs` call `reify_solver_elastic::assembly::test_support::promote_tets_to_p2` in production. That module is `#[doc(hidden)] pub`, documented test-only, and carries `#![allow(dead_code)]`.
  - #8423 (low) covers it, but its premise is false: the `P2TetMesh::promote` it switches to (#7075) is not on main.
- **`value_size_estimate` (fk-12285c864993).**
  - It exists as three private copies: `trajectory_ops.rs`, `dynamics_ops.rs` and `fdm_slice.rs`. Only the `fdm_slice` copy recurses into `Point`/`Vector`.
  - `OpaqueState` lives in `reify_ir::warm`. The finding's "warm-pool owner" (`WarmStatePool`) is reify-eval-private, so reify-analysis could never reach it.

**Test-only state (fk-43921c86c7d3).**
- S5 λ #9002 already splits `dynamics_ops.rs` into its mass-props and trampoline halves.
- What remains: `InverseDynamicsRun.reused` (5 test reads), and modal's `ModalTrampolineRun.reused_assembly` and `TransientTrampolineRun.reused_setup` (13 test reads). Each carries an `allow(dead_code)`.
- No public `ComputeOutcome` field distinguishes a hit from a miss. But a poisoned prior does: a hand-built cache whose key matches the new inputs and whose payload does not. On a hit the result comes from the poisoned payload.

**The dead trajectory helper (fk-baa063db2052).**
- `worst_case_residual_fraction` has 0 production callers.
- Its doc, its G-allow and four more sites claim a consumer: the registration comment, two reify-stdlib docs, `input_shape.rs`, and an audit module doc.
- `new_orphans_2026_06_02_g_allow.rs` pins it.
- Deleting it leaves `ImpulseTrain::residual_vibration` reachable only from tests, and the `shaper_damping_ratio` re-export unused.

**mesh-morph (fk-acc10f2161f6, fk-d632489a4c98, fk-e3acedd1c27a).**
- **`stats.rs`** has no production writer, yet `lib.rs` re-exports its `snapshot` at the crate root. The `morph_stats` RPC serves it and always returns zeros, and nothing in the frontend listens.
- **#7835** (low) plans to delete `stats.rs` and the RPC, and to add an eighth bucket: compose_morph records nothing for `Unsupported` and non-panic `SolverError`.
- **`eligible()`** has no production caller.
- **#8527** deletes `morph()` and re-homes the three eligibility tests onto `compose_morph`. It sits on `task/8527` (7 commits, unmerged, in the merge phase), and its tests assert through the global counters.
- **The engine seam** is `decide_morph_or_remesh` in `morph_producer.rs`. It sees every outcome, but it receives them flattened to `MorphResult` strings: reify-mesh-morph depends on reify-eval, so reify-eval cannot name mesh-morph's types. It records nothing per build.
- **Where it fires:** the morph arm runs only on `build` / `build_snapshot`; tessellation passes `MorphDispatchIo::disabled()`.
- **Global consumers:** the CLI prints `format_summary(&snapshot())` after builds. Three independent test locks guard the one global.

**Already filed, referenced here and not re-filed.**
- fk-d49d3ac15f14 (`tots.rs`) is #8546.
- fk-d632489a4c98 (`morph()`) is #8527.

## 3. Sketch of approach

Four lanes. Lane A waits only on #8527 and S1 θ #9046. Every other leaf follows the S5 move of the file it edits (program order S15 → S5 → S16, plus Leo's ordering ruling §13 R2), so each leaf edits a file once, at its final crate path.

**Lane A — mesh-morph (early).**
- **α** retires `stats.rs`, the `morph_stats` RPC and `eligible()`. It narrows the recorders to the crate and adds the eighth bucket.
- **β** threads a typed outcome and a per-build tally from `decide_morph_or_remesh` into S1 θ's per-build output accumulator and `BuildResult`.

**Lane B — small primitives (after the S5 move of each file).**
- **γ** `value_size_estimate`, after κ₂ and λ.
- **δ** the dead trajectory helper, after λ.
- **ζ** P2 promotion, after μ and ν.
- **ε** the inverse-dynamics `reused` flag, after λ.

**Lane C — elastic static (after S5 ν, S15's decoders and builder, #8374 and #8248).**
- **η** moves the `sampled_*` helpers to a leaf.
- **θ** partitions the file, mechanically, and gives the adaptive problems a summary accessor.
- **ι** lands `CantileverSolveRequest`.
- **κ** makes the overloads explicit and decodes to a typed request.
- **λ** introduces the route enum, the one planner and the per-route solvers.
- **μ** makes the shell route honest (capability routing).

**Lane D — modal (after S5 μ and S15's modal leaves).**
- **ν** partitions `modal_ops`.
- **ξ** gives supports and damping their seams and removes modal's `reused*` flags.
- **ο** lands the structured box mesh and box-face vocabulary. It joins lanes C and D; #8252's count function adapts to it, whichever lands second (D7).

**ω** closes the PRD.

## 4. Contract (H)

Module paths are the post-S5 ones: the FEA family module ν #9004 creates and the modal family module μ #9003 creates in `reify-analysis`. S5's C7 layout binds: shared modules ← family modules ← `PRODUCTION_TARGETS`, and no family imports another family.

### C1 — Explicit overload entries (κ)

```rust
// reify-analysis, FEA family, elastic module
pub(crate) enum ElasticGeometry { Dims { length: f64, width: f64, height: f64 }, Body { mesh: SolverMesh } }
pub(crate) struct ElasticSolveRequest {
    pub geometry: ElasticGeometry,
    pub material: MaterialModel,            // Isotropic | Anisotropic | Heterogeneous (Field overload)
    pub loads: Vec<LoadSpec>,               // S15 γ decoder
    pub supports: Vec<SupportSpec>,         // S15 γ decoder
    pub options: ElasticOptions,            // S15 β decoder (defaults when the overload has none)
}
fn decode_dims(args: &[Value]) -> Result<ElasticSolveRequest, InputDecodeError>;
fn decode_field(args: &[Value]) -> Result<ElasticSolveRequest, InputDecodeError>;
fn decode_body(args: &[Value], realization: &[RealizationReadHandle]) -> Result<ElasticSolveRequest, BodyDecode>;
```

**Rows.** There are five descriptor rows, each with its own `ComputeFn`. Every other descriptor field is copied from the row being split; nothing is dropped.

| Row | `.ri` overload | `demand` | `persistence` / `significance` | `upstream` | `cancellation` |
|---|---|---|---|---|---|
| `solver::elastic_static` | dims | none | the elastic codec / comparator | `plan_elastic_route` adapter (C2) | as the split row |
| `solver::elastic_static_field` | Field | none | the elastic codec / comparator | none | as the split row |
| `solver::elastic_static_body` | body | `VolumeMeshWithBoundary`, or `VolumeMesh` if meshing ε #8288 has deleted the boundary variant | the elastic codec / comparator | none | as the split row |
| `solver::multi_case` | dims | none (the split row's `VolumeMesh` is inert without a geometry argument; dropping it matches the dims elastic row) | as the split row | none | as the split row |
| `solver::multi_case_body` | body | `VolumeMesh`, as the split row: no boundary half (the pinned `register_compute_fns_marks_multi_case_volume_mesh_demand` test asserts its absence; face-selector BCs on multi-case are out of scope, task 4870) | as the split row | none | as the split row |

- **Overload naming.** Each `.ri` overload names its own row, and all five rows are `StdlibOptimized`.
- **The elastic body demand keeps its boundary half.** The `solve_elastic_static` body overload's face-selector supports resolve through `loads_supports_to_bc_node_sets` on the boundary-attributed mesh. Without it, every body scene with a `target` would silently change.
- **The multi-case body demand does not gain one.** Changing it would re-route multi-case body realization through the attributed gmsh producer, which is a separate decision.
- **Derived tables.** These are set-equal to the derived persistable set: `engine_hash_tests::PERSISTED_TARGET_SOURCES`, `persistable_target_fixtures`, and the significance pin in `buckling_significance.rs`. κ adds a fixture row for each new persistable id.
- **Entry shape.** Each entry is decode → `solve_elastic_request` → `into_failed` at S15's one point.

**Invariants.**
1. **No sniffing.** No production code chooses an overload from `Value` variants.
   - The `body_path` binding and its `multi_case.rs` copy are absent.
   - The Field overload is chosen by its row, never by `classify_material`.
   - An index past an overload's declared arity is unreachable, because each decoder reads its own layout.
2. **One request shape.** `solve_load_cases` decodes the shared inputs once and builds one `ElasticSolveRequest` per case by substituting that case's loads and supports. It never re-spells a `Value` layout and never calls an elastic `ComputeFn`.
3. **The body pre-hydration guard has one home.** `decode_body` returns the guard's `BodyDecode::NotYetRealized`, and the entry maps it to today's empty-diagnostics `Failed` (task 4870 contract). `multi_case`'s body entry uses the same function.
4. **Identity stays sound.** S5 η's parity test covers the new rows: each new string has a `.ri` caller and a row.
   - The dims rows keep their ids, so their persisted keys and `shell-extract::extract`'s `EngineInserted { by: solver::elastic_static }` are unchanged.
   - Body and Field results under the old shared string miss once in the persistent cache. The miss is safe, because a new key is a miss, never a wrong hit.

### C2 — One route decision (λ, μ)

```rust
pub(crate) enum AdaptiveLane { None, Uniform, Localized }
pub(crate) enum ElasticRoute { Shell(ShellPlan), Tet { lane: AdaptiveLane } }
pub(crate) struct RouteDecision { pub route: ElasticRoute, pub diagnostics: Vec<Diagnostic> }   // coded Warnings from capability fallbacks
pub(crate) enum RouteRefusal { /* each variant maps to one DiagnosticCode, Error severity; shell-capability refusals under ShellForce::On */ }
// FeaDimsSupportNotRealized is a request-validation refusal (after decode, before plan_elastic_route, out through the entry's one Failed point), not a RouteRefusal
pub(crate) fn plan_elastic_route(req: &ElasticSolveRequest) -> Result<RouteDecision, RouteRefusal>;
```

**Invariants.**
1. **One planner.**
   - Every elastic entry chooses its route through `plan_elastic_route`: the three `solve_elastic_static` rows and `solve_load_cases` per case.
   - On the `solver::elastic_static` row, the `UpstreamPlanner` (S5 C4) is a thin adapter: `decode_dims` → `plan_elastic_route` → request shell-extract iff the route is `Shell`. So on that row the shell-extract node is inserted iff the trampoline then solves the shell route (BT2).
   - The `multi_case` rows insert no upstream node, as today; the solve never reads the extraction, only the GUI overlay does, which is paired to `solver::elastic_static`.
   - No other code calls `classify_shell` or `extract_shell_route_params`.
2. **Capability routing (§13 R1).**
   - The shell route realizes an isotropic material, the root clamp and the transverse component of the tip force.
   - **Inputs that tet realizes and shell cannot are refusal reasons:**
     - a pressure;
     - gravity;
     - the in-plane component of the tip force (§13 R3);
     - a non-isotropic model;
     - a coordinate kind from #8253.
     - With `ShellForce::Auto` the planner routes to tet and returns one `FeaShellRouteInputFallback` Warning per refused input, naming it.
     - With `ShellForce::On` it refuses with one `FeaShellRouteInputRequiresTet` Error naming the first refused input.
     - Today the non-isotropic `On` case is an uncoded Error, the pressure and gravity cases are silent, and the in-plane component gets an uncoded Warning while the shell solves only the transverse part.
     - Re-routing an in-plane tip load is an intended result change (BT3).
   - **Inputs that no dims route realizes are not refusal reasons**, because re-routing would not honour them: see invariant 3.
   - **The Field row never classifies as shell** (λ's planner). Its material is non-isotropic by type, so it plans `Tet` with no shell diagnostic. Today every thin Field scene emits the non-isotropic fallback Warning; that noise is an intended change (BT3).
   - **One rejection reason, one code per severity.** Every shell-incapability is the same rejection reason: the shell model cannot realize this input. So one Warning code (`Auto`) and one Error code (`On`) cover every input, and the input kind is a structured field and in the message (esc-5791-3: one code per rejection reason).
   - `ShellTooThick` keeps its code and stays a planner decision.
   - **G6: a re-routed thin plate is a stiff P1 tet solve.** The probe in §2 measures a 40× under-estimate on a 1 mm plate, with CG at its cap before #8374.
     - The fallback Warning says the result is a solid-element solve of a thin body.
     - The thin-body advisory becomes route-aware: when the shell route was refused, it names the refused input and never recommends `ShellForce.On`, which would then be an Error. Today the advisory recommends `ShellForce.On`.
     - No signal asserts tet accuracy on such a plate.
3. **One dims-support policy (§13 R1 as applied, R3).**
   - On the dims and Field overloads both routes realize exactly the root clamp (`FixedSupport(target: "root")`). The solve clamps the root face whatever the list says.
   - **Every other declared support is a refusal.** That means a target other than `"root"` (or `"x_min"`, the same face), or a non-Fixed kind at the root. The coordinate kinds that #8253's `SolveBoundary` honours on tet are excluded.
     - The refusal is one coded `FeaDimsSupportNotRealized` Error per such support. It names the support and carries its `.ri` construction-site span label (`first_instance_source_span`, task 4089 / B10), one label per named support.
     - The entry returns `Failed`, so `reify eval` exits 1 (INV-SF-2: no plausible result beside an unmodellable request).
     - It is raised before any route is chosen, so it is route-independent.
     - The remedy is overload-specific. Dims and `solve_load_cases` dims point to the body overload. The Field overload has no supports-honouring overload today, so its message says only the root clamp can be modelled.
   - **This replaces the "supports present, none targets root" arm of `FeaUnderConstrained`.** Today that arm is a Warning that says "insufficient supports … the root face is auto-clamped", without naming the support it ignored.
   - **The empty-list arm of `FeaUnderConstrained` is unchanged**, structured detail included.
   - **The root plus extras case stops being silent.** Today it drops the extras without a diagnostic on the tet route, and, once S15 γ #8683 lands, with γ's Warning on the shell route.
   - **`solve_load_cases`** decodes its shared supports once, so a support shared by N cases is refused once, naming the support, not N times. A per-case support is refused per case and fails the call with the case named.
   - **Tests and examples μ rewrites:**
     - `fea_diagnostics_e2e.rs::under_constrained_present_but_unhonored_support_emits_labeled_warning`, whose fixture `fea_under_constrained_support.ri` uses `FixedSupport(target: "tip")`, now asserts the labelled Error;
     - the in-file `first_instance_source_span_threads_into_fea_diagnostic_label` follows the new code;
     - `examples/multi_load_bracket.ri`'s `FixedSupport("mount_face")` becomes `FixedSupport(target: "root")`, its stated intent (the cantilever mount).
   - S15 γ's shell-drop Warning is redundant once the support is refused before routing, and is deleted with its test.
4. **Lane choice is data.**
   - `AdaptiveLane` is chosen once from the request. A localized-lane runtime failure falls back to `Uniform` on the same geometry: the realized mesh when one is present, never the placeholder box (fixes the dropped-iterate drift). It emits the existing fallback Warning.
   - #8248's per-trigger handling for the body overload is consumed as-is (§13 R2).

### C3 — Per-route solvers and one build site (λ)

```rust
pub(crate) enum ElasticSolution {
    Shell { channels: ShellChannels, mid_field: Value, converged: bool, iterations: usize, max_von_mises: f64 },
    Tet { channels: SampledResultChannels, aposteriori: AposterioriFields, reported: ReportedSolve },
}
fn solve_shell_route(req: &ElasticSolveRequest, plan: &ShellPlan, cx: &SolveContext) -> Result<ElasticSolution, RouteFailure>;
fn solve_tet_route(req: &ElasticSolveRequest, lane: AdaptiveLane, cx: &SolveContext) -> Result<ElasticSolution, RouteFailure>;
```

**Invariants.**
1. **One build site.** `ElasticSolution` → `Value` happens at one site, through S15 ε's `build_elastic_result`. No `type_name: "ElasticResult"` literal remains in the FEA family outside that builder (S15 C3 invariant 1).
2. **One outcome mapping.** `RouteFailure` covers degenerate mesh, cancelled, and BC-resolution error. It maps to `ComputeOutcome` at one site.
3. **Comments become structure.** The trampoline is decode → plan → solve → build. The PERF NOTE and reviewer-addressed blocks move to the doc of the solver they describe, or are deleted where the code now states them (stance: escalating comments signal poor clarity).

### C4 — The cantilever request (ι)

```rust
pub(crate) enum MeshSource { Synthetic { grid_override: Option<(usize, usize, usize)> }, Provided(SolverMesh) }
pub(crate) struct CantileverSolveRequest<'a> {
    pub model: &'a MaterialModel,
    pub dims: (f64, f64, f64),
    pub mesh: MeshSource,
    pub tip_force: [f64; 3],
    pub pressures: &'a [PressureSpec],
    pub body_force: [f64; 3],
    pub bc_override: BcNodeSetOverride,
    pub solve: SolveKnobs,                   // the knobs #8374 / #8244 leave on the solve, grouped
    pub warm_start: Option<CgWarmState>,     // or the adapter's warm-state value, as #8374 lands it
    pub progress: Option<&'a mut dyn FnMut(usize, f64) -> CgIterationControl>,
}
impl<'a> CantileverSolveRequest<'a> { pub fn synthetic(model: &'a MaterialModel, dims: (f64, f64, f64)) -> Self; /* defaults for every other field */ }
pub(crate) fn solve_cantilever_fea(req: CantileverSolveRequest<'_>) -> Result<(CantileverFeaSolve, CgWarmState), DegenerateTet>;
```

- **Start from #8374's signature.** ι starts from the parameter list `solve_cantilever_fea` has once #8374 lands (§13 R2), and groups it verbatim:
  - geometry and mesh source;
  - material;
  - the load and boundary inputs;
  - `SolveKnobs`: exactly the solve parameters `solve_cantilever_fea` takes at ι's dispatch.
    - Today that is `deterministic` and `threads`. Solver-performance C6 keeps them honoured; #8374 removes only `SolverMode`, `resolve_execution_modes` and `PARALLEL_DOF_THRESHOLD`.
    - After #8244 and #8374 it is whatever they put on the signature.
    - A knob that S5 C6 carries as a dispatch-context extension (`SolvePolicy`, read by #8374's adapter) is never duplicated here. One home per fact.
  - warm start;
  - progress.

  `SolveKnobs` defaults to the stdlib-declared `ElasticOptions` defaults (built from S15 β's `stdlib_default_instance`, never a Rust literal), so a test default equals an author's default.
- **Later inputs add a field.** A task that lands after ι adds one field and its default; nothing else changes:
  - `SolveBoundary` (#8253) replaces `tip_force`, `pressures`, `body_force` and `bc_override` with one `boundary` field, its I1/I5 rules unchanged;
  - DWR (#7453) adds `qoi`.
- Every test builds the request from `synthetic(..)` plus named overrides.
- The stale "10 args" comment and the `allow(too_many_arguments)` are deleted.

### C5 — Elastic partition (θ, η)

The elastic module becomes a directory, along the h14 partition as adjusted by S15:

| File | Contents |
|---|---|
| `model.rs` | `MaterialModel`, `PressureSpec`. The lowest file; it breaks the readers ↔ solve cycle. |
| `readers.rs` | What S15's decoders leave. |
| `realized_mesh.rs` | `SolverMesh`, the realized-mesh conversion, BC node sets. |
| `cantilever_solve.rs` | `solve_cantilever_fea` and the synthetic mesher until ο. |
| `adaptive.rs` | Both adaptive problems and `IterateRecord`. |
| `result_channels.rs` | The tet result channels, built on `sampled_channels.rs`. |
| the entry | The trampoline. |

- **Tests:** tests follow their file. Shared fixtures (`make_box_tet_volume_mesh`, the shell9 fixtures, `vm_read_handle`) move to one `#[cfg(test)]` fixtures module, which `multi_case`'s tests use instead of their copy.
- **The adaptive seam (G4: CantileverAdaptiveProblem private fields).**
  - Both adaptive problems expose `into_summary(self, status) -> AdaptiveSummary`. The summary carries `n_dofs`, the grid or element counts, `refine_count`, the global indicator and the refined iterate.
  - The trampoline reads only the summary. The `elements N -> M` Info is formatted from it, and its e2e asserts the summary's counts through a structured field, not by parsing the message (heuristic 12).
- **The sampled seam (G4: `sampled_*` helpers).**
  - `sampled_channels.rs` holds the `sampled_*` helpers, `flatten_nodal_stress`, `rotation_sf_from_curl`, `shear_angles_sf_from_gradient` and their `#[cfg(test)]` assertions. It imports only `reify_ir` and the solver mesh types.
  - It is a leaf module of the FEA family: its only users are `elastic_static`, `buckling` and `shell_solve`, all in the family. If κ₁ left a helper in the shared foundation because a non-FEA file used it at κ₁'s dispatch, that helper stays there and η records why.
- **Rule:** no new module SCC (INV-ARCH-1; the S3 tool reds one). `scripts/module-scc-baseline.json` gains no row.

### C6 — Modal partition and seams (ν, ξ)

`modal_ops` becomes a directory along the measured partition. A partition that S15 emptied is dropped, never kept as an empty file. In particular S15 α/β remove most of `options`, ζ much of `result_value`, and γ `DeclaredSupport`.

| File | Contents |
|---|---|
| `core.rs` | Assembly, eigensolve, the mesh enum. |
| `result_value.rs` | Placeholders only (`placeholder_part` until #7097). The degenerate results are deleted by S15 ζ₀ #9287 (gap-fill 2026-10-16, K5-1), so this file no longer breaks a cycle; if nothing remains it is dropped under the empty-partition rule above. |
| `options.rs` | What remains after S15. |
| `damping.rs` | The damping planners. |
| `supports.rs` | Support realization. |
| `free_vibration.rs`, `mechanism.rs`, `transient.rs` | One target each, plus `displacement_at`. |
| `mod.rs` | The module doc naming the four targets and the entry re-exports. |

The four trampolines stay reachable at their current paths.

```rust
pub(crate) struct BeamDims { pub length: f64, pub width: f64, pub height: f64 }
pub(crate) struct DirichletRealization { pub bcs: Vec<DirichletBc>, pub diagnostics: Vec<Diagnostic> }
pub(crate) fn realize_supports(supports: &[SupportSpec], nodes: &[[f64; 3]], dims: BeamDims) -> DirichletRealization;
pub(crate) fn plan_modal_damping(options: &ModalOptions, material: &MaterialDamping) -> Result<(ModalDampingPlan, Vec<Diagnostic>), ComputeOutcome>;
pub(crate) fn plan_lumped_damping(options: &ModalOptions) -> (RayleighPlan, Vec<Diagnostic>);   // the mechanism path's entry
```

**Invariants.**
1. **Supports seam.** `face_company`, `face_realization`, `face_bound` and the anchor helpers are private to `supports.rs`.
   - Supports tests drive `realize_supports`.
   - `nearest_node`'s non-finite-coordinate tests stay as unit tests inside `supports.rs`; a test inside its own module is not cross-module reach.
   - The `WithAnotherFace` payload is asserted through the realization's coded Info diagnostic, never by message text.
2. **Damping seam.** `classify_damping`, `DampingKind` and `extract_loss_factor` are private to `damping.rs`. The mechanism path calls `plan_lumped_damping`, which owns today's unsupported-descriptor Warning. Damping tests drive the two plans.
3. **No cross-module private test reach.** No test module imports a private item of a sibling module. Each file's tests use `super::` only into their own file.
4. **No module SCC** (INV-ARCH-1).

### C7 — Shared primitives (γ, ζ, ο)

```rust
// reify-solver-elastic, production
pub enum BoxFace { XMin, XMax, YMin, YMax, ZMin, ZMax }
impl BoxFace { pub fn parse(name: &str) -> Option<Self>; pub fn axis(self) -> usize; pub fn is_max(self) -> bool; }
pub struct StructuredBoxMesh { pub nodes: Vec<[f64; 3]>, pub tets: Vec<[usize; 4]>, pub grid: (usize, usize, usize) }
impl StructuredBoxMesh {
    pub fn freudenthal(dims: (f64, f64, f64), grid: (usize, usize, usize)) -> Self;   // node order and table as today
    pub fn nodes_on(&self, face: BoxFace) -> Vec<usize>;
}
// reify-solver-elastic::p2_tet, production
pub fn promote_tets_to_p2(nodes_p1: &[[f64; 3]], tets_p1: &[[usize; 4]]) -> (Vec<[f64; 3]>, Vec<[usize; 10]>);
// reify-ir::warm
pub fn value_size_estimate(v: &Value) -> usize;   // recurses List | StructureInstance | Point | Vector
```

**Invariants.**
1. **Counts are the caller's.** `StructuredBoxMesh` takes counts and has no grid policy. The grid policy is each caller's rule: today `synthetic_grid_counts` and modal's own, and #8252's `synthetic_grid` once it lands, which keeps exactly today's counts per caller, NX_MAX clamp included. So every synthetic mesh is byte-identical before and after (BT9).
   - The modal doc's claim that it "mirrors" the static meshing is replaced by a statement of the two callers' policies.
   - Unifying the policies is not in scope (§6 D7).
2. **The adoption ratchets.** Each row sits in the host that already counts its scope, so each fact has one home:

   | Row | Host | What it counts | Target |
   |---|---|---|---|
   | `freudenthal-table-copy` | S14 α #8614's `crates/reify-solver-elastic/tests/adoption_ratchets.rs` (S14 D20: one ratchet binary per crate; production scope) | the six-tet literal in reify-solver-elastic production | 1 (the structured-box module) |
   | `freudenthal-table-consumer` | S5 α #8984's `tests/infra/test_analysis_stratum_ratchet.py` | the same literal in reify-eval / reify-analysis production | 0 |
   | `box-face-literal` | S5's member | the quoted face literals `"x_min"`…`"z_max"` on code lines (comment lines stripped) in any construct (match arm, `==`, call argument) in reify-eval / reify-analysis production | 0 |
   | `test-support-in-production` | S5's member | `assembly::test_support::` paths in reify-eval / reify-analysis production (anchored, so `reify_test_support::` does not count) | 0 |
   | `value-size-estimate-copy` | S5's member, whole-file scope for this row | `fn value_size_estimate` definitions outside `reify-ir` | 0 |

   - **What S5's member sees.** It cuts each file at its first top-level `#[cfg(test)]`. Two of today's sites sit after such a cut and are invisible: `box_face_plane`, behind the production-half `#[cfg(test)] extract_pressure_loads` that θ deletes, and two of the three `value_size_estimate` copies.
     - The leaf that adds a row adds whatever scan option the row needs to the member, in its own diff: comment-line stripping for `box-face-literal`, whole-file scope for `value-size-estimate-copy`.
     - The member's other rows keep their behaviour.
   - **Test copies of the six-tet table** get a one-time grep check, not a ratchet, because neither host counts test code:
     - the three reify-solver-elastic copies (`boundary/patch_load.rs`, `diagnostics.rs`, `tests/heterogeneous_warmstart_integration.rs`);
     - the one copy θ's shared fixtures module keeps.

     ο converts all four. Test re-accretion is an accepted gap.

3. **One cost model.** All three warm-state targets use the one `value_size_estimate`. Its estimate for a result carrying `Point`/`Vector` values is the recursive one.

### C8 — Per-build morph tally (β)

```rust
// reify-eval, morph_producer.rs (reify-eval cannot name reify-mesh-morph types: that crate depends on reify-eval)
pub enum MorphOutcome {
    Morphed,
    Ineligible(IneligibleReason),            // StructuralChange | BijectionFailure | NamingLayerError
    QualityReject { hard: bool, detail: String },
    SolverError { detail: String },          // the elasticity / projection error text, carried, never parsed
    Panicked { detail: String },
    NotAttempted(NotAttemptedReason),        // no producer | no source mesh | empty boundary
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MorphTally { /* one count per MorphOutcome kind */ }
impl MorphTally { pub fn record(&mut self, o: &MorphOutcome); pub fn summary_line(&self) -> String; }
pub struct BuildResult { /* existing fields */, pub morph_tally: MorphTally }
```

**Invariants.**
1. **Typed outcomes, one enum.**
   - `MorphResult` carries `MorphOutcome`, never a reason string. Diagnostic messages are formatted from the typed value (heuristic 12).
   - reify-mesh-morph depends on reify-eval, so its telemetry counters key on this same enum. Its crate-private `diagnostics::MorphOutcome` is deleted, leaving one outcome vocabulary for the tally and the counters (heuristic 11).
2. **The tally joins S1's per-build output accumulator.**
   - `MorphDispatchIo` is built per realization inside the build loop (three shared borrows), so it cannot hold the tally.
   - S1 θ #9046 already makes the drivers accumulate a per-call `DispatchAttribution` in a local and return it on `BuildResult` / `TessellateResult` (S1 §5.9). β adds `MorphTally` to that same accumulator rather than threading a second one beside it (heuristic 11), whatever struct θ names.
   - `decide_morph_or_remesh` returns its `MorphOutcome` beside the `MorphDecision`, and `execute_realization_ops` passes it up into the accumulator. `MorphDispatchIo` stays immutable: the interaction is stateless (heuristic 7). β ← #9046.
   - `build_snapshot`'s only early `None` precedes every realization, so no tally is lost.
   - `MorphDispatchIo::disabled()` paths (tessellation) record nothing.
   - The tally is not an `Engine` field, so INV-BUILD-1's `reset_per_build_state` and S1's partition are untouched.
   - Every outcome is recorded, including `NotAttempted`.
3. **Consumers read the result.** The CLI's `--verbose` line is `result.morph_tally.summary_line()`. `morph_arm_e2e.rs` asserts on returned tallies and takes no lock.
4. **Telemetry stays a sink.**
   - `compose_morph` still records into `diagnostics::COUNTERS`, now with the eighth bucket (α), and `mesh_morph_stats` reads it.
   - No engine-path or cross-crate test asserts the counters except `mesh_morph_stats`'s own handler tests, which keep `debug_server`'s lock.
   - Those tests seed the counters cross-crate today with `record_morphed()`. α makes the recorders `pub(crate)` and gives them one `testing`-feature seeding entry, `diagnostics::seed_for_test`, beside the existing `reset_for_test`. It takes a `MorphOutcome` after β, and α's own bucket enum before it. The handler tests use it, and its name is disjoint from α's `pub fn record_morphed` check.

## 5. Boundary-test sketch (H)

Every row says which leaf lands it and which side of the seam it pins.

| BT | Seam | Test | Today | Leaf |
|---|---|---|---|---|
| BT1 | overload rows ↔ `.ri` | S5 η's identity parity test passes with the five rows. A body-overload call dispatches `solver::elastic_static_body`. The body-overload compile test pins its own string. | one shared string | κ |
| BT2 | planner ↔ trampoline | Sweep (iso, aniso) × (thin, thick) × (Auto, On, Off) on the `solver::elastic_static` row. Assert shell-extract is inserted iff the trampoline's route is `Shell`. A thin Field-row scene emits no shell diagnostic. | red: an aniso thin scene under Auto inserts shell-extract, then solves tet | λ |
| BT3 | regression floor | Every tracked `examples/*.ri` and `tests/prd-gate/fixtures/*.ri` that reaches an elastic or multi-case target prints identical results before and after each lane-C leaf. The intended changes are named:
- μ's BT4/BT5 diagnostics and routing;
- in-plane tip loads on thin dims scenes re-routed to tet (μ, R3);
- the dropped non-isotropic fallback Warning on Field scenes (λ);
- `examples/multi_load_bracket.ri`'s support edited to `"root"` (μ), with unchanged numbers, because the dims model already clamped the root. | — | η θ ι κ λ μ |
| BT4 | shell capability | `tests/prd-gate/fixtures/shell_route_pressure_thin_plate.ri` and `shell_route_gravity_thin_plate.ri`. Under Auto: tet route, one `FeaShellRouteInputFallback` naming the input, `max_von_mises` differing from the tip-only solve, and a thin-body advisory that does not recommend `ShellForce.On`. With `ShellForce.On` substituted: `reify eval` exits 1 with one `FeaShellRouteInputRequiresTet`. | red (probed): 2.143e9 Pa in both, no diagnostic | μ |
| BT5 | dims-support policy | (a) `tests/prd-gate/fixtures/dims_support_root_plus_xmax.ri` (thin, shell-classified) and `dims_support_root_plus_xmax_thick.ri` (tet): `reify eval` exits 1 with one `FeaDimsSupportNotRealized` Error naming `x_max` and carrying its span label. (b) `dims_support_xmax_only.ri`: the same Error replaces today's `FeaUnderConstrained` "supports present, none root" Warning. (c) An empty list: today's `FeaUnderConstrained` Warning, unchanged. (d) `FixedSupport(target: "x_min")` is accepted as the root clamp. | red (probed): (a) exit 0, 2.143e9 Pa, no diagnostic naming `x_max` (after #8683, γ's Warning on the thin twin); (b) exit 0 with the vague advisory | μ |
| BT6 | cantilever request | No `solve_cantilever_fea(` call passes more than one argument. The 20 former positional-`None` tests build `CantileverSolveRequest::synthetic` with overrides and pass unchanged. | 14 args | ι |
| BT7 | modal seams | The supports tests run through `realize_supports` and the damping tests through the two plans. Cache-hit tests feed a poisoned prior through `solve_modal_analysis_trampoline` / `solve_transient_response_trampoline` and assert that the result comes from the poisoned payload. | private paths and `reused_*` flags | ξ |
| BT8 | morph tally | Two engines in one process each build a morph-eligible edit. Each `BuildResult.morph_tally` counts only its own outcome. `morph_arm_e2e` passes with no `MORPH_DIAG_LOCK` and no `reset_for_test`. | global counters, 3 locks | β |
| BT9 | box mesh | For the fixture dims set (the modal examples' 200×10×2 mm, `over_ceiling_mesh`, the smoke cantilever), the nodes and tets of the modal and elastic synthetic meshes are byte-identical to today's, captured before ο. | — | ο |
| BT10 | cost model | `value_size_estimate` of a `List<Vector3>` counts its components. The three targets' existing warm-cache tests pass. The cost of a `Point`/`Vector`-carrying trajectory or inverse-dynamics warm state rises, which changes its eviction rank. That is γ's one intended change; nothing user-visible depends on the rank. | flat count in 2 of 3 copies | γ |
| BT11 | inverse dynamics | A poisoned prior fed to `solve_inverse_dynamics_trampoline` yields the poisoned result. No `reused` field exists. | flag read | ε |

## 6. Resolved design decisions

- **D1 — Overload entries are rows (fk-3f6ee7ecd393).**
  - The ruling says "explicit overload entry points". The registry keys on the string and has no tag channel, so each overload gets its own target string and descriptor row.
  - After S5 ε a row is one table entry, so the change is five rows, not five string tables.
  - The dims rows keep their ids, so their persisted keys and the shell-extract `EngineInserted` relation are untouched.
  - Rejected: an overload tag decoded from arity inside one entry. Field and a defaulted dims call can share an arity, so a tag would be sniffing moved, not removed.
- **D2 — One planner (S5 ε seam).**
  - S5 ε made the elastic row's upstream planner "today's shell classification", and S5 says S16's route enum absorbs it.
  - `plan_elastic_route` is that planner, and the row's `UpstreamPlanner` is a thin adapter (`decode_dims` → plan → request).
  - The disagreement on non-isotropic materials closes by construction.
- **D3 — Capability routing (§13 R1).**
  - It extends coordinate-target C6's ruled shell policy (Auto → tet + Warning, On → Error) from coordinate kinds to every input that tet realizes and the shell model cannot.
  - **R1 as applied, ratified by Leo (§13 R3).** An input that no dims route realizes (a face support other than the root clamp) is not a reason to leave the shell route, since tet would drop it too. It is a coded Error before routing: answering a different structure is the plausible-but-wrong result INV-SF-2 forbids, and S15's decoder already treats an unsupported load kind the same way (`FeaLoadKindUnsupported`).
  - The in-plane tip component, which tet does realize, joins the refusal reasons.
  - The critic's fold (F4) found that the "supports present, none root" arm of `FeaUnderConstrained` already fires today, as a Warning that names no support. The Error replaces that arm, and the empty-list arm stays.
  - It adds no boundary-condition semantics to either dims route. Wiring face supports on the dims routes is out of scope (§11).
  - Codes: one input-neutral pair, `FeaShellRouteInputFallback` (Warning, `Auto`) and `FeaShellRouteInputRequiresTet` (Error, `On`), mirroring coordinate-target's pair shape. The dims-support policy adds `FeaDimsSupportNotRealized` (Error, §13 R3).
  - If coordinate-target α #8251 has minted `FeaPointTargetShellFallback` / `FeaPointTargetRequiresTet` first, μ folds them into the neutral pair, naming the input kind in the message and in a structured field. If μ lands first, #8251 uses μ's pair (adapt-second note at decompose).
- **D4 — `CantileverSolveRequest` grows by fields.**
  - Its fields are today's, minus what #8374 removes.
  - `SolveBoundary` (#8253) and `qoi` (#7453) each become one field when they land, whichever of the two tasks and ι lands second (§13 R2).
- **D5 — Partitions are mechanical, then restructured.**
  - θ and ν move code without changing behaviour.
  - The behavioural restructure (κ, λ, μ, ξ) happens in the smaller files. That keeps each diff reviewable and each lock narrow.
- **D6 — Poisoned-prior probes replace `reused*` flags (fk-43921c86c7d3).**
  - No public observable distinguishes a hit from a miss, and adding one for tests would be the same smell.
  - A poisoned prior asserts behaviour through the public `ComputeFn`: a hit uses the prior's payload.
  - The modal half lands with ξ, which rewrites those tests anyway. The dynamics half lands after S5 λ.
- **D7 — The box mesh shares the generator, not the policy (fk-603bbbab0515).**
  - #8252 (coordinate-target C8 and BT10) rules that each caller keeps exactly today's counts. So the modal–static `nx` difference stays, as an explicit per-caller policy.
  - Moving modal onto `NX_MAX` would change seven modal fixtures at `L/h = 100` with unmeasured accuracy (2 % P2 bands). That is not a refactor.
  - The generator goes to reify-solver-elastic so the solver's own test copies adopt it too.
  - **The generator takes counts, so it does not wait for #8252.** ο's callers pass whatever count rule exists when ο lands: `synthetic_grid_counts` and modal's own rule, or #8252's `synthetic_grid`. Whichever of ο and #8252 lands second adapts. BT9 holds either way.
- **D8 — P2 promotion moves now (fk-cecdff4f1373).**
  - `promote_tets_to_p2` moves to `reify_solver_elastic::p2_tet`, beside the P2 element's `EDGES`, and `test_support` keeps fixtures only.
  - #8423 is subsumed. Its ElementOrder half is delivered by S15 β #8679's options decoders. Its P2 half assumed a `P2TetMesh::promote` that is not on main.
  - #7075 builds `P2TetMesh` on the production function (adapt-second note).
- **D9 — `value_size_estimate` lives in `reify_ir::warm` (fk-12285c864993).**
  - It sits beside `OpaqueState::estimated_size_bytes`, in the lowest crate every caller reaches. reify-analysis may not import reify-eval's `warm_pool`.
- **D10 — `morph_stats` is deleted (fk-acc10f2161f6).**
  - #7835's preferred resolution, confirmed: no frontend reads it, and it has served zeros since it shipped.
  - `mesh_morph_stats` remains the telemetry RPC.
  - **Who edits the tool tables.**
    - If S25 τ #8955 has landed, α removes `morph_stats` from `gui/src-tauri/debug-tools.json` and regenerates the derived tables.
    - Otherwise α edits the hand lists in the same diff, because they go red otherwise: S25's `docs/debug-mcp-contract.md` row, S26's `KNOWN_DEBUG_TOOL_NAMES`, `debugParity`'s `PURE_ENGINE_SIDE`, and `sidecarPromptParity`'s `NOT_ADVERTISED_TO_SIDECAR`. α also removes the `docs/gui-event-channels.md` row.
    - The owners get cross-reference lines. The as-authored D3 table in `docs/prds/debug-server-gating-boundary.md` is a dated record and is not edited.
- **D11 — Delete `worst_case_residual_fraction` (fk-baa063db2052).**
  - Ruled deletable.
  - `ImpulseTrain::residual_vibration` becomes test-only, as the shaper tests' oracle, rather than a production orphan.
  - Every false wiring claim is corrected in the same diff.

## 7. Disposition table

| Key | Severity | Leaf | Note |
|---|---|---|---|
| fk-3f6ee7ecd393 | high | λ (closes); κ, θ, η (part) | |
| fk-9facd9401295 | medium (weakened) | ι | the stale dead-code allows are θ's |
| fk-603bbbab0515 | medium | ο | |
| fk-cecdff4f1373 | medium | ζ | subsumes #8423 |
| fk-7687550ec80f | medium | ξ (closes); ν (part) | subsumes #7760 |
| fk-43921c86c7d3 | low | ε (closes); ξ (modal half) | the module split is S5 λ #9002 |
| fk-baa063db2052 | medium | δ | |
| fk-12285c864993 | low | γ | |
| fk-d49d3ac15f14 | low | — | already #8546; referenced, not re-filed |
| fk-acc10f2161f6 | medium | α | subsumes #7835 |
| fk-d632489a4c98 | medium (weakened) | — | already #8527; α depends on it |
| fk-e3acedd1c27a | low | β | |

## 8. Pre-conditions (G3, verified on main `1b06f93151`)

- **Substrate present today:**
  - `reify_ir::warm` (`OpaqueState`);
  - `reify_solver_elastic::p2_tet` (P2 element kernel and `EDGES`);
  - `shell_boundary::build_support_bcs` and `SupportKind::{Fixed, Pinned}`;
  - `classify_shell`, `ShellForce { Off, Auto, On }`;
  - `MorphDispatchIo`, `decide_morph_or_remesh`, `BuildResult`;
  - `diagnostics::COUNTERS` and its seven buckets;
  - `register_morph_producer`.
- **Produced upstream, wired as hard edges:**

  | Producer | What S16 needs from it |
  |---|---|
  | S15 β #8679 | `ElasticOptions`/`ModalOptions` decoders |
  | S15 γ #8683 | `LoadSpec`, `SupportSpec` |
  | S15 ε #8690 | `build_elastic_result` |
  | S15 ζ #8693, ι #8702 | modal builders and codes |
  | S5 α #8984 | the ratchet member |
  | S5 κ₂ #9001 | moves the FDM family (`fdm_slice.rs`'s `value_size_estimate` copy) |
  | S5 λ #9002 | moves dynamics/trajectory |
  | S5 μ #9003 | moves modal |
  | S5 ν #9004 | moves the FEA core |
  | S14 α #8614 | the solver-elastic adoption-ratchet binary |
  | #8374, #8248 | §13 R2 |
  | #8527 | deletes `morph()` |
  | S1 θ #9046 | the per-build output accumulator (`DispatchAttribution`) |

- **Reached transitively** through ν and μ: S5 ε #8994 (descriptor rows and the `UpstreamPlanner`) and S5 η #8997 (identity parity). S14 δ #8619 (`AssemblyElement::new` only) is ordered before S5 μ/ν, and so before S16.
- **No grammar change.** The `@optimized` strings are annotation arguments, and new strings parse as the old ones do (G3 vector 1 not needed). No `.ri` fixture is novel syntax.

## 9. Cross-PRD relationship (G4)

| Artefact / PRD | Direction | Mechanism | Owner |
|---|---|---|---|
| `compute_targets/mod.rs` `sampled_*_field` helpers (program G4) | this PRD **owns** | η: leaf `sampled_channels.rs` in the FEA family | **S16** |
| `CantileverAdaptiveProblem` private fields read by the trampoline (program G4) | this PRD **owns** | θ: `AdaptiveSummary` | **S16** |
| reify-mesh-morph eligibility seam, `compose_morph` single public entry (program G4) | this PRD **owns** | α (after #8527) | **S16** |
| S15 decoders, `build_elastic_result`, modal builders | **consumes** | κ, λ, ν, ξ | S15 |
| S15 γ shell-route support Warning | **supersedes** | μ refuses an unrealizable support before routing (`FeaDimsSupportNotRealized`, Error), which makes γ's Warning redundant; μ deletes it and its test | S16 (ruling OQ4) |
| S5 descriptor rows, `UpstreamPlanner`, family modules, ratchet member, C7 move protocol | **consumes** | κ adds rows; λ supplies the elastic planner; C7 steps 3–4 bind θ, ν, η | S5 |
| S14 δ #8619 `AssemblyElement::new` | **ordered after** (via S5 μ/ν) | — | S14 |
| S14 α #8614 `crates/reify-solver-elastic/tests/adoption_ratchets.rs` (S14 D20) | **consumes** | ο adds the `freudenthal-table-copy` row | S14 |
| `docs/debug-mcp-contract.md` (S25), `KNOWN_DEBUG_TOOL_NAMES` (S26), sidecar and debug parity lists | consumer edit, same diff | α removes `morph_stats` | S25, S26 |
| S1 θ #9046 per-call outputs (`DispatchAttribution` accumulated in a driver local, returned on `BuildResult` / `TessellateResult`) | **ordered after**, β ← #9046 | β adds `MorphTally` to θ's accumulator and `BuildResult.morph_tally`; no Engine field | S1 (accumulator), S16 (tally) |
| coordinate-target #8251 / #8252 / #8253 / #8256 / #8257 | adapt second (D7 for #8252) | `SolveBoundary` → one `CantileverSolveRequest` field; `synthetic_grid` → the counts ο's callers pass; μ's code pair absorbs #8251's | coordinate-target (mechanism), S16 (structure) |
| solver-performance #8374 | **before** θ | the adapter owns the solve and `SolverMode` / `resolve_execution_modes` / `PARALLEL_DOF_THRESHOLD` are gone; `deterministic` and `threads` stay honoured and ride `SolveKnobs` | that PRD |
| #8248 | **before** θ | per-trigger lane outcomes | #8248 |
| meshing #8288 | adapt second, with a decompose note | if #8288 lands after β, its new failed-morph path produces a `MorphOutcome` that reaches the tally, not only the counters; it also edits `morph_arm_e2e.rs` | meshing-service |
| DWR #7453, meshing #8289/#8290, PDROP #7080/#7084, flexible-modal #7142/#8247/#7166/#7167, #6882, #8255, #7189, #8078, #8367, #8375, #7887, #7869 | adapt second | land into the partitioned files; delivered-check paths repointed per C7 step 4 | each task |
| #7835, #7760, #8427, #8423 | **subsumed** | cancelled at decompose with a reason naming the leaf and `x_subsumed_by`; the leaf carries `metadata.subsumes` | S16 |
| #8527 (`morph()`), #8546 (`tots.rs`) | **referenced** | α ← #8527 | those tasks |
| #7075 static P2 | **consumes** ζ | `P2TetMesh` built on `p2_tet::promote_tets_to_p2` | #7075 |
| S24 PLINECITE `--rename`, invariants Pointer column | **same-diff obligation** | every leaf that moves a file runs the rename once #8822 has landed | S24 |

No new contested-ownership pair is introduced (`phase-3-breadcrumb-map.md` §3).

## 10. Decomposition plan

**Move protocol.** Every leaf that moves, renames or deletes a file or symbol applies S5's C7 steps 3–4 in its own diff:
- path-keyed registries (ptodo/pdiag baselines, G-allow audit tests, ratchet scan roots, `module-scc-baseline.json`, engine-hash coverage, `check-nan-safe-ordering.sh` scope, the registration lint's definition-file list, nextest partitions);
- `plinecite-baseline-gen --rename` once S24 ο #8822 has landed;
- a `reify-audit --pattern PDCHECK` repoint of every live task's delivered check it breaks: three writes each, `.yaml`, `.md` twin and the task's complete `metadata.delivered_checks`.
- a hand repoint of **done** producers' checks, which PDCHECK does not scan.

Each path below dies first at S5 μ/ν, whose own C7 step 4 repoints it into `reify-analysis`. S16's θ and ν then turn the repointed single file into a directory and repoint it again. The breakers known at author time (fold-verifier V7; re-run PDCHECK at dispatch):
- **θ** (the elastic file): #6879, #5802, #5797 (deferred), #5313, #7453, #8289, #6731. The `absent` checks of #8679, #8683 and #7080 go vacuous and are repointed, not dropped.
- **ν** (the modal file): #7166, #7167, #7097, #8252, #8255, #8256, #6882, #8375, #8837, #7084. The `absent` checks of #8674, #8679, #8683 and #8693 are repointed likewise.
- **Done producers, by hand:** #7261 (`ShiftSkippedModes`), and #6878 (`"MaterialDamping"\s*=>`, which ξ also moves into `damping.rs`).

**New gate-resident tests.** None. Every leaf adds tests to existing binaries and rows to S5's existing ratchet member. A leaf that nonetheless adds an integration-test file carries its nextest partition entry in the same diff. No member asserts a wall-clock bound.

**Trigger-chain gate.** κ and λ are high (fk-3f6ee7ecd393). Decompose adds both as dependencies of #8612.

| Label | Task | Modules | Observable signal | Prereqs | Keys · INV |
|---|---|---|---|---|---|
| α #9085 | Retire `stats.rs` (re-exports, recorders, tests, `stats_g_allow.rs`, the `testing`-feature comment naming it), the `morph_stats` RPC (ToolDef, dispatch, its 4 tests, `types.ts` `MorphStats` and its typecheck fixture, the event-channel contract fixtures, `docs/gui-event-channels/morph-stats.md` and its `gui-event-channels.md` row) and `eligible()` with its audit pin; the tool tables per D10 (regenerate from `debug-tools.json` if S25 τ #8955 has landed, else edit the hand lists). `record_*` become `pub(crate)`. A `testing`-feature `diagnostics::seed_for_test` replaces the gui handler tests' cross-crate `record_morphed()` calls (C8 inv 4). The crate-root `snapshot` re-export goes. Eighth bucket `remeshed_solver_error` for the `Unsupported` arm and non-panic `SolverError` (`format_summary`, the `mesh_morph_stats` payload and its gui type). `metadata.subsumes: [7835]`; #6407 (already retired by the backlog sweep) is left as is | reify-mesh-morph, gui/src-tauri, gui/src, docs | `mesh_morph_stats` reports a non-zero `remeshed_solver_error` after an injected solver failure; `morph_stats` is absent from the debug tool list; `reify_mesh_morph::stats` is absent | **#8527** | fk-acc10f2161f6 · INV-SF-5 |
| β #9092 | Per-build morph tally (C8): one `MorphOutcome` in reify-eval (with detail strings carried) replaces `MorphResult`'s reason strings and mesh-morph's private enum; `decide_morph_or_remesh` returns it; the tally joins S1 θ's per-build output accumulator and lands as `BuildResult.morph_tally`; `seed_for_test` takes a `MorphOutcome`; CLI `--verbose` prints it; `morph_arm_e2e` asserts returned tallies, with no lock and no `testing` dev-dep in reify-eval | reify-eval, reify-mesh-morph, reify-cli | BT8; `reify build --verbose` on a morph-eligible edit prints the per-build morph line from the result | α, **#9046** | fk-e3acedd1c27a · h7, h11, h12 |
| γ #9086 | One `value_size_estimate` in `reify_ir::warm` (C7), recursing into `Point`/`Vector`; the three private copies deleted in their reify-analysis homes; ratchet row `value-size-estimate-copy` (whole-file scope, added to the member here). Its one intended change is the eviction cost of `Point`/`Vector` warm states (BT10); otherwise the regression floor | reify-ir, reify-analysis (the three callers), tests/infra | BT10; the inverse-dynamics, trajectory and FDM-slice warm-cache e2e tests pass unchanged; row = 0 | **#8984**, **#9001**, **#9002** | fk-12285c864993 · INV cand 5 |
| δ #9087 | Delete `worst_case_residual_fraction`, its tests and its audit pin (`trajectory_ops_producer_is_g_allow_marked`). Correct every false wiring claim: module doc, the registration comment, `reify-stdlib` `lib.rs` and `trajectory/mod.rs` docs, `input_shape.rs`, the two robustness examples' comments, the 2026-05-30 audit module doc. `ImpulseTrain::residual_vibration` becomes `#[cfg(test)]`; the `pub use` of `build_train_for_shaper` and `shaper_damping_ratio` loses both names that no longer have a cross-crate caller. Move the test module to the end of the file; the module doc describes the two trampolines | reify-analysis (trajectory family), reify-stdlib, reify-audit tests, examples | `worst_case_residual_fraction` absent repo-wide; the trajectory e2e tests and `examples/trajectory/*` print unchanged | **#9002** | fk-baa063db2052 · INV-SF-5 |
| ε #9088 | Delete `InverseDynamicsRun.reused`; its cache-hit test feeds a poisoned prior through `solve_inverse_dynamics_trampoline` (D6). A behaviour-preserving refactor leaf: its signal is the regression floor | reify-analysis (dynamics family) | BT11; the dynamics e2e tests pass unchanged; `reused` absent from the dynamics family | **#9002** | fk-43921c86c7d3 · tests stance |
| ζ #9089 | P2 promotion production home (D8): `promote_tets_to_p2` → `reify_solver_elastic::p2_tet`; the modal and buckling callers and the solver tests import it there; `test_support` doc stays "fixtures only"; ratchet row `test-support-in-production`. `metadata.subsumes: [8423]` | reify-solver-elastic, reify-eval/reify-analysis callers, tests/infra | row = 0; modal and buckling P2 e2e unchanged; `test_support.rs` defines no `promote_tets_to_p2` | **#8984**, **#9003**, **#9004** | fk-cecdff4f1373 · INV cand 5 |
| η #9090 | `sampled_channels.rs` leaf in the FEA family (C5): the `sampled_*` helpers, `flatten_nodal_stress`, the curl/shear helpers and their test assertions; `SampledResultChannels` and `error_indicator_field` built on it | reify-analysis (FEA family) | BT3 on the FEA examples; the file imports nothing from a sibling FEA module | **#9004** | fk-3f6ee7ecd393 (part) · INV-ARCH-1 |
| θ #9093 | Elastic partition (C5): directory module along the adjusted h14 partition; `model.rs` breaks the cycle; `AdaptiveSummary` (G4 seam); tests per file, shared fixtures module adopted by `multi_case` tests (their six-tet copy and `make_box_tet_volume_mesh` copy deleted); stale allows removed (`CantileverFeaSolve`, `realized_solver_mesh`, `#[cfg(test)] extract_pressure_loads`, `solve_shell_static`). `metadata.subsumes: [8427]` | reify-analysis (FEA family) | BT3; the trampoline reads no `problem.grid` / `problem.iterates`; no module SCC; no file over 2,000 production lines | η, **#8374**, **#8248** | fk-3f6ee7ecd393 (part) · INV-ARCH-1, h13, h14 |
| ι #9094 | `CantileverSolveRequest` (C4); 23 call sites migrated; tests build from `synthetic(..)` | reify-analysis (FEA family) | BT6; BT3 | θ | fk-9facd9401295 · h4, h9 |
| κ #9102 | Explicit overload entries (C1): five rows with every descriptor field per the C1 table (the body rows keep the boundary demand), five `@optimized` strings, three decoders; `multi_case` builds per-case typed requests; the body guard in `decode_body`; `PERSISTED_TARGET_SOURCES`, `persistable_target_fixtures` and the significance pins extended; the stdlib overload header comments and the compile-test pins updated | reify-analysis, reify-compiler stdlib, compiler stdlib compile tests, reify-eval tests | BT1; BT3; `body_path` absent | θ, ι, **#8679**, **#8683** | fk-3f6ee7ecd393 (part, **high**) · h12 |
| λ #9118 | Route enum, one planner, per-route solvers, one build site (C2 items 1 and 4, C3): the dims row's `UpstreamPlanner` calls `plan_elastic_route`; PERF NOTE prose rehomed or deleted; the `elements N -> M` e2e reads the summary | reify-analysis, reify-eval (upstream routine call only if S5 left one) | BT2; BT3; one `"ElasticResult"` literal in the FEA family (the builder) | κ, ι, **#8690** | fk-3f6ee7ecd393 (**high**, closes) · INV-PD-2, h2, h3 |
| μ #9119 | Capability routing (C2 items 2 and 3, D3, §13 R1, R3): shell capabilities; `FeaShellRouteInputFallback` / `FeaShellRouteInputRequiresTet` (absorbing #8251's pair if landed); `FeaDimsSupportNotRealized` Error (span-labelled, refused before routing, overload-specific remedy) replacing `FeaUnderConstrained`'s "supports present, none root" arm (§13 R3); the in-plane tip component re-routed as a refusal reason; the rewrites named in C2 inv 3 (the two labelled-advisory tests, `examples/multi_load_bracket.ri`); the thin-body advisory route-aware (never recommends `ShellForce.On` after a refusal); S15 γ's shell-drop Warning and its test deleted; a test drives the five committed evidence fixtures. Docs-truth: the FEA chunk's routing and support paragraphs (extends or creates `crates/reify-mcp/src/tools/chunks/fea.md`, as solver-performance λ #8384 does); intent-level findability ("why did my thin plate solve as solid", "my support is ignored") | reify-analysis, reify-core (codes), reify-mcp chunks, tests/prd-gate/fixtures | BT4, BT5 via `reify eval` on committed `tests/prd-gate/fixtures/` scenes; each chunk signature compiles in a smoke `.ri` | λ, **#8683** | — · INV-SF-5, INV-SF-6 |
| ν #9091 | Modal partition (C6): directory module; per-file module docs; tests per file; four trampoline paths unchanged. `metadata.subsumes: [7760]` | reify-analysis (modal family) | BT3 on the modal examples and `modal_analysis_e2e`; no module SCC | **#9003**, **#8693**, **#8702** | fk-7687550ec80f (part) · INV-ARCH-1, h13, h14 |
| ξ #9120 | Modal seams (C6 invariants 1–3): `realize_supports`, `plan_modal_damping`, `plan_lumped_damping`; supports and damping tests onto them; `reused_assembly` / `reused_setup` deleted with poisoned-prior tests (D6) | reify-analysis (modal family) | BT7; no test imports a sibling module's private item; `reused_` absent | ν | fk-7687550ec80f (closes), fk-43921c86c7d3 (modal half) · tests stance |
| ο #9121 | Structured box mesh and `BoxFace` (C7) in reify-solver-elastic; modal `build_beam_mesh`, the elastic synthetic arm (which the uniform adaptive lane also reaches), the three solver-elastic test copies and θ's shared fixtures copy adopt it; `face_bound` / `box_face_plane` use `BoxFace`; ratchet rows `freudenthal-table-copy` (S14 host), `freudenthal-table-consumer` and `box-face-literal` (S5 host). Counts stay per caller (D7) | reify-solver-elastic, reify-analysis, tests/infra | BT9; rows at 1, 0 and 0; the test copies absent | θ, ν, ξ, **#8614**, **#8984** | fk-603bbbab0515 · INV cand 5, h11 |
| ω #9122 | PRD close: the terminal stamp | this PRD and manifest | committed `SHIPPED` header with landed leaf ids, the AS-AUTHORED freeze and the LIVE/AS-AUTHORED map | all above | — |

**Docs-truth gate.**
- μ is the only language-surface leaf: routing and support diagnostics that a `.ri` author sees. It carries item 1 (the chunk) and item 4 (findability) in its own diff.
- Items 2 and 3 are not applicable: no authoring idiom changes, and after μ's edit of `multi_load_bracket.ri`, every elastic dims example writes `FixedSupport(target: "root")`.
- κ changes `@optimized` strings that no author writes.

**D3 disposition (author time, 2026-10-14/15).**
- μ is the only leaf with a CLI-observable premise. The lead probed it directly with `reify eval` (`target/debug/reify`, built 2026-10-12; the elastic files are unchanged since `a030ee7528`).
  - The five evidence fixtures exit 0 at main, none of them naming its extra input.
  - With `ShellForce.Off`, the tet route realizes gravity and pressure (§2).
  - These are the red premises of BT4 and BT5.
- The other leaves are refactors whose signals are e2e regression floors, kernel tests, GUI/debug surfaces and docs. These are out of the D3 harness's probe reach (memory `procedural_prd_d3_workflow_reach_limits`), so the workflow is not run for them.
- If decompose re-runs D3, it passes μ alone.

**G7 walk (advisory at author time).**
- **INV-SF-5 / nothing-vacuous-and-unowned:** the shell drop, the dims-support drop, the pressure, gravity and in-plane drop on shell, and `stats.rs`'s zeros are all removed or made loud.
- **INV-SF-2:** an unrealizable declared support is an Error with `Failed`, so `reify eval` exits 1. No plausible result is returned beside it (R3).
- **INV-SF-6:** μ's code pair, the dims-support Error and the eighth bucket's summary are coded.
- **INV-PD-2:** λ writes `ElasticResult` only through S15's builder.
- **INV-ARCH-1:** θ and ν introduce no SCC.
- **INV-BUILD-1:** untouched; β adds no Engine field.

No waiver is needed.

## 11. Out of scope

- **Wiring face supports into either dims route's boundary conditions.** §13 R1 rejected it. Supports are honoured on the body overload today. The coordinate kinds are coordinate-target's.
- **Unifying the modal and static grid policies** (D7). That needs an accuracy measurement and a ruling.
- **The `tots.rs` helpers** (#8546) and **`morph()`** (#8527): filed separately.
- **The decoders and builders themselves** (S15), **the crate move and descriptor registry** (S5), **the Engine partition** (S1).
- **The debug tool table export** (S25 τ #8955) and **the GUI wire-type generation** (S26).
- **The real mid-surface shell solve.** The flat-plate driver's synthetic mesh stays; `solve_flat_plate_shell`'s own OQ-2 owns it.

## 12. Open questions (tactical)

1. **Folding #8251's pair.** If #8251 has landed its two codes, μ either renames them to the neutral pair or keeps them as deprecated aliases for one release. *Suggested:* rename; neither code has shipped in a release. Decide in μ.
2. **Root-face spelling.** Decided: `"root"` and `"x_min"` both name the dims model's root face (BT5d). Once ο's `BoxFace` exists, `"root"` parses as `BoxFace::XMin` for every box-mesh consumer, modal included (amended 2026-10-16, gap-fill K5-3: modal's silent skip turned `FixedSupport(target: "root")` into a free-free solve; was "at the elastic dims entry only").
3. **`ElasticSolution::Shell`'s exact fields.** They follow S15 ε's `ElasticResultParts`. Decide in λ.
4. **Where `MorphTally::summary_line` formats.** In reify-eval beside the tally, or in the CLI. *Suggested:* beside the tally, so the GUI could reuse it. Decide in β.
5. **The modal damping test at `mechanism_modal_warns_on_unsupported_damping_descriptor`** pins `classify_damping` as "the single extension point". Under C6 the extension point is the two plans. Its Case C blocks are rewritten to them. Decide the exact assertions in ξ.

## 13. Rulings (Leo, 2026-10-14/15, this session; program §Resolved decisions binds everything else)

1. **R1 — Shell-route user supports (program OQ4): capability routing** (2026-10-14).
   - Each route declares what it realizes. The one planner sends a scene with any input the shell model cannot realize to tet under `ShellForce::Auto`, with a coded Warning, or refuses it under `On`, with a coded Error.
   - No new boundary-condition semantics on either dims route.
   - Rejected: wiring box-face supports on both dims routes (results change for every non-root scene; it needs a `SolveBoundary` variant coordinate-target owns), and wiring the shell plate only (route choice would change physics).
   - *Refined by R3:* as first ruled, the re-route list was "a support other than the root clamp, a pressure, gravity, or a non-isotropic material", with a coded Warning for a face support no dims route realizes. R3 refuses such supports before routing (an Error), and adds the in-plane tip component to the re-route list. The final re-route list is a pressure, gravity, the in-plane tip component, a non-isotropic model, or a coordinate kind.
2. **R2 — Ordering against out-of-program editors: near-term reshapers.**
   - Beyond the program upstreams (S15 → S5 → S16), the elastic lane waits for #8374 (solver-performance β: the adapter takes the solve, and the execution-mode knobs leave `solve_cantilever_fea`) and #8248 (body overload never enters the uniform lane).
   - Every other pending editor of the two files adapts second, and gets a decompose note naming the leaf it rebases onto. That covers coordinate-target (#8253, #8256, #8257, …), DWR #7453, meshing, PDROP, flexible-modal and the rest.
3. **R3 — Unrealizable supports are an Error; in-plane tip loads re-route (Leo, 2026-10-15, ratifying the R1 refinement).**
   - A declared support the dims model cannot realize (a target other than `"root"`/`"x_min"`, or a non-Fixed kind at the root) is a coded `FeaDimsSupportNotRealized` Error naming it with its span label. The model "is going to produce a wildly wrong answer" otherwise.
   - The empty-list advisory stays a Warning.
   - The in-plane tip-force component joins the shell refusal reasons (Auto: tet with a Warning; On: Error), because tet realizes it.
   - Rejected: a Warning with the route unchanged (the v2 draft), and re-routing non-root supports to tet (tet ignores them too).

## Gap-fill amendments (2026-10-16)

Rulings from the attended gap-fill sitting, `plans/gap-fill-deliberation-reify-20261016.md`. Task records carry the same notes.

- **K5-1** (`§K5-1`): S15 ζ₀ #9287 deletes `degenerate_modal_result` / `degenerate_displacement_history`, so the §2 "cycle hazard" bullet and C6's `result_value.rs` cycle rationale are dropped (amended in place above). ν #9091 builds `result_value.rs` only if something remains (`placeholder_part` until #7097).
- **K5-3** (`§K5-3`, uncontested block): modal support realization is total. ξ #9120's `realize_supports` emits one coded refusal Error per support that names no box face or uses a target form modal cannot realize (selector, `PointSupport.at`) until its owner wires it, and the entry returns `Failed` (INV-SF-2); the pinning test `build_dirichlet_bcs_ignores_supports_that_name_no_face` is rewritten to assert the code. ο #9121's `BoxFace::parse("root")` → `XMin` for every box-mesh consumer; §12 OQ2 amended in place above. Code naming (a modal refusal code, or μ #9119 minting a general one) is decided at ξ.
- **K5-4** (`§K5-4`, uncontested block): ModalResult echo fields come from what was applied. `DirichletRealization` gains `applied: Vec<SupportSpec>`, and `plan_modal_damping` / `plan_lumped_damping` return the applied descriptor (or none); the free_vibration and mechanism builders take those, not options. New ξ #9120 capability `boundary-conditions-echo-from-realization` (grep `field_or\(options, "boundary_conditions"` absent in `crates`; three writes). S15 ζ #8693 carries a doc note on the hand-off.
- **K5-2** (`§K5-2`, notes only): the new stream `support-realization-policy` (its PRD is authored separately from `~/.claude/spawn-briefs/`) follows S15 γ #8683 and ο #9121 and absorbs ξ #9120's `realize_supports` (it becomes a call), turns μ #9119's dims refusal into a capability row (R1's "root clamp only" capability and R3's `FeaDimsSupportNotRealized` behaviour kept), makes #7081 a call rather than a port, and gives #8256 a consumer seam. K5-3's refusal and K5-4's applied echo fold into its refusal and realization types when it lands. No S16 leaf is re-scoped by K5-2.


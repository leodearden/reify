# PRD — Solver crates: layering, shared element primitives, one predicate home

> **Status:** decomposed 2026-10-12 — α #8614, β #8616, γ #8618, δ #8619, ε #8621, ζ #8626, η #8628, θ #8629, ι #8630, κ #8632 (cancelled 2026-10-14, subsumed by S10 β₁ #8935; R2), λ #8637, μ #8640, ν #8645, ξ #8653, ο #8656, π #8658, ρ #8660 (PRD close); adopted #8528; gap-fill 2026-10-16: ξ₂ #9267, ξ₃ #9268, η₂ #9271, η₃ #9272, φ-ff #9273, χ-ff #9274, ψ-ff #9275, adopted #7801. Retro-critic sitting 2026-10-19, ruling R2: see the closing amendments section. Authored 2026-10-12 (pass 1). Stream **S14** of the review-all
> program `plans/review-all-program-reify-20261012.md` (run `review-all-reify-20261010`), ruled in
> that doc's "Uncontested block" decision. Authored via `/prd` by a wave-1 seat; the lead commits it.
>
> **Milestone:** v0.6. **Approach:** B + H. FEA is a high-stakes seam per the overlay, and the
> stream spans reify-solver-elastic, reify-constraints, reify-eval, reify-ir, reify-kernel-gmsh and
> reify-mesh-morph. The contract is in §5 and the boundary tests in §6.
>
> **Code anchors** verified against main `8e6ffcac9d` (2026-10-12). The finding anchors were
> verified at `a030ee7528` (2026-10-10). Main moves fast: cite by symbol and re-locate lines at
> implementation time.

## 1. Goal

Every behaviour that the solver crates ship today keeps shipping unchanged: the same CLI output,
the same `E_*` diagnostics and the same golden values. What changes is where facts live. Each of
the following gets exactly one home that the next change edits once:

- the force-density input contract and the cable/strut sign rule;
- the element-level shell primitives (frame, plane-stress law, shear correction, bubble
  condensation);
- the per-element DOF-layout and MPC-row invariants;
- the bar/active-set load kernel;
- the integration-test FEA harness;
- the gmsh-unavailable fact;
- the spatial index's build/query contract;
- the constraint solver's slack, Money, centrality, floor and signed-contribution predicates.

`reify-constraints/src/solver.rs` (11,788 lines) becomes the layered module tree that was measured
for it.

## 2. Consumers and user-observable surface (G1, G2)

This is a behaviour-preserving restructure. Its user-observable signal is that the existing
end-to-end surfaces stay green with **no expectation edits**, plus new rejection tests where an
invariant moves from "unchecked" to "checked". Named consumers per mechanism:

| mechanism | consumer (engine-integration-norm seam) | user-observable surface that must stay unchanged |
|---|---|---|
| `ForceDensityProblem` + `MemberKind::admits` | reify-eval `compute_targets/form_find.rs` trampolines (§3.4 ComputeNode dispatch) | `crates/reify-cli/tests/harness_cli/cli_tensegrity_e2e.rs`; `crates/reify-eval/tests/golden/tensegrity_t_prism.txt`; the `harness_fea_solver_e2e/tensegrity_*` e2e modules; `examples/tensegrity_*.ri` |
| `elements::shell_frame`, `ShellSection`, shared condensation | `shell_assembly`, `elements/*`, `geometric_stiffness/membrane`, `shell_kinematics`, `shell_result`, `shell_solve`, `membrane_load` → reify-eval `shell_solve` / `membrane_load` trampolines (§3.4) | `crates/reify-cli/tests/harness_cli/cli_shell_eval.rs`; `crates/reify-eval-fea-tests/tests/shell_solve_e2e.rs`; `crates/reify-solver-elastic/tests/shell_benchmarks.rs` |
| `AssemblyElement::new`, private `ElementStiffness` storage | every assembly caller: reify-solver-elastic kernels, reify-eval `elastic_static` / `modal_ops`, reify-mesh-morph `elasticity` | every FEA e2e suite (elastic static, modal, buckling, shells, tensegrity) |
| `tensegrity_load_analysis` as a thin call | reify-eval `compute_targets/tensegrity_load.rs` (§3.4) | `harness_fea_solver_e2e/tensegrity_t3b_load.rs`; `crates/reify-solver-elastic/tests/tensegrity_t3b_load.rs` |
| private `MpcRow` | `apply_mpc_row_elimination`, `buckling_kernel` MPC reduction, reify-eval mixed-region dispatch | `crates/reify-eval/src/engine_build/mixed_region_tests.rs` (mixed tet+shell) |
| `tests/common/fea_harness.rs` | the 5 FEA integration-test files plus the tensegrity test files | those suites, green |
| `GeometryError::BackendUnavailable` | `mesher::map_geometry_error`, `volume_refine::map_geometry_error` → elastic_static refinement's "not available in this build" diagnostic | unchanged diagnostic text on a stub build |
| `TetSpatialIndex<'a>` | `resample.rs` (both grid resamplers) | the resample suites; GUI scalar channels through `resample_nodal_to_grid` |
| `tests/qoi.rs` | the QoI public surface | DWR suites (`crates/reify-solver-elastic/tests/dwr_boundary_tests.rs`) |
| reify-constraints predicate exports | reify-eval `engine_eval.rs` (§3.5 ConstraintSolver) | `crates/reify-eval/tests/centrality_default.rs`, `robustness_floor_signal.rs`, `continuous_cost_min_example_e2e.rs`; `crates/reify-cli/tests/harness_cli/cli_explain.rs` (`explain_centrality.ri`) |
| `solver/` module tree | `DimensionalSolver` via `ConstraintSolver`; `cpsat`, `registry`, `dual_jacobian` | every `reify check`/`eval` path with `auto` params; `crates/reify-constraints/tests/*` |
| adoption ratchets | the merge gate (crate-scoped test runs) | red on a re-added inline copy |

**Docs-truth gate: not triggered.** No grammar, builtin, stdlib signature, diagnostic code or
CLI/GUI behaviour changes. Diagnostic message text is byte-stable by contract (§5 C2, C8).

## 3. Background — the findings

These findings are from run `review-all-reify-20261010`. Statements and measurements are in
`plans/review-all-reify-20261010.json` under each key.

| key | anchor | sev | verdict | gist |
|---|---|---|---|---|
| fk-63218ffb51cd | `form_find.rs::form_find_anchored_surfaces` | high | confirmed | The sign rule is copied 6×, the anchor partition 3× (unchecked), and 3 error enums share 5 variants. There is no single input contract. |
| fk-edde1bcefb9f | `shell_assembly.rs::build_shell_frame` | medium | confirmed | Element primitives live in the assembly module: 8 upward imports and 2 module cycles. |
| fk-bc1e40784fe0 | `shell_assembly.rs::degenerate_stiffness_core` | medium | confirmed | Condensation is copied 2× and the copies diverged (the guard is in 1 of 2). KAPPA has 2 homes, G is inlined 16×, and the formulation is chosen by two bools. |
| fk-188e6a0adc9c | `assembly/global.rs::AssemblyElement` | medium | confirmed | The DOF-layout invariant is not uniformly enforced. `ElementStiffness` fields are public (72 `.data[` sites) and its stride doc is wrong. |
| fk-20c32882531a | `mpc.rs::MpcRow` | medium | confirmed | Public fields bypass `MpcRow::new`; struct literals are pinned by tests; the status prose is stale. |
| fk-8f79315fc7e8 | `membrane_load.rs::membrane_load_analysis` | medium | confirmed | 132 lines are copied from tensegrity_load, and the copies forked (the orphan-grounding guard). |
| fk-95dd3ef3a20a | `slug:solver-elastic-integration-test-harness-copies` | medium | confirmed | 69 extra helper copies over 38 names; `dedup_bcs` has drifted. |
| fk-c940cb1edad4 | `solver.rs::resolve_execution_modes` | low | weakened | Two mirror mode enums and one resolver caller. **See §10 Q1; no leaf here.** |
| fk-824124d1e74e | `shell_result.rs::ShellStress` | low | confirmed | Dead container. **Already filed: #8528** (adopted, §9). |
| fk-66ffd82ed984 | `interpolation.rs::TetSpatialIndex` | low | weakened | Every query replays all three build arguments; the tol contract is unchecked. |
| fk-c497935d4b8a | `volume_refine.rs::map_geometry_error` | low | weakened | gmsh absence is carried as a message substring. |
| fk-473bb6240e81 | `qoi.rs` | low | confirmed | The 1,613-line test module reaches only public items. The production docs answer reviewers. |
| fk-e719c0dc7f0d | `reify-constraints/src/solver.rs` | high | confirmed | The file has 9 unlayered concerns, a floor↔core cycle, a derive↔cost cycle, and 143 tests that reach 34 private items. |
| fk-813c018bf3cf | `engine_eval.rs::scope_qualifies_for_robustness_floor` | medium | confirmed | Five eval-side mirrors of solver predicates rest on a false "no dependency" premise. |

### 3.1 Freshness record (G6 — re-read at `8e6ffcac9d`)

`git diff a030ee7528..main` over every anchored path changes only `assembly/global.rs`, its two
re-export lines (`assembly/mod.rs`, `lib.rs`) and one doc block in `tests/determinism.rs`. All of
these come from task #8373 (solver-performance's node-owned `assemble_stiffness`). The effect on
the findings:

- **fk-188e6a0adc9c is partly fixed.** `AssemblyElement` now has one private `dofs_per_node()`, so
  the "3 inline `d_e` derivations" (6 per the skeptic) are now 1. Still true at main:
  - `AssemblyElement` and `ElementStiffness` have public fields;
  - `detect_orphan_dofs` checks only empty connectivity and node range, under `debug_assert!` and
    with no divisibility check, while `assemble_stiffness` asserts all three in release;
  - the `ElementStiffness` doc says "the DOF index is `3 · node_idx + axis`".

  The leaf scope (δ, ε) is narrowed to what remains.
- **fk-c940cb1edad4 is superseded by a committed PRD.** `docs/prds/v0_6/elastic-static-solver-performance.md`
  (decomposed 2026-10-08) deletes the execution-mode axis outright rather than unifying it:
  - β #8374 removes elastic_static's use of `resolve_execution_modes`;
  - ξ #8375 deletes `SolverMode::Parallel`, `PARALLEL_DOF_THRESHOLD`, `resolve_execution_modes`,
    `AssemblyMode` and the `assemble_global_stiffness` wrapper, and moves every caller to
    `assemble_stiffness(…, threads: NonZeroUsize, poll)`; its delivered checks assert
    `enum AssemblyMode` absent;
  - η #8381 deletes the then-single-variant `SolverMode`.

  The ruled remedy ("one ExecutionMode enum every assemble/solve entry takes") contradicts that
  plan, so no leaf here builds it. The disposition is §10 Q1, returned to the lead.
- Every other anchor reproduces at main: the six `MemberKind::Cable =>` production sites; the three
  unchecked `is_anchor[a] = true`; the eight upward `shell_assembly::` imports and both cycles; the
  two `let det_bb =` (only the degenerate copy guarded); `5.0 / 6.0` twice; the G formula 16×;
  `MpcRow` public fields and its struct literals at `lib.rs` doc and `mpc.rs` test; the tensegrity
  guard `node < n_nodes &&` absent from membrane_load; `qoi.rs` 2,423 lines with `#[cfg(test)]`
  at 811; `solver.rs` 11,788 lines; the five engine_eval mirrors and their "does NOT depend on
  reify-constraints" docs (Cargo.toml has reify-constraints as a normal dependency, task 4386).
- **Additional measurement (this session):** the in-crate Ge/Gt/Le/Lt/And op-rule pact
  (`collect_slack_terms`, `collect_floor_terms`, `derive_from_expr`) is a fourth lockstep copy of
  the slack rule inside reify-constraints. The weight × σ(sense) fold is written in four places:
  `solver.rs::eval_objective_set`, `registry.rs`'s fold, `cpsat.rs`'s ranking score, and
  `Engine::objective_term_contributions`. Both are folded into leaf ν.
- **In-flight work on the same files:**
  - **#8581** (extract the trial seam to `crate::trial`) and **#8449** (extract the
    bound-derivation family) are both pending, and both are subsets of ξ here.
  - **#6655** (rewrites the per-leaf derivation worker) and **#7492** (adds `TrialModel`) are
    pending edits to `solver.rs`.
  - **#7420** (dedupes `dimension_of`) is pending.
  - **[Amended 2026-10-20, retro-critic R2]** **Other active PRDs on `solver.rs` / the predicate seam** (§7.1 G4 rows):
    - solver-legibility-telemetry P4 **#6722** (ε: per-inequality slack keying) and **#6737**
      (μ: robustness-floor threading). #6722 now waits on ν #8645 and builds on ν's
      `for_each_inequality_leaf` (its "cross-crate copy is deliberate" premise is refuted by task
      4386); #6737 targets ν's `robustness_floor_applies` and ο's `solve_core` / `SolveRegime`.
    - geometry-algebra-solver-unification P2 **#6678** (κ), **#6680** (μ), **#6677** (ι) name
      `solver.rs`; ρ #8660 re-points them (and every other open task naming `solver.rs`) after π.
    - **#8799** (adoption-ratchet engine extraction into `reify_test_support::adoption_ratchet`) and
      **#6655** (HC4, already executing) now land before ν.
  - **#8375 / #8380 / #8381** (solver-performance) edit `membrane_load.rs`, `tensegrity_load.rs`,
    `buckling_kernel.rs`, `qoi.rs`, `tests/determinism.rs`, `tests/solver_gate_smoke.rs` and every
    `AssemblyElement` caller.

## 4. Resolved design decisions

| # | decision | why |
|---|---|---|
| D1 | **Force density:** `ForceDensityProblem` is validated once at construction. It range-checks member endpoints, anchors and surface corners, checks lengths, and checks signs through `MemberKind::admits(q)`, the sign rule's only home. Every anchored, aniso and free entry point builds one. `form_find_anchored` is the empty-surfaces case of the surfaces entry. | h11 SPOT, h10 uniform enforcement. Fix 6563 needed three commits for one guard. |
| D2 | **Force-density errors:** one `ForceDensityInputError` (`SignViolation { member }`, `DimensionMismatch`, `IndexOutOfRange { what, index, len }`, `NonTensionSurfaceStress { surface }`, `SurfaceCountMismatch`) nested as an `Input(..)` variant in `FormFindError`, `AnisoFormFindError` and `FreeFormError`. Solve-time failures stay in each solver's own enum. `map_ff_error` is deleted. The trampoline's `E_FormFindInfeasible` message text is byte-stable. | One input vocabulary; per-solver failure modes stay distinct (h6). |
| D3 | **Fixed-point driver:** the isotropic and anisotropic cotangent fixed-point loops become one driver parameterised by a per-triangle stencil (an `IsotropicStencil` / `AnisotropicStencil` implementing one trait). The member-force loop, the surface-stencil scatter and the line rank-1 scatter each have one copy, shared with `form_find_free`. | The two loops are line-identical except for the assembler (h3). |
| D4 | **Shell layering:** `ShellFrame`, `build_shell_frame`, `plane_stress_d`, `mat3_mul`, `membrane_node_pair_block` and `rotate_blockdiag` move to `elements/shell_frame.rs`. `shell_assembly`, `elements/*`, `geometric_stiffness/*`, `shell_kinematics`, `shell_result`, `shell_solve` and `membrane_load` all import downward from it. Crate-root re-exports are kept. `tilted_q_for_shell_tests` moves to a `#[cfg(test)] pub(crate) mod test_support` under `elements::shell_frame`. | Closes the shell_assembly↔degenerate_shell and shell_assembly↔shell_kinematics cycles (h9, h13). |
| D5 | **"KAPPA on the section type".** No shell section type exists today. A crate-private `ShellSection { thickness, material }` is built at the top of each public shell-stiffness entry from its existing `(nodes, thickness, material)` arguments, so the **public signatures are unchanged**. It owns `SHEAR_CORRECTION` (5/6, the one definition), `shear_modulus`, `membrane_d()` (t·D), `bending_d()` (t³/12·D) and `shear_kappa_g()`. `shell_result`'s stress recovery reads the same `ShellSection`. `IsotropicElastic::shear_modulus()` replaces the 16 inline `E / (2(1+ν))`. | One shear-correction fact; stiffness and stress recovery cannot desynchronise (h11). Keeping the public signatures avoids churning reify-eval call sites. |
| D6 | **Shell numerics:** one bubble-condensation function carries the release-time near-singular guard (the degenerate copy's `det_bb > 1e-14 · k00·k11`, else zero correction), and the MITC3+ path calls it too. One bubble K_BB integral, one `scaled(d, s)` helper and one `ElementStiffness` packing helper. `degenerate_stiffness_core`'s two bools become `DegenerateFormulation { membrane: MembraneStrain::{Displacement, Assumed}, bubble: RotationBubble::{Off, On} }`. The three public wrappers keep their names. | The MITC3+ unguarded divide is latent today (K_NB is bit-zero), but it fires the day K_NB activates (h10). |
| D7 | **`AssemblyElement`:** fields become private. `AssemblyElement::new(id, connectivity, k_e)` asserts non-empty connectivity, divisibility and `dofs_per_node ≥ 1`, with a message naming the element id. That follows the existing release-`assert!` policy (Task 2544), so construction **panics**; it does not return `Result`. The node-range check needs `n_nodes`, so it stays in one check function that **both** `assemble_stiffness` and `detect_orphan_dofs` call in release. | The intrinsic invariant is enforced at construction; the extrinsic invariant is checked uniformly at both entry points (h10). |
| D8 | **`ElementStiffness`:** fields become private behind `zeros`, `from_row_major(n, Vec)` (asserts `len == n²`), `n_dofs()`, `get`, `add`, `set`, `as_row_major()`. The doc states the stride as `n_dofs / n_nodes`. | The `data.len() == n_dofs²` invariant gets a home (h10, h5). |
| D9 | **Load kernels:** `tensegrity_load_analysis` becomes a thin call of `membrane_load_analysis(…, &[] /* no patches */, …)`. `TensegrityLoadOptions` and `TensegrityLoadError` become `pub type` aliases of the membrane types; they are field- and variant-identical today. `TensegrityLoadSolve` is mapped from `MembraneLoadSolve`, with `slack ← member_slack` and the surface fields dropped. The copied helpers and the copied `GROUNDING_DIAGONAL` go. The merged kernel has **one** orphan-grounding policy: the leaf reads the validation preamble and either keeps `node < n_nodes &&` with a test, or proves it redundant and pins that. | h11. The fork has already happened once. |
| D10 | **`MpcRow`:** fields become private with `dofs()`, `coeffs()` and `rhs()` accessors. `MpcRow::new` and `shell_tet_tying` are the only constructors. The struct-literal smoke test and the doctest line are deleted, and the module and struct docs describe the shipped surface. | h10. A zero pivot or a length mismatch becomes unrepresentable. |
| D11 | **Test harness:** the integration FEA harness lives in `crates/reify-solver-elastic/tests/common/fea_harness.rs`, declared from the existing `tests/common/mod.rs` (ruling: "one tests/common FEA harness"). It has one `dedup_bcs`, which always panics on conflicting duplicate-DOF values. The tensegrity `canonical_prism` / `triplex_topology` redefinitions use `common`. `src/assembly/test_support.rs` and the in-crate `global.rs` fixture copies are out of scope (§8). | h11. The ruling names the home. |
| D12 | **[Amended 2026-10-20, retro-critic R2]** **Dead text, superseded by S10 R5** (κ #8632 cancelled 2026-10-14, subsumed by S10 β₁ #8935, which removes the gmsh-free build; fk-c497935d4b8a rides on #8935). ~~**gmsh absence:** `GeometryError::BackendUnavailable { backend: &'static str }` is added in reify-ir with a Display arm. Both reify-kernel-gmsh stubs (`mesh_plane_2d`, `refine_volume`) return it. Both `map_geometry_error` copies match it structurally, and `msg.contains(STUB_UNAVAILABLE_MARKER)` is deleted. The constant goes once it has no reader. The "not available in this build" diagnostic text is byte-stable.~~ (superseded by S10 R5) | h12. See §10 Q2 on reify-ir ownership. |
| D13 | **Spatial index:** `TetSpatialIndex<'a>` borrows `nodes` and `elems` and owns `tol`; `locate(point)` and `locate_counted(point)` take only the point. `volume_refine::TetShape<'a>` carries its `&VolumeMesh`. | h7, h10. The same-mesh and same-tol contracts become unrepresentable to break. |
| D14 | **QoI tests:** `qoi.rs`'s test module moves verbatim to `crates/reify-solver-elastic/tests/qoi.rs` (every item it uses is re-exported at the crate root). Measured-release-behaviour, rejected-alternative and TDD-step prose leaves the production docs; the PRD section pointers stay. | The file's single move (ruling); h14, comments stance. |
| D15 | **Predicate home:** reify-constraints exports, over reify-ir types only: `has_inequality_slack(&CompiledExpr)`, `objective_is_money(&ObjectiveSet)`, `centrality_gate(auto_types, constraints)`, `robustness_floor_applies(objective, constraints)` and `signed_term_contribution(&ObjectiveTerm, f64)`. One `for_each_inequality_leaf` decomposition (Ge/Gt/Le/Lt under And → greater side, lesser side, slack type) feeds `collect_slack_terms`, `collect_floor_terms` and `has_inequality_slack`. **[Amended 2026-10-20, retro-critic R2]** ~~`derive_from_expr` reuses its orientation.~~ (superseded by R2 M3) HC4 #6655 lands before ν and replaces the linear-only `derive_from_expr` miner; the HC4 worker's comparison handling (in `src/hc4.rs` / `src/interval.rs`) consumes `for_each_inequality_leaf`. ν's Ge/Gt orientation ratchet row covers `crates/reify-constraints/src/**` including `hc4.rs` and `interval.rs`, and the row states its exact needle. The weight × σ fold-site row is scoped to `src/` only, so cpsat's deliberately independent test oracle is not forced onto the helper. The solver's own gates (`build_centrality_objective`, the floor gate in the core, the `cost_robustness_lambda` dispatch) call the exports. engine_eval.rs deletes its five mirrors and calls them, and the lockstep prose goes. The finite-bounds caveat stays a documented solver-only check. | h11. The "cannot depend" premise is false (Cargo.toml line 34, task 4386). |
| D16 | **solver.rs tree:** `crate::trial` (crate level, as #8581 measured; `cpsat`, `registry` and `dual_jacobian` import it) < `solver/residual` < `solver/bounds` (**[Amended 2026-10-20, retro-critic R2]** ~~`effective_bounds`, `default_bounds_for`, which need only `dimension_of`~~ (superseded by R2 B3): `crate::domain` owns `StrictBound` and the dimension-default box `default_bounds_for` / `effective_bounds` and imports nothing from `crate::solver`; `solver/bounds` imports them from `crate::domain` or is deleted) < `solver/derive` (+`extract_initial_point`) < `solver/cost` (`ConstraintCostFunction`, `build_centrality_objective`, `eval_objective_set`, `build_simplex`, `multistart_points`, `SolveRegime`, uniqueness tolerances) < `solver/floor` (`objective_is_money`, `collect_floor_terms`, `robustness_margin_for`, `synthesise_floor_constraints`, `worst_unmet_floor_term`) < `solver/core` (`solve_core` plus **`constraints_witness` and `floor_infeasible_diagnostic`**) < `solver/tradeoff` < `solver/uniqueness` < `solver.rs` (the `ConstraintSolver` impl, `solve_with_meta`, `solve_ranked_impl`). The parent keeps the non-mod-rs layout used by `engine_build.rs` / `geometry_ops.rs`. | The partition measured in the record (10 partitions, `stays_whole: false`). |
| D17 | **Closing the floor↔core cycle.** The ruling co-locates `constraints_witness` with the core. Measured at main: `floor_infeasible_diagnostic` *also* calls `constraints_witness`, and the core calls `floor_infeasible_diagnostic`, so co-locating `constraints_witness` alone leaves the cycle intact. Both therefore live in `solver/core`. `floor.rs` keeps the pure synthesis, which the core calls downward. | This is the minimal extension of the ruling that actually breaks the cycle (§10 Q3 asks the lead to confirm it). |
| D18 | **Core rename:** `solve_core_with_sd_tolerance` loses its tolerance parameter (all 5 production call sites pass `NM_SD_TOLERANCE`) and is renamed `solve_core`. The present thin `solve_core` wrapper is folded into it. | h1. |
| D19 | **solver.rs tests:** a test whose subject is one layer moves into that layer's child test module and reaches only what that layer exposes `pub(super)`. A test of `ConstraintSolver` behaviour moves to `crates/reify-constraints/tests/solver_integration.rs` (it exists). A test that reached a private helper of *another* layer is rewritten against the owning layer's interface, never by widening visibility. No assertion changes value. `#5721`'s visibility rule holds: `fold_dependent_cells_skipping_collisions` stays private, and its tests live in `trial`. | Tests stance: tests that reach internals signal an interface smell. A behaviour-preserving restructure must not turn 143 tests red. |
| D20 | **Ratchets:** each extracted primitive ships an adoption ratchet (INV candidate 5). Each crate gets one ratchet test binary — `crates/reify-solver-elastic/tests/adoption_ratchets.rs` (created by α) and `crates/reify-constraints/tests/adoption_ratchets.rs` (created by ν) — in the `version_id_discipline_gate.rs` shape. **[Amended 2026-10-20, retro-critic R2]** #8799 lands first and extracts the ratchet engine into `reify_test_support::adoption_ratchet`; ν's host consumes it and keeps only its RATCHETS table, the BT14 loop and the real-tree test (no second lexer). A row is a named pattern, a production-scope path set and a committed baseline; it goes red when the count rises above the baseline, and each leaf lowers its row to the target. Seeded self-tests prove each row both flags and clears. The binaries are fast, carry no wall-clock bound, and need no nextest partition row. | The 3-of-N abandonment shape cannot recur. Crate-scoped runs trigger exactly when the crate is edited. |

## 5. Contract (the H component)

- **C1 `ForceDensityProblem`** (reify-solver-elastic `form_find`):
  - **Construction:** `ForceDensityProblem::new(nodes, members, kinds, q, anchors, surfaces, surface_stresses) -> Result<Self, ForceDensityInputError>`.
  - **Invariant after `Ok`:**
    - every index is `< nodes.len()`;
    - `members`, `kinds` and `q` have equal lengths;
    - `kinds[i].admits(q[i])`;
    - `surfaces.len() == surface_stresses.len()`;
    - every σ is > 0.

  The anchored, aniso and free entry points take it, or build it as their first statement. No
  solver body indexes an unvalidated slice.
- **C2 `MemberKind::admits(self, q: f64) -> bool`:** `Cable ⇒ q > 0`, `Strut ⇒ q < 0`, NaN ⇒ false.
  It is the only place in `crates/*/src` where a member kind is matched to a sign.
- **C3 `elements::shell_frame`:** the six primitives keep their signatures and their bits. The
  crate-root re-exports (`ShellFrame`, `build_shell_frame`, `plane_stress_d`) keep their paths. No
  non-test file under `elements/`, `geometric_stiffness/`, `shell_kinematics.rs` or
  `membrane_load.rs` imports from `shell_assembly`.
- **C4 `ShellSection`** (crate-private): the only definition of the shear-correction factor in
  `crates/reify-solver-elastic/src`. Stiffness and stress recovery both read it.
- **C5 `AssemblyElement::new` / `ElementStiffness`:**
  - Construction panics on an intrinsic violation and names the element id.
  - `assemble_stiffness` and `detect_orphan_dofs` share one release-mode extrinsic check.
  - `ElementStiffness` exposes no public field.
- **C6 `MpcRow`:** no public field. Every value satisfies `dofs.len() == coeffs.len() > 0`, with
  `coeffs[0]` finite and non-zero and `rhs` finite.
- **[Amended 2026-10-20, retro-critic R2]** **Dead text, superseded by S10 R5** (κ #8632 cancelled, subsumed by S10 β₁ #8935). ~~**C7 `GeometryError::BackendUnavailable { backend }`:** the only representation of "this
  build lacks backend X". No consumer reads `GeometryError::OperationFailed`'s string to recover it.~~
- **C8 reify-constraints predicate exports:**
  - All five are `pub fn` over reify-ir types and are re-exported at the crate root.
  - The solver's own gates call them.
  - `engine_eval.rs` defines no function that re-derives any of them.
  - Diagnostic and `centrality_synthesized_scopes` outputs are identical before and after.
- **C9 `solver/` tree:** for every pair of modules in the tree, at most one non-test `use`/call
  edge direction exists (no module SCC larger than 1). The public surface of reify-constraints is
  unchanged. **[Amended 2026-10-20, retro-critic R2]** **Guard:** `scripts/assert-module-scc.py` measures top-level modules only, so
  it cannot see C9. π adds a row to ν's `crates/reify-constraints/tests/adoption_ratchets.rs` that
  lexes `super::<child>` / `crate::solver::<child>` edges among `src/solver/*.rs`, asserts the D16
  layer order, and carries a seeded self-test proving a core↔floor import reds.

## 6. Boundary tests (both sides)

| # | side | scenario | pre | post (asserted) | leaf |
|---|---|---|---|---|---|
| BT1 | solver | out-of-range anchor or member endpoint into `form_find_anchored_surfaces` | index = nodes.len() | `Err(FormFindError::Input(IndexOutOfRange{..}))`, no panic | η |
| BT2 | solver | cable with q ≤ 0 / strut with q ≥ 0 / NaN q, for each of the anchored, aniso and free entries | — | `Input(SignViolation{member})` from all of them | η |
| BT3 | consumer | `examples/tensegrity_t_prism.ri` through `reify eval` | — | output equals `tests/golden/tensegrity_t_prism.txt` unchanged; the `E_FormFindInfeasible` message tests are unchanged | η |
| BT4 | solver | mixed tet+shell mesh with a malformed element (`n_dofs % nodes ≠ 0`) passed to `detect_orphan_dofs` in release | — | panics naming the element id (today: silent truncation) | δ |
| BT5 | solver | `ElementStiffness::from_row_major(n, v)` with `v.len() ≠ n²` | — | panics | ε |
| BT6 | solver | MITC3+ element with near-singular K_BB | det_bb below the guard | zero correction, finite K (today: unguarded divide) | γ |
| BT7 | consumer | CLI shell eval and the shell_solve e2e | — | unchanged values | β, γ |
| BT8 | solver | `MpcRow::new` with a zero pivot or a length mismatch | — | rejected; no public path builds one otherwise | θ |
| BT9 | consumer | tensegrity T3b load e2e (solver and eval harness) through the thin call | — | unchanged expectations | ζ |
| BT10 | consumer | **[Amended 2026-10-20, retro-critic R2]** **Dead text, superseded by S10 R5** (κ #8632 cancelled, subsumed by S10 β₁ #8935). ~~a stub-build `GeometryError::BackendUnavailable` routed by both `map_geometry_error`s~~ | — | ~~`GmshUnavailable` variants; diagnostic text unchanged~~ | ~~κ~~ |
| BT11 | solver | `TetSpatialIndex` locate parity against `locate_element_p1` | — | bit-identical over the existing resample fixtures | λ |
| BT12 | consumer | `centrality_default.rs`, `robustness_floor_signal.rs`, `cli_explain` (`explain_centrality.ri`) | — | unchanged | ν |
| BT13 | solver | `ConstraintSolver` integration tests across the partition | — | unchanged values; no assertion edits | ξ, ο, π |
| BT14 | gate | each ratchet row: add one inline copy in a seeded fixture | — | red; migrating one copy without lowering the baseline → red ("lower the baseline") | α, ν |

## 7. Pre-conditions

- **Substrate verified at main (G3):**
  - reify-eval → reify-constraints is a normal dependency.
  - `IsotropicElastic` has public `youngs_modulus` / `poisson_ratio`; `fn shear_modulus` is
    absent today.
  - `elements/` exists with `mod.rs`; `tests/common/mod.rs` exists (92 lines, tensegrity).
  - `GeometryError` (reify-ir `geometry.rs`) has no exhaustive match outside its own Display impl.
  - A stub-arm compile of reify-kernel-gmsh is reachable by building with an empty `GMSH_LIB_DIR`
    (the build script reads it; the `check-manifold-deps.sh` behavioural loop uses the same
    mechanism).
  - The non-mod-rs parent-plus-directory layout is in use (`engine_build.rs` + `engine_build/`).
- **Upstream tasks** (out-of-batch dependencies; real `add_dependency` edges at decompose):
  - #8375 (solver-performance ξ) before δ, ζ, ι. It moves every assembly caller, deletes
    `AssemblyMode`, rebuilds `tests/determinism.rs` and edits `membrane_load.rs` and
    `tensegrity_load.rs`.
  - #8528 (ShellStress deletion) before γ, since both edit `shell_result.rs`.
  - #6655, #7492 and #7420 before ξ, since all three are pending edits to `solver.rs` /
    `dimension_of`.
  - **[Amended 2026-10-20, retro-critic R2]** #8799 (ratchet-engine extraction) and #6655 (HC4) before ν; ν before
    solver-legibility-telemetry #6722.
  - **[Amended 2026-10-20, retro-critic R2]** η₂ #9271, φ-ff #9273, χ-ff #9274 and #7801 before analysis-stratum S5 ξ #9006.

### 7.1 Cross-PRD relationship and seam owners (G4)

Seam owners are from the program doc's G4 table. Every seam below that the table lists is S14's.

| shared artefact | owner | this PRD's role |
|---|---|---|
| `ElementStiffness.data` direct indexing (72 sites, 9 files) | **S14** | ε edits it |
| `tilted_q_for_shell_tests` fixture (6 modules) | **S14** | β edits it |
| reify-constraints solver predicate exports consumed by `engine_eval.rs` | **S14** | ν edits it |
| reify-ir `GeometryError` | *unlisted* (S10 owns the `GeometryKernel` trait and typed replies) | **[Amended 2026-10-20, retro-critic R2]** none: κ #8632 is cancelled (subsumed by S10 β₁ #8935, S10 R5). ~~κ adds one variant by default; §10 Q2~~ |
| `engine_eval.rs` (S4 routes cell-eval sites through `cell_eval_ctx`, wave 2) | S4 for the cell-eval mechanism | ν deletes five mirror fns and swaps one fold. `objective_term_contributions`' `eval_ctx_with_meta` call is left for S4. |
| reify-eval `compute_targets/{form_find,tensegrity_load,membrane_load,elastic_static}.rs`, `modal_ops.rs` (S5 crate move, wave 2; S15 decoders, wave 1; S16 trampoline factoring, wave 3) | S5 / S15 / S16 by area | S14 makes only error-mapping or constructor-call edits there, in wave 1, before S5 moves the files. **[Amended 2026-10-20, retro-critic R2]** The gap-fill leaves (η₂ #9271, φ-ff #9273, χ-ff #9274, #7801) edit the form-find trampoline before S5 ξ #9006 moves it; edges `#9006 ← #9271, #9273, #9274, #7801` are recorded. |
| reify-solver-elastic `shell_boundary.rs` (S15 fk-f33ff551c66d `SupportKind`) | S15 | β edits only its `#[cfg(test)]` fixture import |
| reify-compute-contract `elastic_result.rs` doc pointer to `ShellFrame` | S5 (owns that crate's dependency edge) | β fixes a doc path only |
| `docs/prds/v0_6/elastic-static-solver-performance.md` leaves #8375, #8380, #8381 | that PRD | δ, ζ, ι depend on #8375; fk-c940cb1edad4 defers to it (§10 Q1) |
| INV candidate 7 (no intra-crate SCC grows; Tarjan baseline) | S3 / S7 | β removes two reify-solver-elastic SCC edges and π leaves `solver/` acyclic. ~~Whichever lands later records the smaller baseline.~~ (superseded by R2 B1) **[Amended 2026-10-20, retro-critic R2]** The INV-ARCH-1 tool measures top-level modules only; C9's guard is π's ratchet row in ν's host (§5 C9). |
| `reify-constraints/src/solver.rs` and its predicate exports (solver-legibility-telemetry P4 ε #6722, μ #6737) | **S14** | **[Amended 2026-10-20, retro-critic R2]** ν first: `#6722 ← #8645`. #6722 builds slack on ν's `for_each_inequality_leaf`, threads `ConstraintNodeId` through `collect_slack_terms` only and calls the reify-constraints export from eval; #6737 targets ν's `robustness_floor_applies` and ο's `solve_core` / `SolveRegime`. |
| `reify-constraints/src/solver.rs` (geometry-algebra-solver-unification P2 κ #6678, μ #6680, ι #6677) | **S14** for the split | **[Amended 2026-10-20, retro-critic R2]** S14 splits first; ρ #8660 re-points every open task naming `solver.rs` to its `solver/<layer>.rs` home after π. |
| INV candidate 5 (extract-with-ratchet) | repo / overlay G-gate | α and ν host the ratchet members; every extract leaf names its row |

## 8. Out of scope

- The execution-mode axis (fk-c940cb1edad4): owned by elastic-static-solver-performance #8375 /
  #8381 pending §10 Q1.
- `src/assembly/test_support.rs` and the in-crate `global.rs` fixture copies
  (`kuhn_split_hex_to_six_tets_fixture`, `box_p1_mesh_fixture`). They are not in
  fk-95dd3ef3a20a's measurement; a follow-up may fold them into one home.
- `solve_core`'s documented `unique: true` placeholder: the finding names it, but its proposal does
  not fix it. A typed result split is a separate design.
- The reify-eval compute-target trampolines' own decode duplication (S15) and their crate move
  (S5).
- file:line pointers in volume_refine.rs, interpolation.rs and similar files (S23,
  fk-e4b87a3c5f90). This stream rewrites only the stale *status* prose in `mpc.rs` and the
  reviewer prose in `qoi.rs`, which its own keys own.

## 9. Decomposition plan

Labels are this PRD's own; other PRDs' tasks are cited by id only. Every leaf's diff carries its
ratchet row lowered to target (D20) where it extracts a primitive. Every leaf's signal includes
"existing suites green with no expectation edits".

| label | title | modules | x_finding_key | prereqs | observable signal |
|---|---|---|---|---|---|
| α #8614 | Adoption-ratchet host for reify-solver-elastic | `crates/reify-solver-elastic/tests/adoption_ratchets.rs` | — (INV cand. 5 enabler) | — | The binary runs on the gate. Its rows (below) are at today's baselines, and seeded self-tests prove each row flags and clears (BT14). |
| β #8616 | Shell primitives below assembly: `elements/shell_frame.rs` | solver-elastic `elements/`, `shell_assembly`, `geometric_stiffness/membrane`, `shell_kinematics`, `shell_result`, `shell_solve`, `membrane_load`, `shell_boundary` (tests), `lib.rs`; reify-eval test `harness_topology_selector/shell_channels_mapping.rs` (import path); the `ShellFrame::local_to_global` doc pointer in reify-compute-contract `elastic_result.rs` (doc only) | fk-edde1bcefb9f | α | Upward-import row = 0; both cycles gone; BT7 green. Owns the `tilted_q_for_shell_tests` seam. |
| γ #8618 | `ShellSection`, one guarded condensation, scaled/packing helpers, `DegenerateFormulation` | `shell_assembly`, `shell_result`, `constitutive`, `error_estimator` | fk-bc1e40784fe0 | β, #8528, α | Ratchet rows: `let det_bb =` → 1, `5.0 / 6.0` → 1, inline G → 1 (the `shear_modulus` body), `td[i][j] =` → 0. BT6 new and green; BT7 green. |
| δ #8619 | `AssemblyElement::new` validating constructor; one release check for both entry points | solver-elastic `assembly/global.rs` and every `AssemblyElement {` site (≈ 95 across solver-elastic src/tests, reify-eval `elastic_static`/`modal_ops`, reify-mesh-morph) | — (prerequisite; fk-188e6a0adc9c is closed by ε) | α, #8375 | BT4 new and green; ratchet row `AssemblyElement {` struct literals outside `global.rs` → 0. |
| ε #8621 | Private `ElementStiffness` storage; stride doc fixed | `assembly/mod.rs` + the 72 `.data[` sites (`resample`, `p2_tet`, `mass_matrix`, `geometric_stiffness/*`, `tensegrity_load`, `membrane_load`, `shell_assembly`, `membrane_cst`) and tests; **[Amended 2026-10-20, retro-critic R2]** reify-eval `tests/harness_topology_selector/shell_mixed_region_validation.rs` (the one external reader, migrated to `get()`/`n_dofs()`) | fk-188e6a0adc9c | δ, β, γ, ζ | BT5 new and green; ratchet row `.data[` outside `assembly/mod.rs` → 0. Owns the `ElementStiffness.data` seam. |
| ζ #8626 | `tensegrity_load_analysis` as a thin call of `membrane_load_analysis` | `tensegrity_load.rs`, `membrane_load.rs`, `lib.rs` | fk-8f79315fc7e8 | α, #8375 | BT9 green; ratchet row: definitions of `member_force_delta` / `bar_axial_force_delta` / `GROUNDING_DIAGONAL` → 1 each; one orphan-grounding policy pinned by a test. |
| η #8628 | `ForceDensityProblem`, `MemberKind::admits`, nested input errors, one stencil-parameterised fixed-point driver | `form_find.rs`, `form_find_free.rs`, `lib.rs`; reify-eval `compute_targets/form_find.rs` (error mapping only) | fk-63218ffb51cd | α | BT1, BT2 new and green; BT3 green. Ratchet rows: `MemberKind::Cable =>` in production → 1 (`admits`); `is_anchor[a] = true` in production → 1. `map_ff_error` gone. |
| θ #8629 | Private `MpcRow` fields; delete the literal smoke test and doctest; true module docs | `mpc.rs`, `lib.rs`, `buckling_kernel.rs`; reify-eval `engine_build/mixed_region_tests.rs` | fk-20c32882531a | α | BT8 new and green; ratchet row `MpcRow {` literals outside `impl MpcRow` → 0; mixed-region tests green. |
| ι #8630 | One FEA harness in `tests/common/fea_harness.rs`; one conflict-checking `dedup_bcs` | `crates/reify-solver-elastic/tests/*.rs`, `tests/common/` | fk-95dd3ef3a20a | α, #8375 | Ratchet row "top-level non-`test_` fn name defined in > 1 file under tests/" → 0 (today 38 names / 69 extra copies). A mis-specified duplicate BC panics in every suite. |
| κ #8632 | **[Amended 2026-10-20, retro-critic R2]** **Dead text: cancelled 2026-10-14, subsumed by S10 β₁ #8935 (S10 R5).** ~~`GeometryError::BackendUnavailable`; structural routing in both `map_geometry_error` | reify-ir `geometry.rs`; reify-kernel-gmsh `mesh_profile_2d.rs`, `refine_volume.rs`, `lib.rs`; solver-elastic `mesher.rs`, `volume_refine.rs` | fk-c497935d4b8a | — | BT10 green; `msg.contains(STUB_UNAVAILABLE_MARKER)` gone; the stub arm compiles with an empty `GMSH_LIB_DIR`.~~ |
| λ #8637 | `TetSpatialIndex<'a>` owns its mesh borrow and tol; `TetShape<'a>` carries its mesh | `interpolation.rs`, `resample.rs`, `volume_refine.rs` | fk-66ffd82ed984 | — | BT11 green; `locate` takes only the point. |
| μ #8640 | `qoi.rs` tests to `tests/qoi.rs`; reviewer prose out of production docs | `qoi.rs`, `tests/qoi.rs` | fk-473bb6240e81 | — | `tests/qoi.rs` runs the 20 tests unchanged; `qoi.rs` has no `#[cfg(test)]` module. |
| ν #8645 | One predicate home in reify-constraints; engine_eval mirrors deleted; reify-constraints ratchet host | reify-constraints `solver.rs`, `registry.rs`, `cpsat.rs`, `lib.rs`, `tests/adoption_ratchets.rs`; reify-eval `engine_eval.rs` | fk-813c018bf3cf | **[Amended 2026-10-20, retro-critic R2]** #8799, #6655 | BT12 green. Ratchet rows: Ge/Gt orientation arms in reify-constraints src → 1; weight × σ fold sites → 1. The five eval-side mirror fns gone. Owns the predicate-exports seam. |
| ξ #8653 | solver.rs lower layers: `crate::trial`, `solver/residual`, `solver/bounds`, `solver/derive` with their tests | reify-constraints `solver.rs`, `solver/`, `trial.rs`, `lib.rs`, `cpsat.rs`, `registry.rs`, `dual_jacobian.rs` | — (prerequisite of π) | ν, #6655, #7492, #7420 | BT13 green; the derive↔cost cycle is gone. **Subsumes #8581 and #8449** (decompose cancels them with a reason naming this leaf). |
| ο #8656 | solver.rs middle layers: `solver/cost`, `solver/floor`, `solver/core` (+`constraints_witness`, `floor_infeasible_diagnostic`); core renamed | reify-constraints `solver.rs`, `solver/` | — (prerequisite of π) | ξ | BT13 green; the floor↔core cycle is gone (D17); `solve_core_with_sd_tolerance` gone. |
| π #8658 | solver.rs upper layers: `solver/tradeoff`, `solver/uniqueness`, api parent; remaining tests to layer interfaces or `tests/solver_integration.rs` | reify-constraints `solver.rs`, `solver/`, `tests/solver_integration.rs` | fk-e719c0dc7f0d | ο | BT13 green; `solver.rs` has no inline `mod tests` **[Amended 2026-10-20, retro-critic R2]** and no `src/solver/tests.rs` stitch; C9 holds (no module SCC > 1 in `solver/`), guarded by π's layer-order row in ν's ratchet host; no test reaches another layer's private item. |
| ρ #8660 | PRD close | this PRD, its manifest | — | α–π | The terminal Status header, per the overlay's freeze shape. **[Amended 2026-10-20, retro-critic R2]** After π: one scripted `update_task` pass (fused-memory MCP, never tasks.db) re-points every non-terminal task naming `solver.rs` (`metadata.files` complete list plus a details note mapping each anchor to its `solver/<layer>.rs` home); also reconciles the dead κ text and the stale §9 counts (#8783). |
| (adopted) | Delete the dead `ShellStress` | `shell_result.rs`, `lib.rs` | fk-824124d1e74e | — | **#8528** (filed by the run). Not re-filed; γ depends on it. |

Ratchet rows α lands at today's baselines (each leaf lowers its own): upward `shell_assembly`
primitive imports (8 → 0, β); `let det_bb =` (2 → 1), `5.0 / 6.0` (2 → 1), inline
`/ (2.0 * (1.0 + nu))` (16 → 1), `td[i][j] = ` (5 → 0) (γ); `AssemblyElement {` outside
`global.rs` (δ); `.data[` outside `assembly/mod.rs` (72 → 0, ε); duplicated load helpers (2 → 1
each, ζ); `MemberKind::Cable =>` in production (6 → 1), unguarded `is_anchor[a] = true` in
production (3 → 1) (η); `MpcRow {` literals outside the impl (2 → 0, θ); cross-file duplicate test
helpers (69 → 0, ι). The implementer re-measures each baseline at α's tree; the counts above are
the review's.

## 10. Open questions

Lead rulings (2026-10-12, program doc "Resolved decisions — pass 1 open questions"): Q1 → (A), fk-c940cb1edad4 stamped on #8375 and #8381, no leaf; ~~Q2 → (A), κ in wave 1, S10 consumes~~ (superseded by S10 R5: κ #8632 cancelled 2026-10-14, subsumed by S10 β₁ #8935); Q3 → (A), both into solver/core (D17). #8581 and #8449 cancelled as subsumed by ξ #8653.

1. **fk-c940cb1edad4 (returned to the lead — ruling contradicts committed plan).** See §3.1.
   Options:
   - **(A)** Stamp the key onto #8375 (and #8381 for `SolverMode`): "work exists"; no S14 leaf.
   - **(B)** Accept it, as a cancelled task with `x_acceptance_reason` naming #8375 / #8381.
   - **(C)** Build `ExecutionMode` anyway, which contradicts #8375's delivered check
     `enum AssemblyMode` absent.

   Suggested: (A).
2. **GeometryError ownership.** The G4 table names no owner for reify-ir's `GeometryError`; S10 owns
   the `GeometryKernel` trait and typed replies. Default: κ adds the one variant and its Display
   arm in wave 1, and S10 consumes it. Alternative: hold κ on S10 (wave 2) with `x_seam_wait`.
3. **D17 extension of the ruling.** Co-locating `constraints_witness` alone does not close the
   floor↔core cycle, because `floor_infeasible_diagnostic` also calls it. Default: both move to
   `solver/core`. Alternative: `constraints_witness` takes the re-solve as a closure and stays in
   `floor`.
4. *(tactical, η)* Whether `DegenerateTriangle` is an input error or a solve-time error depends on
   whether the cotangent iteration can degenerate a triangle that was valid at input. The leaf
   reads the loop and places it. The default is both: an input check at construction, plus the
   solver variant for mid-iteration degeneration.
5. *(tactical, ι)* Dead-code lints in test binaries that `mod common;` without using every helper.
   Default: `#![allow(dead_code)]` at the top of `tests/common/mod.rs`, matching the
   `assembly/test_support.rs` precedent.

## Gap-fill amendments (2026-10-16)

Rulings by Leo in the gap-fill sitting of 2026-10-16 (`plans/gap-fill-deliberation-reify-20261016.md`).
Every new leaf follows an #8612-closure leaf (η #8628, ξ #8653, #6655) rather than widening it (§G1).
Manifest blocks: `solver-crates-layering.capability-manifest.yaml` / `.md`, "Gap-fill leaves".

- **§K3-D3** (ParamDomain carrier; re-homes #5470's discrete domain field). New high leaf
  **ξ₂ #9267**, after #6655 and ξ #8653: `reify_constraints::domain::{ParamDomain, Endpoint,
  DomainTable}` with `derive_domains` once per component after decomposition, `lo > hi` a typed
  Refuted, readers as projections (seed_box, clamp_box, int_range, default_bounded_sides),
  `AutoParam.bounds` and its three `None` writers deleted. ~~It sits at crate level below
  `solver/derive`;~~ (superseded by R2 B3) **[Amended 2026-10-20, retro-critic R2]** `crate::domain` owns the strictness policy
  (`StrictBound`, or `Endpoint::Open` as its successor) and the dimension-default box
  (`default_bounds_for` / `effective_bounds`) and imports nothing from `crate::solver`;
  `solver/bounds` and `solver/derive` import from it or are deleted; ξ #8653's "move as written"
  for those items is re-pointed to domain; ξ₂'s signal gains "`scripts/assert-module-scc.py check`
  green, no {domain, solver} SCC". The carrier decision (A′) stands; ξ stays behaviour-preserving (the carrier's behaviour change is ξ₂'s, not ξ's).
  New medium leaf **ξ₃ #9268** (`leaf_verdicts`, after ξ, in `solver/residual`): acceptance calls
  the checker's comparison semantics (with #6653's equality policy), NaN/Undef are Undecidable,
  StrictBound::Dropped/SteppedInside become search hints; absorbs #9012. #5470 now depends on ξ₂
  and its delivered check is re-pointed to `crates/reify-constraints/src`
  (`discrete-cost-minimisation` amendment of the same date).
- **§Uncontested block → K6-D1.** New medium leaf **η₂ #9271** after η #8628: `FreeSpec {
  Explicit(q) | GroupRatios(GroupSpec) }` with `GroupSpec::new` the only group-check home;
  finiteness in `admits` and the σ test; `AnisotropicSurfaceStress::new` rejecting zero/NaN
  warp_dir as `DegenerateMaterialFrame`; node finiteness at construction;
  `ForceDensityInputError::NonFinite { what, index }`; new boundary cases for NaN σ, +inf q,
  zero/NaN warp_dir and an out-of-range group id, each through the free entry. Extends C1's
  invariant to "every form-find input is finite, in range and sign-admissible". #7800 is folded in
  and cancelled as subsumed. η #8628 gets a note only.
- **§K6-D2.** #7801 is adopted into the stream and widened (low → medium):
  `FreeStandingForm::certify(d, x)` is the single nullity-(d+1) check, after η #8628. New medium
  leaf **η₃ #9272** after #7801: the GroupRatios gauge covariance at λ = 2^±20 is measured first;
  `fd_criteria::StiffnessScale` replaces the normalisers only if that test reds. #7571 gets a pointer.
- **§K6-D3.** Three leaves after η #8628: **φ-ff #9273** (low; `fd_matrix`/spectrum module owning
  `assemble_d` and `SymmetricSpectrum::of`, typed `SpectrumError`; ratchet row
  `.self_adjoint_eigen(` outside the module → 0), **χ-ff #9274** (medium; free-loop split:
  `CombinedIterate`, `QSource { FixedQ, SearchedQ }`, `StallGuard`, relax step; after φ-ff and
  #7801; #7613 depends on it), **ψ-ff #9275** (low, last; behaviour tests at the new seams,
  `reassemble_d` and `equilibrium_residual_scaled` deleted, aniso split only if the heuristic-13
  check passes; after χ-ff and η₃).
- ρ #8660 (PRD close) now also depends on η₂, η₃, φ-ff, χ-ff and ψ-ff.

## Retro-critic sitting amendments (2026-10-20)

Ruling R2 (Leo, ruled_by: human, 2026-10-19). Pointer for every bullet:
`plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R2`.
Critique: `~/.claude/review-all/review-all-reify-20261010/retro-critic/S14.json`.

- **B1 (π #8658, ο #8656) → (a).** C9 gets a mechanical guard owned by π: a row in ν's
  `crates/reify-constraints/tests/adoption_ratchets.rs` lexing `super::<child>` /
  `crate::solver::<child>` edges among `src/solver/*.rs`, asserting the D16 order, with a seeded
  core↔floor self-test (§5 C9). The §7.1 INV-candidate-7 sentence and π's "S3 α's Tarjan baseline
  records the smaller SCC set" are struck: `assert-module-scc.py` measures top-level modules only.
  π's files gain `tests/adoption_ratchets.rs` and its checks a path-absent twin on
  `src/solver/tests.rs`; edge `#8658 ← #8645`.
- **B2 (ν #8645 vs solver-legibility-telemetry P4 ε #6722 / μ #6737) → ν first.** Edge
  `#6722 ← #8645`. #6722 builds on ν's `for_each_inequality_leaf`, threads `ConstraintNodeId`
  through `collect_slack_terms` only, calls the reify-constraints export from eval, and drops the
  refuted "cross-crate copy is deliberate" premise (task 4386). #6737 targets ν's
  `robustness_floor_applies` and ο's `solve_core` / `SolveRegime`. §7.1 gains the P4 G4 row; §3.1
  lists #6722 / #6737. ν and #6722 released from the R0 hold.
- **M2 (D20).** Edge `#8645 ← #8799`: ν's ratchet host consumes
  `reify_test_support::adoption_ratchet`.
- **M3 (D15).** Edge `#8645 ← #6655`: D15 restated against the HC4 worker; ν's Ge/Gt row covers
  `src/**` including `hc4.rs` / `interval.rs`; the fold-site row is scoped to `src/`.
- **B3 (ξ₂ #9267) → domain owns the policy.** Supersedes gap-fill K3-D3's placement clause ("sits at
  crate level below `solver/derive`"); the carrier decision A′ stands. `domain.rs` owns `StrictBound`
  (or `Endpoint::Open`) and `default_bounds_for` / `effective_bounds` and imports nothing from
  `crate::solver`; ξ #8653's "move as written" for those items is re-pointed to domain (D16
  amended); ξ₂'s signal gains `assert-module-scc.py check` green with no {domain, solver} SCC.
- **M1.** ρ #8660 gains the post-π re-point step (§9 ρ row). §7.1 gains the G4 row for P2 (#6678,
  #6680, #6677) and P4 (#6722, #6737) on `solver.rs`; §3.1 lists them. Declined: M1 without the
  re-point step.
- **M4 (ε #8621).** The manifest binding "no external reader" is corrected (FAIL→corrected):
  `crates/reify-eval/tests/harness_topology_selector/shell_mixed_region_validation.rs` reads
  `ke.data` / `ke.n_dofs`; ε migrates it to `get()` / `n_dofs()` and the file joins ε's files (§9 ε
  row).
- **M5.** Edges `#9006 ← #9271, #9273, #9274, #7801` (S5 analysis-stratum ξ waits on the gap-fill
  leaves); §7.1: gap-fill leaves edit the form-find trampoline before S5 ξ moves it. Declined: "existing
  variants only". η₂ and φ-ff released from the R0 hold.
- **Polarity.** π: path-absent twin on `crates/reify-constraints/src/solver/tests.rs`; η₂ #9271:
  `non-finite-input-error` anchored on `NonFinite \{ what`; φ-ff #9273: `eigen-calls-leave-solvers`
  anchored on `\.self_adjoint_eigen\(`.
- **κ #8632 (cancelled 2026-10-14, subsumed by S10 β₁ #8935).** D12, C7, BT10, the §7.1
  `GeometryError` row, the §9 κ row and the §10 Q2 ruling clause are marked dead text superseded by
  S10 R5; κ's block is removed from both manifest twins.

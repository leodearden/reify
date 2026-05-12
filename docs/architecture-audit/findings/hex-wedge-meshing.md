# Audit: Hex and Wedge Meshing for Swept Geometries

**PRD path:** `docs/prds/v0_3/hex-wedge-meshing.md`
**Auditor:** audit-hex-wedge-meshing
**Date:** 2026-05-12
**Mechanism count:** 24
**Gap count:** 12 (4 PARTIAL, 3 FICTION, 3 TODO, 1 DRIFT, 1 ORPHAN)

## Top concerns

- **Live realization pipeline does NOT call `dispatch_volume_mesh`.** The 8-case truth-table dispatcher is fully implemented and unit-tested in `engine_build.rs:2403`, but is marked `#[allow(dead_code)]` and `pub(crate)`, with zero non-test callers. The hex/wedge path therefore never executes in production. Sibling fact: `mesh_surface_to_volume_with_diagnostics` (the tet fall-back the dispatcher composes against) is also not yet called from reify-eval — the volume-mesh realization stage is itself still upstream-of-engine. Two latent integrations stack here.
- **`ElasticOptions.force_tet` and `ElasticOptions.require_hex_wedge` are stdlib fields with no Rust reader.** Declared correctly in `crates/reify-compiler/stdlib/solver_elastic.ri:159-160`, validated via `constraint !(force_tet && require_hex_wedge)`, but no `.force_tet` lookup exists in any non-doc Rust code. The escape hatches are runtime-fictional today.
- **PRD-listed `ElasticOptions.sweep_subdivisions` knob is missing entirely from the stdlib struct.** PRD task #7 and task 2988 description both promise `sweep_subdivisions` as the user-facing K override; the underlying Rust helper `derive_layer_count` exists, and its error messages name `sweep_subdivisions`, but the param isn't declared on `ElasticOptions`. DRIFT, not FICTION — the user-facing surface drifted from the PRD during decomposition.
- **Quad-face Neumann BC integrals (PRD task #5 sub-requirement) absent.** `FaceOrder` enum has only `P1Tri`/`P2Tri`. Surface tractions on hex/wedge quad faces have no integration path. Task 2986 (pending) owns this; it has not yet started despite four of its predecessor element/assembly tasks landing in early May.

## Mechanisms

### M-001: `classify_swept_body` Phase A op-stream classifier

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/sweep_classifier.rs:205` (`pub fn classify_swept_body`); unit tests covering Extrude/Revolve/SweepLinear positive cases, multi-profile Loft / LoftGuided / SweepGuided / Pipe / twisted-Sweep negative cases (sweep_classifier.rs:1100+); e2e via `tests/swept_kind_classifier_e2e.rs`; task 2982 done @ commit 3f6cbf9f45.
- **Blocks:** none
- **Note:** Classifier returns `Option<SweptKind>` for the last op of a compiled-op slice; rejects curved-path Sweep, multi-profile Loft, and any non-sweep last op.

### M-002: `SweptKind` discriminated union (Extrude / Revolve / SweepLinear)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `sweep_classifier.rs:73` (`#[non_exhaustive] pub enum SweptKind`); Phase B-ready via `#[non_exhaustive]`; round-trip tests `swept_kind_table_*` in same file.
- **Blocks:** none
- **Note:** Variants match PRD task #1 enumeration. The PRD names a `Loft { profile, path }` variant; impl uses `SweepLinear { profile, path }` (PRD's "single-profile-loft" was renamed during impl). Minor naming drift, semantics identical.

### M-003: `SweptKindTable` per-realization storage + lifecycle

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `sweep_classifier.rs:141`; `Engine::swept_kind_table` field declared at `lib.rs:524`; cleared on every `build()` / `build_snapshot()` / `tessellate_*` (engine_build.rs:536, 790, 1026, 2250); recorded post-classify at engine_build.rs:1904.
- **Blocks:** none
- **Note:** Mirrors `FeatureTagTable` / `TopologyAttributeTable` pattern, last-write-wins, per-build lifecycle.

### M-004: `Engine::swept_kind_table()` public accessor (for GUI/morph consumers)

- **State:** ORPHAN
- **Failure mode:** F4 (producer ready, no consumer wired)
- **Evidence:** `crates/reify-eval/src/engine_admin.rs:223` `pub fn swept_kind_table() -> &SweptKindTable`. No non-test caller exists anywhere in the workspace (`grep -rn` returned nothing in `gui/` or `crates/reify-mesh-morph/`).
- **Blocks:** PRD claim that "Tag persists on the realized body so other systems (mesh morphing, GUI) can read it" is honored at the type level but no downstream system reads it yet.
- **Note:** Future consumers (mesh-morph PRD's swept-mesh preservation claim, GUI "hex/wedge meshed" badge) are stubbed via accessor; both stand to be discovered as FICTION when those PRDs are audited.

### M-005: `swept_kind_to_sweep_params` converter (SweptKind → SweepParams)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `sweep_classifier.rs:302`; used both by tests and by `dispatch_volume_mesh` (engine_build.rs:2436).
- **Blocks:** none
- **Note:** Bridges classifier output into the sweep-step input type; canonical conversion path so engine and tests cannot diverge.

### M-006: P1 hex (8-node) reference element (shape functions, gradients, 2×2×2 Gauss)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/elements/hex_p1.rs` (636 lines); module re-exported via `lib.rs:346`; behavioral pin tests (symmetry, rigid-body null spaces, patch tests). Task 2983 done @ commit 85505d85de.
- **Blocks:** none
- **Note:** Matches PRD task #2.

### M-007: P1 wedge (6-node) reference element (shape functions, gradients, tri×line quadrature)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `elements/wedge_p1.rs` (705 lines); re-export `lib.rs:350`; behavioral pin tests; task 2984 done @ commit 53741f422c.
- **Blocks:** none
- **Note:** Matches PRD task #3.

### M-008: Element-level stiffness assembly for hex (`element_stiffness_hex_p1`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `assembly/hex.rs:28`; shares constitutive integration helper `element_stiffness_generic` with tet/wedge; behavioral test suite via `run_element_stiffness_tests`; task 2985 done @ commit 6def80b56b.
- **Blocks:** none
- **Note:** Matches PRD task #4 (hex half).

### M-009: Element-level stiffness assembly for wedge (`element_stiffness_wedge_p1`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `assembly/wedge.rs:29`; same generic integration helper as hex; behavioral tests; task 2985 done.
- **Blocks:** none
- **Note:** Matches PRD task #4 (wedge half).

### M-010: Mixed-element global assembly (per-element-type dispatch over tet + hex + wedge)

- **State:** PARTIAL
- **Failure mode:** F4 (primitive type-agnostic; integration path unwritten)
- **Evidence:** `assembly/global.rs:33` `AssemblyElement<'a>` carries `connectivity: &[usize]` and `k_e: &ElementStiffness` with per-element DOFs-per-node derived from `k_e.n_dofs / connectivity.len()` — so hex (24 DOFs / 8 nodes = 3), wedge (18/6 = 3), tet (12/4 = 3) all flow through `assemble_global_stiffness` unchanged. However: no caller assembles a mixed slice (no code constructs `AssemblyElement` from `SweptMesh3d` connectivity). Task 2986 still pending.
- **Blocks:** 2986 (pending), 2993 (validation depends on 2986)
- **Note:** Architectural primitive is element-type-agnostic by design; "mixed-element assembly" gap is purely on the *assembler-input-construction* side, not the scatter loop.

### M-011: Quad-face Neumann BC integrals (hex/wedge surface tractions)

- **State:** FICTION
- **Failure mode:** F1 (PRD task #5 sub-requirement; no code)
- **Evidence:** `crates/reify-solver-elastic/src/boundary/neumann.rs:37` `pub enum FaceOrder { P1Tri, P2Tri }` — no Quad arm; `apply_traction_load` at neumann.rs:478 dispatches solely on tri orders; task 2986 (which owns this) is pending.
- **Blocks:** 2986 (pending); transitively 2993 (validation)
- **Note:** PRD task #5 sub-bullet 2 explicitly calls for "2D quadrature on the quad (2×2 Gauss)". Hex/wedge solves with surface tractions on quad faces are not yet expressible.

### M-012: 2D cross-section extraction + Gmsh-2D meshing (triangle or recombined-quad output)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-gmsh/src/mesh_profile_2d.rs`; `crates/reify-solver-elastic/src/mesher.rs` orchestrator `mesh_swept_profile_2d`; `Mesh2d`, `Mesh2dError`, `Mesh2dReport`, `SweepElementTarget`, `recombine_quality_ok` re-exported at `solver-elastic/lib.rs:378-381`; integration tests `tests/mesh_swept_profile_2d_tests.rs`; task 2987 done @ commit c247c71ad7.
- **Blocks:** none
- **Note:** Matches PRD task #6; hex-preferred / wedge-fallback discriminated via `SweepElementTarget` + `recombine_quality_ok` check.

### M-013: Sweep step `sweep_2d_mesh_to_3d` (2D mesh × K layers → 3D hex/wedge connectivity)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/sweep.rs:363` `pub fn sweep_2d_mesh_to_3d`; `SweptMesh3d` / `SweptConnectivity::{Hex,Wedge}` (sweep.rs:184); Extrude / Revolve / SweepLinear arms; SweepLinear==Extrude byte-identity test (sweep.rs:717+); task 2988 done @ commit 1b8242ad7f.
- **Blocks:** none
- **Note:** Matches PRD task #7. ~1080 lines, comprehensive test coverage.

### M-014: `derive_layer_count` (mesh_size-derived K with min-2 through-thickness)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `sweep.rs:78` `pub fn derive_layer_count`; clamp tests for pathological inputs; re-export at lib.rs:389.
- **Blocks:** see M-015 (the user-facing knob it would be driven by).
- **Note:** Pure helper; safe-by-default behavior under degenerate inputs.

### M-015: `ElasticOptions.sweep_subdivisions` explicit K override

- **State:** DRIFT
- **Failure mode:** F5 (PRD knob never declared on the user-facing struct)
- **Evidence:** PRD task #7 §"Sweep step": "K layers controlled by `ElasticOptions.mesh_size` derivation [...] or explicit `sweep_subdivisions` override"; task 2988 description repeats this. The Rust diagnostic message in `sweep.rs:99-120` names `sweep_subdivisions` as a knob, but `crates/reify-compiler/stdlib/solver_elastic.ri:150-160` contains NO `sweep_subdivisions` param. `grep` returns no occurrences in `.ri` files. The override knob is unreachable from user code.
- **Blocks:** none (gap is silent; mesh_size-derived K still works)
- **Note:** A minor user-facing escape hatch that was specified but lost during decomposition. Easy fix; PRD-vs-stdlib audit value is the surprise.

### M-016: Through-thickness check (warn when K < 2)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `sweep.rs::check_sweep_through_thickness` + `ThroughThicknessSweepWarning` re-export (lib.rs:389); test `w.message.contains("mesh_size")` and `w.message.contains("sweep_subdivisions")` at sweep.rs:896.
- **Blocks:** none (warning text references the missing M-015 knob)
- **Note:** Diagnostic-only; the warning message refers to a `sweep_subdivisions` knob that doesn't exist on `ElasticOptions` (see M-015) — minor cosmetic inconsistency.

### M-017: `dispatch_volume_mesh` 8-case truth-table dispatcher

- **State:** PARTIAL
- **Failure mode:** F6 (function fully implemented + unit-tested; **no live caller**)
- **Evidence:** `crates/reify-eval/src/engine_build.rs:2403` `pub(crate) fn dispatch_volume_mesh` with `#[allow(dead_code)]`; `VolumeMeshOutcome` (engine_build.rs:2353) also `#[allow(dead_code)]`; unit tests `dispatch_volume_mesh_tests` at engine_build.rs:4652-4915 cover all 8 truth-table cases via mock closures. `grep` shows zero non-test callers across the workspace. Task 2989 done @ commit 305c96f1856f81dffc — but the "done" was for *defining* the dispatcher, not threading it into a live realization stage.
- **Blocks:** All downstream hex/wedge runtime behavior. 2991 (P2 diagnostic), 2992 (fall-back diagnostics), 2993 (validation), 2994 (synthetic fixtures — task done but tested only at the pipeline-component layer, not via dispatcher).
- **Note:** This is the hex/wedge analogue of GR-001: substantial apparatus, no actual runtime activation. The hex_wedge_quality test file (header comment) explicitly notes "It does NOT import the classifier [...] or the dispatcher" — testing exists at each layer but the integration is not exercised end-to-end through any live realization.

### M-018: `mesh_surface_to_volume_with_diagnostics` tet fall-back integration

- **State:** PARTIAL
- **Failure mode:** F4 (kernel-side fn implemented; engine-side caller absent)
- **Evidence:** `crates/reify-kernel-gmsh/src/mesh_volume.rs:161` `pub fn mesh_surface_to_volume_with_diagnostics`; `grep` shows no `mesh_surface_to_volume` call from `reify-eval`; the tet fall-back closure in `dispatch_volume_mesh` is a generic `FnOnce() -> Result<VolumeMesh, GeometryError>` — not yet bound to any concrete tet-mesh producer in the realization stage.
- **Blocks:** Hex/wedge fall-back path (and tet-default path that PRD compares against).
- **Note:** Discovered transitively. PRD assumes the tet path is already present from FEA PRD task #17 (task 2925, done); the kernel-side function exists but the eval-side wiring is not yet present. Likely an FEA-PRD-shaped finding bleeding into this PRD's gap surface; flagged here for completeness, owned by FEA PRD.

### M-019: `ElasticOptions.force_tet` runtime readout (drives dispatcher short-circuit)

- **State:** FICTION
- **Failure mode:** F1 (compile-time-only field; no Rust reader)
- **Evidence:** stdlib declaration at `crates/reify-compiler/stdlib/solver_elastic.ri:159` (`param force_tet : Bool = false`); compile-time tests pin defaults + the constraint `!(force_tet && require_hex_wedge)` in `crates/reify-compiler/tests/solver_elastic_tests.rs:702-770`. `grep -rn "\.force_tet"` in `/crates/` returns ONLY documentation strings — no actual `if options.force_tet` site exists in the workspace. `dispatch_volume_mesh` takes `force_tet: bool` as a parameter but no production caller binds it from `ElasticOptions`. Task 2990 (done) wired the stdlib field; the live consumer was implicit in task 2989 but is absent.
- **Blocks:** 2991 (in-progress), 2992 (pending) — both need this lookup to dispatch their diagnostics.
- **Note:** Direct analogue to GR-001 ("declared but never read"). Distinct because here the field is `Bool`, not a struct constructor — but the failure shape (PRD assumes runtime evaluation backing for a compile-time declaration) is structurally identical.

### M-020: `ElasticOptions.require_hex_wedge` runtime readout (turns fall-back into error)

- **State:** FICTION
- **Failure mode:** F1 (compile-time-only field; no Rust reader)
- **Evidence:** stdlib declaration at `solver_elastic.ri:160`; compile-time defaults/constraint tests parallel to force_tet; `grep -rn "require_hex_wedge"` shows only PRD-doc + stdlib-test references in Rust code, no `if options.require_hex_wedge` site. `dispatch_volume_mesh` takes it as a parameter but no production caller binds it from `ElasticOptions`.
- **Blocks:** 2991 (in-progress), 2992 (pending).
- **Note:** Symmetric to M-019.

### M-021: P2-element-order interaction diagnostic ("P1 hex used despite element_order=P2")

- **State:** TODO
- **Failure mode:** F1 (PRD-mandated one-shot info diagnostic; task in-progress)
- **Evidence:** Task 2991 status=`in-progress` per fused-memory; no `hex_wedge_p2_substitution` or equivalent diagnostic ID present in `crates/reify-types/src/diagnostics.rs` or solver-elastic. Test header at `hex_wedge_quality.rs:1-37` mentions classifier + dispatcher tests live elsewhere; no validation of the diagnostic path here either.
- **Blocks:** Honest documentation of the auto-promotion contract.
- **Note:** Blocked transitively by M-017/M-019/M-020 (the diagnostic must be emitted from inside the dispatcher's swept-path arm, which currently has no live caller).

### M-022: Fall-back diagnostic mapping (distinct IDs for each fall-back cause)

- **State:** TODO
- **Failure mode:** F1 (PRD task #11; task 2992 pending)
- **Evidence:** Task 2992 status=`pending`; PRD enumerates `hex_wedge_phase_a_finishing_ops` / `hex_wedge_invalid_sweep_geometry` / `hex_wedge_2d_mesh_failure` / `hex_wedge_force_tet` / `hex_wedge_promoted` diagnostic IDs; `grep` returns zero matches for any of these IDs in `crates/`. The `dispatch_volume_mesh` truth table produces only `GeometryError::OperationFailed(String)` strings — no distinct diagnostic IDs surfaced via the standard diagnostic stream.
- **Blocks:** 2993 (validation suite asserts on these IDs).
- **Note:** Required for the "transparent optimization" PRD failure-mode policy to be observable.

### M-023: Validation suite extension (cantilever + thick-walled cylinder vs analytical)

- **State:** TODO
- **Failure mode:** F1 (PRD task #12; task 2993 pending)
- **Evidence:** Task 2993 status=`pending`; no `hex_wedge_validation.rs` test file (`find` confirms only `hex_wedge_quality.rs` + `mesh_swept_profile_2d_tests.rs` + `shell_benchmarks.rs` in solver-elastic tests/). PRD task #12 specifies regression assertions on (a) analytical agreement and (b) "convergence-vs-DOF steeper than tet" — neither exists.
- **Blocks:** Final confidence that hex/wedge is doing real work beyond "doesn't crash".
- **Note:** Cleanly blocked by M-010/M-011/M-017/M-019/M-020/M-022 — until the runtime path executes end-to-end with diagnostics, the regression suite has nothing to instrument.

### M-024: Cache-key composition (no new `ReprKind` variant; geometry hash + force_tet drives element type)

- **State:** PARTIAL
- **Failure mode:** F2 (claim is plausible but contingent on M-019 wiring; not yet exercised)
- **Evidence:** PRD §"Cache-key handling" promises element-type composition is implicit in the geometry hash + `force_tet`; `crates/reify-kernel-gmsh/src/cache_key.rs:45` `volume_mesh_cache_key` does not currently incorporate `force_tet` (search returned 0 matches). `crates/reify-eval/src/compute_cache_key.rs` includes options-hash but the actual field set hashed is not auditable from a quick read — defer to M-019: until `force_tet` is read at runtime, the cache key's correctness w.r.t. hex/wedge vs tet of the same body cannot be empirically validated.
- **Blocks:** Cache correctness once M-017/M-019 light up.
- **Note:** Flagged because the PRD's "same cache key works for both element types" claim is a load-bearing simplification — it deserves an explicit test once the live path activates.

## Cross-PRD breadcrumbs

- **`structural-analysis-fea`** owns the *non-swept* `mesh_surface_to_volume_with_diagnostics` realization wiring (M-018). The hex/wedge PRD assumes this is already in place; if the FEA-PRD audit finds the tet-realization wiring is itself incomplete, M-017 and M-018 share a single upstream gap.
- **`mesh-morphing`** is the named downstream consumer of `Engine::swept_kind_table()` (M-004). If that PRD's audit confirms it relies on swept-mesh preservation as a "load-bearing benefit" but has no `swept_kind_table()` call site, the orphan-accessor pattern recurs across the v0.3 PRD set.
- **`fea-gui-rendering` (v0.3)** is the named consumer of the "hex/wedge meshed" status badge. Same orphan-accessor pattern likely surfaces there.
- **`compute-node-infrastructure`** is implicitly upstream: any `@optimized("solver::elastic_static")` wiring will need to read `force_tet`/`require_hex_wedge` (M-019/M-020) from the `ElasticOptions` value at ComputeNode-dispatch time. If GR-001 (struct-constructor runtime evaluation) is unfixed, the `ElasticOptions(force_tet: true)` constructor literal cannot produce a runtime value to read in the first place — M-019/M-020 may be doubly-blocked.
- **`a-posteriori-error-estimation` (v0.4)** treats refinement decisions as element-type-aware in spirit; once M-017 lights up, mixed hex/wedge/tet meshes need to be flowed into the Z-Z indicator path. Not blocking, but a cross-cutting integration that's invisible today.

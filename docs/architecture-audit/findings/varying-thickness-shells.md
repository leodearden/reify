# Audit: Varying-Thickness Shell Elements

**PRD path:** `docs/prds/v0_5/varying-thickness-shells.md`
**Auditor:** audit-varying-thickness-shells
**Date:** 2026-05-12
**Mechanism count:** 12
**Gap count:** 12

## Top concerns

- Every mechanism this PRD relies on is **FICTION or TODO** — the PRD assumes a fully-shipped v0.4 constant-thickness foundation that has not landed. v0.4 shells PRD is "design resolved + decomposed (2026-05-05) — deferred"; tasks T1–T23 (extraction → kernel → integration → stdlib → engine) are not done. Specifically, `reify-shell-extract` is **not** a dependency of `reify-solver-elastic` (verified via Cargo.toml dependents) — the end-to-end shell pipeline is not wired.
- The PRD's central syntax — `@shell(thickness = linear_taper(root = 5 mm, tip = 1 mm, axis = ...))` — is **structurally incompatible** with the current annotation grammar. `reify_types::AnnotationArg` is a closed enum `{String, Int, Real, Bool, Ident}` — no expression / function-call / Field variant. The compiler explicitly errors on non-numeric thickness (`annotations.rs:160-176`). Lifting this is a non-trivial language-surface change (annotation args becoming runtime expressions, evaluation order against compile vs realize phases).
- The PRD presupposes a stdlib "thickness field producer" library (`linear_taper`, `radial_thickening`, `imported_thickness_map`). **None of these exist** in `crates/reify-compiler/stdlib/*.ri`. The PRD calls them "small additions, can be defined alongside" but they require `Field<Point3, Length>`-typed producer functions evaluable at mid-surface points — a non-trivial intersection with composed-field eval and unit checking.
- One latent win: per-vertex thickness **is already preserved** through `MidSurfaceMesh.thickness: Vec<f64>` and through the dedup mesher (`mesher.rs:325-385` averages thickness on vertex merge). The PRD's claim "voxel-medial extraction already produces per-vertex thickness as a byproduct" is correct and intact. The lossy step ("v0.4 collapses it to a per-body scalar") happens at a layer that doesn't exist yet — so v0.5 may not need to "preserve the field", it just needs to never collapse.

## Mechanisms

### M-001: v0.4 constant-thickness shells path shipped (precondition)

- **State:** TODO
- **Failure mode:** F1 (foundation unshipped)
- **Evidence:** `docs/prds/v0_4/structural-analysis-shells.md` status line "design resolved + decomposed (2026-05-05) — deferred"; v0.4 shells decomposition tasks T1-T23 not done (would need orchestrator query to enumerate precise IDs); `reify-shell-extract` crate exists with extraction + segmentation + mesher + mid-surface-naming but `reify-solver-elastic` does not depend on it (verified: `grep -r reify-shell-extract crates/*/Cargo.toml` returns only the crate's own Cargo.toml).
- **Blocks:** entire varying-thickness PRD
- **Note:** PRD explicitly gates on v0.4 shipping (§"Pre-conditions for activating"). v0.4 has substantial individual pieces wired (MITC3+ stiffness, MPC plumbing, voxel-medial extraction, mid-surface mesher, segmentation, mid-surface naming, Z-Z indicator) but no engine integration glue between extraction and solver-elastic.

### M-002: Per-vertex thickness preserved through extraction pipeline

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-shell-extract/src/mid_surface.rs:35-45` (`MidSurfaceMesh.thickness: Vec<f64>` field), `mid_surface.rs:670-680` (`thickness.push(2.0 * phi.abs())` per vertex), `mid_surface.rs:1081-1110` (test `extract_mid_surface_per_vertex_thickness_matches_slab_full_thickness` validates accuracy on slab fixture), `crates/reify-shell-extract/src/mesher.rs:325-385` (`dedup_vertices` averages thickness across merged duplicates, preserving `thickness.len() == vertices.len()`).
- **Blocks:** none — this is the only wired mechanism in the PRD's pipeline.
- **Note:** The PRD's central data-path claim "v0.4 collapses it to a per-body scalar; v0.5 preserves the field" is half-right. The mid-surface extraction layer preserves it today; the (unshipped) integration layer between extractor and solver is where collapse would happen.

### M-003: Element-kernel through-thickness integration reads thickness at the Gauss point

- **State:** FICTION
- **Failure mode:** F2 (kernel API assumes constant scalar)
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs:194-203` (`shell_element_stiffness(nodes: &[[f64;3];3], thickness: f64, material: &IsotropicElastic)` — thickness is a scalar by signature, asserted `> 0.0`); `shell_assembly.rs:212` (`let t = thickness;` used in closed-form analytical integration `t * d_pl`, `t³/12 * d_pl`, `κ·G·t` — single-Gauss, integrand-constant formulation per docstring "1-point rule, integrand constant"); `shell_assembly.rs:118-120` `KAPPA = 5.0/6.0` baked in for the Reissner-Mindlin through-thickness shape function.
- **Blocks:** any per-vertex / per-Gauss thickness flow into the element stiffness; the membrane/bending/shear closed forms (`t·D`, `t³/12·D`, `κGt`) all use a single scalar that must be replaced with an actual through-thickness integration loop.
- **Note:** v0.4 deliberately chose 1-point in-plane × analytical-through-thickness. Varying thickness collapses the analytical integration option: you'd need numerical through-thickness Gauss (or a 3-Gauss in-plane rule that samples interpolated thickness). This is a kernel rewrite, not a parameter change. PRD's "kernel modification" framing is correct.

### M-004: Stress recovery samples local thickness at query points

- **State:** FICTION
- **Failure mode:** F2 (recovery API assumes single thickness)
- **Evidence:** `crates/reify-solver-elastic/src/shell_result.rs:283` (`use crate::shell_assembly::shell_element_stiffness;`); `shell_result.rs:660-680` (`shell_element_stiffness(&UNIT_TRI, t, &mat)` uses scalar `t`); top/mid/bottom stress channel design (v0.4 PRD §Stress through thickness) was specified for constant `t` — the bending/membrane decomposition uses `t/2` as the surface offset. With varying `t`, the offset is per-vertex (or interpolated to query point) and the local recovery formula has additional in-plane bending terms from the thickness gradient (PRD §"Result interpretation" — "bending-stiffness gradient introduces extra in-plane bending modes").
- **Blocks:** any varying-thickness stress recovery; potentially also v0.4 if naive recovery silently produces wrong stress on varying-thickness inputs.
- **Note:** PRD calls out the physics correctly (gradient introduces extra terms). The implementation doesn't exist; not even a stub.

### M-005: `@shell(thickness = ...)` annotation parses keyword-style thickness expression

- **State:** FICTION
- **Failure mode:** F1 (annotation grammar restricts args to compile-time literals)
- **Evidence:** `crates/reify-types/src/annotation.rs:77-83` (`AnnotationArg` enum: `String | Int | Real | Bool | Ident` — no Expr, no FunctionCall, no Field, no keyword/named-arg variant); `crates/reify-compiler/src/annotations.rs:158-176` (`@shell` accepts only `[]` or `[Int|Real]` positional thickness; non-numeric warns "thickness argument must be a numeric literal"); `crates/reify-syntax/src/lib.rs:1043-1053` (parser-side `Annotation.args: Vec<Expr>` — full expressions parsed but lowered to `AnnotationArg` literals only). No named-field annotation arg syntax in `ts_parser.rs`.
- **Blocks:** `@shell(thickness = linear_taper(...))`, `@shell(thickness = 2 mm)` with units (Real-only — no quantity literal variant), every "Annotated field" mode in PRD §"User specification surface".
- **Note:** PRD's syntax `@shell(thickness = linear_taper(...))` requires three new things at once: (a) keyword/named annotation args, (b) function-call/Expr annotation args, (c) evaluation timing semantics (annotations are compile-time; field producers may be runtime). Closest sibling is the structure-ctor gap GR-001 — both are "compile-time vs runtime evaluation of named references".

### M-006: Stdlib `linear_taper` / `radial_thickening` / `imported_thickness_map` field producers exist

- **State:** FICTION
- **Failure mode:** F3 (stdlib library named but absent)
- **Evidence:** No matches for any of `linear_taper`, `radial_thickening`, `imported_thickness_map`, `thickness_field` in `crates/reify-compiler/stdlib/*.ri` (verified via grep across `.ri` and `.rs` corpus). Existing stdlib `Field<>`-aware fns are limited (e.g. `fea_multi_case.ri:203` mentions `Field<Point3, T>` in a docstring for `worst_case` reductions, but no producer fns).
- **Blocks:** every "Annotated field" example in PRD §"User specification surface" and §"Sketch of approach"; "small additions" claim in §"Pre-conditions for activating" is misleading — these need typed producer fns evaluated against mid-surface points.
- **Note:** Composes with `imported-field-source-hdf5-csv.md` for `imported_thickness_map`. Probably belongs as a separate stdlib task (or sub-PRD) rather than "filed alongside".

### M-007: `@shell` with bare-form derives thickness from medial extraction (auto path)

- **State:** TODO
- **Failure mode:** F4 (auto-classification dispatcher unshipped — declared TODO in code)
- **Evidence:** `crates/reify-compiler/src/annotations.rs:154-159` (comment: "positional thickness arg; when omitted, T18's auto-classification dispatcher is expected to derive thickness from medial-axis analysis (not yet implemented). `[] => {}` — bare @shell — defer thickness to medial analysis"); v0.4 T18 (auto-classification dispatch) is part of the v0.4 deferred decomposition.
- **Blocks:** PRD's "Auto (default)" mode (§"User specification surface"); also v0.4's auto path.
- **Note:** This is fundamentally a v0.4 gap that this PRD inherits.

### M-008: Mesher coupling — refines where thickness changes rapidly relative to element size

- **State:** FICTION
- **Failure mode:** F5 (refinement strategy mentioned, no infrastructure)
- **Evidence:** No matches for "thickness gradient" refinement logic in `crates/reify-shell-extract/`. Mesher accepts a static `MesherOptions` (merge tolerance, aspect ratio gates, angle gates, smoothing iterations); no adaptive size-field input or per-vertex target-size driven by extracted field. `mesher.rs:74-85` notes "Maximum Laplacian smoothing iterations on quality failure" but smoothing implementation is "not yet shipped" (v0.4 deferred). `crates/reify-solver-elastic/src/error_estimator.rs` (Z-Z indicator) drives a tet-only refinement loop today.
- **Blocks:** PRD §"Mesher coupling" claim "same logic as for stress concentrations but driven by thickness gradient" — the stress-concentration-driven adaptive remesh for shells doesn't exist either.
- **Note:** Composes with `a-posteriori-error-estimation.md` indicator path and `mesh-morphing.md` warm-start preservation.

### M-009: Element-node-interpolated thickness at integration points

- **State:** FICTION
- **Failure mode:** F2 (lowering decision unmade; element kernel APIs are scalar)
- **Evidence:** PRD §"Open design questions" — "Per-element vs. per-Gauss-point thickness ... Lean per-Gauss-point". Currently `shell_element_stiffness` signature accepts only `thickness: f64`; no per-node thickness vector; no thickness-aware shape-function evaluation. `crates/reify-solver-elastic/src/shell_kinematics.rs` produces in-plane shape gradients only; no through-thickness shape function consumes thickness as a Field-of-(ξ,η).
- **Blocks:** accuracy vs simplicity decision is unresolved; either path requires a new kernel API.
- **Note:** Decision is design-open per PRD. Code does not constrain it yet.

### M-010: Stepped-thickness discontinuity handling (mesh-refinement vs MPC-tied regions)

- **State:** FICTION
- **Failure mode:** F4 (design-open)
- **Evidence:** PRD §"Open design questions" — "Continuous-vs-discontinuous thickness fields... Either model the transition with mesh refinement + linear interpolation across the step, or treat it as two separate shell regions tied with MPCs". MPC plumbing exists (`crates/reify-solver-elastic/src/mpc.rs`); shell-region segmentation exists (`crates/reify-shell-extract/src/segmentation.rs`); but the MPC-as-thickness-step mechanism is purely conceptual — no segmentation strategy classifies by `|∇thickness|` discontinuity; no MPC builder for shell/shell same-region-different-thickness tying.
- **Blocks:** physically-discontinuous thickness designs (flange-to-web is the common case).
- **Note:** This is design-open in the PRD. Flagging as a gap because the implementation pathway will have to choose.

### M-011: Varying-thickness validation benchmarks

- **State:** FICTION
- **Failure mode:** F6 (acknowledged gap — no canonical reference solutions exist)
- **Evidence:** PRD §"Open design questions" — "the standard shell benchmarks (pinched cylinder, Scordelis-Lo) assume constant thickness. Need to identify or construct varying-thickness reference solutions." Existing `crates/reify-solver-elastic/tests/shell_benchmarks.rs` covers constant-thickness benchmarks only.
- **Blocks:** validation suite for varying-thickness shells.
- **Note:** Acknowledged as design-open. No work product exists; this is research not implementation.

### M-012: Mid-surface compose with `mesh-morphing.md` warm-start preservation under thickness change

- **State:** FICTION (cross-PRD breadcrumb)
- **Failure mode:** F1 (composing-PRD itself deferred)
- **Evidence:** `docs/prds/v0_3/mesh-morphing.md` is deferred (per `MEMORY.md` index entry `project_v03_mesh_morph_prd_resolved.md`, tasks 2938-2953 deferred). Warm-start exists for parameter-driven CG/optimization (`crates/reify-solver-elastic/src/warm_state.rs`, `crates/reify-constraints/tests/solver_integration.rs`) but not for shell-mesh-morphing with thickness field.
- **Blocks:** PRD §"Composes with mesh-morphing.md" claim ("thickness fields morph alongside geometry under parameter changes; warm-start preservation works the same way").
- **Note:** This is properly a cross-PRD compose, not a self-contained gap. Breadcrumb only.

## Cross-PRD breadcrumbs

- `composite-laminated-shells.md` (v0.5+) — composes per PRD §"Out of scope" — varying total thickness × variable ply count = union of both.
- `mesh-morphing.md` (v0.3, deferred) — M-012 above.
- `fea-gui-rendering-shells.md` (v0.4) — thickness-display mode for varying-thickness rendering; the GUI doesn't yet render shells at all.
- `imported-field-source-hdf5-csv.md` (separate audit found in `findings/`) — needed for `imported_thickness_map` field producer (M-006).
- `a-posteriori-error-estimation.md` — Z-Z indicator + refinement strategy interacts with M-008 thickness-gradient refinement.
- `persistent-naming-v2.md` — mid-surface entity naming exists in `crates/reify-shell-extract/src/mid_surface_naming.rs` (composes with v0.4 T20; varying-thickness adds no new naming demand).
- **GR-001** transitively relevant — `linear_taper(...)` produces a `Field<...>`-typed value, which is a different mechanism from structure-ctor evaluation but shares the "compile-time annotation arg must reference a runtime-evaluable construct" failure pattern.

## Skipped: none

This PRD is a v0.5+ stub but is **not** purely process — it makes concrete claims about data path, element kernel, annotation syntax, and stdlib library, all of which are inventoryable.

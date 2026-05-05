# PRD: Shell Elements for Thin-Body Structural Analysis

Status: design resolved + decomposed (2026-05-05) — deferred, candidate v0.4. Sibling to v0.3 linear-elastostatic FEA. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Add 2D shell elements to Reify's structural-analysis stack so that thin bodies — flexures, sheet metal, panels, casings — solve accurately and cheaply. Tet-only FEA underperforms badly here; shells are the conventional answer in commercial CAD-FEA. The defining UX commitment for v0.4 shells: **the user does not annotate thin features**. Reify auto-detects, extracts the mid-surface, picks the right element formulation, and reports through-thickness stress correctly — including for bodies buried in libraries whose thinness depends on parameters resolved at evaluation time.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships P1/P2 tetrahedral elements only. This is fine for blocky bodies but breaks down for thin features:

- **Shear locking** — linear (P1) tets exhibit severe shear locking in bending, often underestimating thin-body deflection by 30–50%. P2 tets reduce locking substantially but don't eliminate it; even P2 underperforms shells by 2–5× on thin features.
- **Element-count explosion** — maintaining sane aspect ratio (<10:1) through a 1mm flexure in a 100mm part demands many tets across thickness, which cascades to many tets longitudinally. A single flexure can drive element counts to 50K–200K when the underlying physics needs ~500 DOFs.
- **Stress concentrations at flexure necks** — tet meshes need local refinement that nobody asks for explicitly.

The v0.3 PRD acknowledges this gap and adds a thin-body diagnostic warning that points at this PRD as the eventual fix.

Shell elements are mature in commercial FEA: Abaqus S3R, Ansys SHELL181, MFEM RT_TraceFiniteElement, etc. Reify's handwritten faer-rs solver path makes adding shells tractable — element kernel is the new work, the linear-algebra layer is reused.

## Why deferred to v0.4

- Needs **v0.3 linear-static FEA shipped** as foundation — shares solver kernel, BCs, materials, options. Building shells before tets is backwards.
- Needs **v0.2 OpenVDB voxel kernel** as foundation for voxel-medial mid-surface extraction (see Resolved design decisions). Building shells before the dispatcher exposes a voxel-realization path means hand-rolling either an OCCT offset path (brittle) or a separate voxelizer (duplication).
- **Mixed shell/tet auto-segmentation** is the dominant motivator for shells in a parameter-driven design language, and it requires the medial extractor to be field-quality, not just thin-sheet-quality. v0.4 timing lets the v0.2 OpenVDB integration mature on field-level use cases first.

## Sketch of approach

Five logically separable pieces:

1. **Voxel/medial mid-surface extraction.** Realize the body as `ReprKind::Voxel` via the v0.2 multi-kernel dispatcher (sparse narrow-band OpenVDB). Compute a medial-surface mask: voxels where two opposing nearest-surface points lie at approximately equal distance. Extract iso-surface of mask → triangle mid-surface mesh; per-vertex thickness = 2 × SDF at corresponding voxel (free byproduct).
2. **Per-region auto-classification.** Per body, the medial mask either (a) covers the whole body with consistent thickness/extent ratio → shell, (b) is empty / thickness/extent ratio too high → tet, (c) covers part of the body → mixed shell + tet. Classification determines the meshing and element-kernel route. No user annotation required for the common case.
3. **Shell element formulation.** MITC3+ Reissner-Mindlin triangle elements (3-node, 6 DOFs/node, mixed-interpolation strain to eliminate transverse-shear locking). P1 only for v0.4; P2 deferred unless demand surfaces. Reuses isotropic constitutive law from v0.3.
4. **Shell/tet coupling.** MPC (multi-point constraint) tying at interfaces. Three tying points across thickness on the tet side; constrain shell rotational DOFs to displacement gradients on the tet side. Reuses the v0.3 row-elimination machinery built for Dirichlet BCs.
5. **Annotation surface for explicit overrides.** `@shell(thickness = ...)` to force shell treatment with optional thickness override (extracted otherwise); `@solid` to force tet treatment. Both validate against medial extraction and error when the geometry is incompatible.

User-visible result type: `ElasticResult.stress` becomes a structured field with `top`, `mid`, `bottom` channels for shell elements (tet elements use `mid` only with `top`/`bottom` set equal to `mid`). Stress tensors are reported in a local frame attached to each mid-surface element, with a `frame` field for transformation to global. GUI rendering of these additions is tracked in `fea-gui-rendering-shells.md`.

## Pre-conditions for activating

- v0.3 linear-static FEA shipped (kernel, BCs, materials, mesher, validation suite).
- v0.2 multi-kernel work has shipped the OpenVDB integration: `ReprKind::Voxel` realizable from B-rep via the dispatcher's conversion chain at resolutions sufficient for shell-mesh-coarse use (≈ thickness/3 voxel size for the thinnest expected feature).
- Topology selectors mature enough to express face-tagged thicknesses and to address mid-surface entities (composes with persistent-naming-v2's derived-geometry naming sub-vocabulary).
- Concrete user demand for thin-body FEA (flexures, sheet metal designs in active use).

## Resolved design decisions (2026-05-05)

**Mid-surface extraction: voxel/medial via OpenVDB.** Single approach, no tiered fallback. Comparison considered:

| Candidate                  | Robustness                | Thickness output             | Build cost                          | Mixed-region support |
|----------------------------|---------------------------|------------------------------|-------------------------------------|----------------------|
| OCCT `BRepOffsetAPI`       | Brittle on T-junctions, fillets, edges | Parametric (offset surface)  | Already available (v0.1 OCCT)       | No                   |
| Manifold mesh medial       | Reasonable                | Per-vertex from mesh distance | Available (v0.2 Manifold)           | Possible, awkward    |
| **OpenVDB voxel/medial**   | **Robust on arbitrary geometry, including topology change under params** | **Per-vertex from SDF (free)** | **Available (v0.2 OpenVDB)**        | **Falls out for free** |

Cost picture: OpenVDB sparse narrow-band only allocates voxels near the surface (~10⁷ active voxels for a 1m part with 0.1mm voxels and 0.1m² of thin-sheet surface — orders of magnitude smaller than dense or uniform-octree). Resolution defaults to thickness/3 for the thinnest expected feature; user-overridable via `ElasticOptions.shell_voxel_size`.

**Element formulation: MITC3+ triangles, P1 only.** Reasons: (a) MITC3+ valid range (L/t > ~3) overlaps tet valid range (L/t < ~10) so there's no error-gap in the auto-classification dichotomy; (b) DKT's silent inaccuracy in the L/t = 5–20 marginal-thickness regime is a credibility-killing bug class, not worth the simpler kernel; (c) quad shells deferred because mid-surface quad meshing is uneven on curvy mid-surfaces and no v0.4 use case demands it.

**Auto-classification by default. No annotation required for the common case.** The trichotomy (definitely-shell, definitely-tet, ambiguous error) collapses to a dichotomy via formulation-range overlap. Per-body procedure:

- Run voxel/medial extraction.
- If medial mask covers whole body with thickness/extent ratio < `shell_threshold` (default 0.2, equivalent to L/t > 5): shell.
- If medial mask is empty or thickness/extent ratio > `shell_threshold` everywhere: tet.
- Otherwise (mask covers part of the body): mixed shell + tet, auto-segmented per region.

The "ambiguous middle" error case essentially never fires in default operation. The single residual error case is "user explicitly `@shell`'d a body with too-high thickness/extent ratio anywhere," detected free during extraction and reported with diagnostic.

**Mixed shell/tet bodies are first-class, not a v0.4.x follow-on.** Auto-segmentation per region is the dominant motivator: a flexure attached to a solid block in a parameter-driven library is exactly the case the user cannot annotate manually. Per-region segmentation: medial mask connectivity → "shell-eligible regions" (where mask exists with consistent thickness/extent ratio) vs "tet regions" (where mask is absent or ratio too high). Each region meshes with the appropriate kernel, MPC tying at interfaces.

**MPC coupling at shell/tet junctions.** Three tying points across thickness on the tet side; shell rotational DOFs constrained to displacement gradients of tet displacement field. Implementation reuses the v0.3 row-elimination plumbing — same code path as Dirichlet BCs, no new linear-algebra surface.

**Stress through thickness: top/mid/bottom fields.** Bending stress (the dominant stress on flexures) lives on top and bottom surfaces, not the mid-surface. `ElasticResult.stress` extends to a structured field with `top`, `mid`, `bottom` channels. Tet results populate all three with the same value (no through-thickness variation to report); shell results populate them per the through-thickness integration. Backward-compatible: code that reads `result.stress` (now equivalent to `result.stress.mid`) continues to work for tet-dominated workloads.

**Stress frame: local (in-plane / out-of-plane), with frame field for global transformation.** Returning a global tensor would lose the membrane-vs-bending decomposition that makes shell results interpretable. The `frame` field on shell results gives per-element local-to-global rotation; stdlib helper `to_global(stress, frame)` available when global tensor is wanted.

**BC framework: rotation auto-clamp on `FixedSupport`, explicit `PinnedSupport` for opt-out.** A shell `FixedSupport` that doesn't clamp rotations is a "pin" not a "clamp" — the difference is a factor of 4× in tip deflection, and the silent-pin bug would mislead designers. Default: `FixedSupport` on a shell auto-clamps rotational DOFs. New `PinnedSupport` stdlib type for the explicit-pin case (free rotation, displacement-only constraint). Tet `FixedSupport` semantics unchanged.

**Failure semantics: hard error on explicit `@shell`, fallback-with-diagnostic on auto.** When extraction fails or yields too-thick geometry on a `@shell`-annotated body, it's a model error and the user wants to know now. When extraction fails on auto-classification, it's a hint that the body is genuinely solid; fall back to tet meshing with a diagnostic so the user can verify if surprising. Failure causes mapped to specific diagnostics (no medial mask, inconsistent thickness, too-thick anywhere, multi-thickness step exceeds threshold, etc.).

**Persistent-naming for derived mid-surfaces.** Mid-surface faces and edges are derived geometry; they need stable IDs so users can attach BCs, thicknesses, and probes that survive parameter changes. Composes with `persistent-naming-v2.md` — adds a derived-geometry naming sub-vocabulary for mid-surface entities (e.g., `body.mid_surface().face("region_0")`, `body.mid_surface().edge("flex_root")`).

**Validation: shell-specific benchmarks.** Pinched cylinder, Scordelis-Lo roof, hemisphere with point loads, twisted beam — the canonical shell formulation tests with known reference solutions and locking sensitivity. Plus a mixed shell/tet benchmark: flexure attached to a solid block (canonical coupling problem).

**GUI rendering deferred to sibling PRD.** Shell-specific rendering (mid-surface vs. extruded display, top/mid/bottom stress toggle, mixed shell/tet body rendering, shell-normal debug overlay) tracked separately in `fea-gui-rendering-shells.md`. Headless shell FEA is independently valuable; GUI lands in parallel.

## Open design questions

- **Medial-extraction edge-pruning threshold.** Spurious branches at body edges and corners need pruning by branch-length-vs-local-thickness ratio. Default ratio TBD empirically once the extractor is implemented; configurable via `ElasticOptions.shell_branch_prune_ratio`.
- **Voxel resolution auto-default.** `thickness/3` is the lean for the thinnest expected feature, but "thinnest expected feature" requires either user hint or a coarse-voxelization-and-iterate pre-pass. Decide during implementation.
- **`@shell(thickness=...)` argument required vs. optional.** Lean: optional — extraction provides thickness, the argument is a override / declaration of intent. Confirm during stdlib task.
- **Mid-surface remeshing threshold.** When the extracted triangle mesh has poor element quality (sliver triangles), invoke MMG2D-style remeshing or fail with diagnostic. Quality threshold TBD empirically.

## Decomposition plan

Twenty-three tasks. Voxel-medial extraction (T1-T4) and shell element kernel (T5-T9) are independent and parallelisable. Mixed-element work (T10-T12) and engine integration (T18-T20) gate on both. All tasks queued with `planning_mode = true` (architect plans before implementation).

**Voxel-medial mid-surface extraction (depends on v0.2 OpenVDB):**

1. `reify-shell-extract` crate skeleton + voxel-medial mask algorithm. Per-voxel bidirectional nearest-surface query via OpenVDB SDF, gradient-discontinuity detection, medial mask output as a sparse voxel grid. **Gate:** v0.2 OpenVDB Voxel ReprKind realizable from B-rep at thickness-relevant resolutions.
2. Mid-surface mesh extraction: iso-surface from medial mask (sparse-grid marching-cubes equivalent), per-vertex thickness from SDF. Output is a triangle mesh tagged with per-vertex thickness field.
3. Spurious-branch pruning: detect and remove medial-surface branches whose length/local-thickness ratio falls below `shell_branch_prune_ratio` (configurable, empirical default). Standard medial-axis-cleanup algorithm.
4. Per-region segmentation classifier: connected-component analysis on medial mask + thickness/extent ratio per component. Output: region labels per voxel, tagged mid-surface patches per region, per-region classification (shell-eligible / tet-eligible / mixed-component-of-body).

**Shell element kernel (independent of mid-surface extraction; can be developed in parallel):**

5. MITC3+ element formulation in `reify-solver-elastic`: shape functions, tying-point evaluation, mixed-strain interpolation for transverse shear. Reference element in local 2D frame.
6. Shell stiffness assembly under isotropic linear-elastic constitutive law: through-thickness analytical integration (constant-thickness, isotropic D matrix), local-to-global transformation per element using mid-surface frame.
7. Shell stress recovery: top/mid/bottom stress fields in local frame; per-element `frame` field for global-frame transformation. Sampling helpers for arbitrary mid-surface point queries.
8. Shell BC application: Dirichlet on rotational DOFs (auto-clamp from `FixedSupport`); explicit `PinnedSupport` opt-out path. Reuses v0.3 row-elimination plumbing.
9. Shell mid-surface mesher: triangulate mid-surface mesh from extractor output, quality checks (triangle aspect ratio, min angle), optional MMG2D-style remeshing on quality failure. Default Gmsh 2D from extractor mesh.

**Mixed shell/tet integration (depends on extraction T1-T4 and element kernel T5-T9):**

10. MPC tying mechanism: shell-rotation ↔ tet-displacement-gradient constraint equations, three tying points across thickness on tet side. Reuses v0.3 row-elimination machinery; one new constraint type alongside Dirichlet, no new linear-algebra surface.
11. Mixed-element global assembly: K matrix assembly handling tet + shell + tying constraints in one system. Per-element-kind sparse pattern, unified solve via existing CG solver.
12. Mixed-region body partitioning: take auto-segmenter output (T4), mesh tet regions with Gmsh (existing v0.3 path), mesh shell regions with mid-surface mesher (T9), wire MPCs at interfaces (T10).

**Stdlib / language surface (independent of solver gates; parallel-shippable):**

13. `@shell(thickness = ...)` annotation: parse, validate, optional thickness param. Compiler integration; thickness extracted from medial analysis if argument omitted.
14. `@solid` annotation: parse, validate, force-tet behavior — bypasses extraction entirely.
15. `PinnedSupport` stdlib type and shell-aware extension to `FixedSupport`: auto-clamp rotation when applied to a shell entity, explicit-pin via `PinnedSupport`. Type compatibility checks at call site.
16. `ElasticResult` extension for shell results: structured stress field with `top` / `mid` / `bottom` channels and per-element `frame` field. Tet results populate all channels with the same value (backward-compatible); existing `result.stress` access aliases to `result.stress.mid`.
17. `ElasticOptions` extension: `shell_threshold` (default 0.2 thickness/extent ratio), `shell_voxel_size` (default thickness/3 of thinnest feature, with iterative auto-default fallback), `shell_branch_prune_ratio`, `shell_force` (off/auto/on, settable globally and per-body via annotation).

**Engine integration (depends on extraction + kernel + stdlib):**

18. Auto-classification dispatch: per-body, run voxel/medial extraction (cached as a ComputeNode keyed on geometry hash + extraction options); decide shell / tet / mixed per region; route to appropriate kernel path. Cache key includes extraction options so threshold changes invalidate cleanly.
19. Shell extraction failure handling: hard-error on `@shell` annotated bodies, fallback-with-diagnostic on auto-classified bodies. Diagnostic mapping for common failures: no medial mask, inconsistent thickness, too-thick anywhere, multi-thickness step exceeds threshold, segmentation produced no clear regions.
20. Persistent-naming for derived mid-surface entities: stable IDs for mid-surface faces and edges. Adds a derived-geometry naming sub-vocabulary on top of `persistent-naming-v2`. **Gate:** persistent-naming-v2 PRD shipped or its derived-geometry hook landed.

**Validation & polish:**

21. Shell benchmark suite: pinched cylinder, Scordelis-Lo roof, hemisphere with point loads, twisted beam. Reference solutions, locking-detection assertions, P1-MITC3+ accuracy comparisons against published values.
22. Mixed-region validation: flexure-on-block test case (canonical shell/tet coupling problem). Compares against published reference solution; verifies MPC tying gives smooth stress/displacement across the interface.
23. End-to-end example: thin-walled bracket with `param thickness : Length = auto`, `minimize mass subject to max(stress.top.von_mises) < material.yield_stress`. Demonstrates auto-classification + shell-element FEA + auto-resolve loop closing the design loop on a thin-body design.

## Out of scope for this PRD

- Composite / laminated shells — `v0_5/composite-laminated-shells.md` stub.
- Varying-thickness shells (non-uniform thickness across mid-surface) — `v0_5/varying-thickness-shells.md` stub. Note: voxel-medial extraction *already* produces per-vertex thickness, but v0.4 collapses to per-body scalar; v0.5 preserves the field.
- Shell stability / buckling analysis — `v0_5/structural-stability-buckling.md` stub.
- Membrane-only or plate-only formulations — covered as degenerate cases of full shell.
- Beams / 1D structural elements — sibling PRD if demand emerges.
- P2 (6-node) shell elements — defer until P1 ships and demand surfaces.
- Quad shell elements (4-node MITC4 / S4R-style) — defer; mid-surface quad meshing is uneven and no v0.4 use case demands it.
- GUI rendering of shell results — `v0_4/fea-gui-rendering-shells.md` sibling PRD.

## Relationship to other PRDs and tasks

- **Successor to `structural-analysis-fea.md`** — addresses the thin-body limitation explicitly called out in v0.3. Shares solver kernel, materials, BC framework, options structure. The v0.3 thin-body diagnostic (warning only) becomes actionable here.
- **Hard dependency on `multi-kernel.md` (v0.2) — specifically OpenVDB Voxel ReprKind** for mid-surface extraction.
- **Hard dependency on `persistent-naming-v2.md`** for derived-geometry naming of mid-surface entities (T20).
- **Partial overlap with `hex-wedge-meshing.md`** — both address thin-body issues but via different routes (hex/wedge is anisotropic 3D solid mesh on swept geometry; this PRD is 2D shell on arbitrary thin geometry). Hex/wedge is smaller scope and ships v0.3.x; shells is the proper general fix.
- **Composes with `multi-load-case-fea.md`** — shells participate in multi-load workflows the same way solids do; envelope reductions work over the new structured stress field.
- **Composes with `mesh-morphing.md`** — mid-surface morphs alongside the original body geometry under parameter changes; warm-start preservation works the same way as for tet meshes.
- **Composes with `a-posteriori-error-estimation.md`** — Z-Z indicator extends to shell elements with through-thickness sampling; refinement strategy may differ (in-plane refinement vs. through-thickness, the latter often handled by formulation choice rather than refinement).
- **Sibling: `fea-gui-rendering-shells.md` (v0.4)** — GUI work for shell rendering, parallel-shippable.
- **Successor stubs: `composite-laminated-shells.md`, `varying-thickness-shells.md`, `structural-stability-buckling.md` (all v0.5+)** — domain extensions filed for when use cases emerge.

# Audit: Mesh Morphing for Topology-Preserving Parameter Changes

**PRD path:** `docs/prds/v0_3/mesh-morphing.md`
**Auditor:** audit-mesh-morphing
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 11 (WIRED count: 7)

## Top concerns

- **Gap 1 — engine wiring is the chokepoint.** Every algorithm primitive (Stage A, Stage B, eligibility, boundary-BC translation, Laplacian quick-pass, elasticity morph, quality check, stiffness rules, MorphOptions) is **WIRED** inside `reify-mesh-morph`. None of it is invoked from anywhere outside the crate. The VolumeMesh realization path in `crates/reify-eval/src/engine_build.rs::dispatch_volume_mesh` only dispatches between Tet and Swept; there is no morph-or-remesh branch. The user-visible promise ("morphing is automatic; user does not opt in") is unfulfillable today. Task 2947 (PRD task #10) is pending and is the load-bearing integration; until it lands the crate is essentially shelf-ware.
- **Gap 2 — no real OCCT Projector, no real BoundaryAssociation producer.** The `Projector` trait and `BoundaryAssociation` struct are well-shaped, but **no concrete `impl Projector` exists** anywhere in the workspace (only a `RecordingProjector` in `boundary.rs` tests), and the Gmsh/OCCT surface-mesh paths do not emit `NodeAttachment` data — `BoundaryAssociation::associate` is never called from outside `mesh-morph`'s own tests. Even if task 2947 lands, it has no way to populate the boundary association or invoke a real closest-point projector. These two missing producers are silent prerequisites the PRD glosses over by deferring them to "follow-on tasks accompanying #10 or #7".
- **Gap 3 — `CorrespondenceMap::vertex_to_vertex` is structurally always-empty in v0.2.** The boundary module documents this explicitly (`boundary.rs:130-140`) — any `OnVertex` node returns `ProjectionFailure::MissingCorrespondence`. Any tet mesh where a P1 vertex node lands on a B-rep vertex would fail to morph today. The PRD does not call this out; Phase 3 must decide whether v0.3 morph runs only on meshes with zero on-vertex nodes (very restrictive) or whether vertex correspondence is added.
- **Gap 4 — FEA-warm-start preservation depends on engine wiring that owns warm-state plumbing.** The PRD's compounding-win narrative ("FEA warm-start state preserved across morph") is asserted but unverified. Task 2952 (warm-start regression) is pending; it gates on 2947. Until then the "preserves element-to-DOF mapping" claim is **only documented**, not pinned. Note that the PRD says "morph is essentially `solve_elastic_static` called with loads=[], supports=boundary_displacements" — that very `solve_elastic_static` runtime entry point is **GR-001 FICTION** (structure-ctor runtime eval for `Support`/`Load`) — but mesh-morph sidesteps it by composing reify-solver-elastic primitives directly (no stdlib `fn` call). So GR-001 does NOT transitively block mesh-morph algorithmically; it only blocks any user-visible `morph()` exposure that runs through stdlib FEA.

## Mechanisms

### M-001: Stage A — design-tree structural classifier (graph-shape hash + per-cell dimensional/structural classification)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/structural_classifier.rs:74-230` (`realization_graph_shape_hash`, `classify_cell`, `stage_a_eligible`); used by `crates/reify-mesh-morph/src/eligibility.rs:125-127`. Conservative-default rule: everything not Scalar/Real/Int → Structural — catches boolean modes, enum-typed mode params, pattern counts (rule 3 explicitly), feature suppression (rule 2 via `structure_controlling`).
- **Blocks:** N/A
- **Note:** PRD criterion (a/c) "graph shape unchanged + no feature added/removed/reordered" is the topology-fingerprint hash; criterion (b) "only dimensional leaves differ" is the per-cell rule-4 dispatch. Test coverage for the conservative-default catch-all (PRD-named: boolean-mode, suppression, enum-typed mode) is implicit via the type whitelist — no dedicated regression-guard test for each PRD-named example, but the structural-controlling test (`stage_a_eligible_mixed_dimensional_and_structural_diff_returns_false`) pins the gate.

### M-002: Stage B — persistent-naming bijection check + CorrespondenceMap

- **State:** PARTIAL
- **Failure mode:** F2 (mechanism exists but is missing a load-bearing sub-mechanism)
- **Evidence:** `crates/reify-eval/src/morph_stage_b.rs:46` (`CorrespondenceMap`); `vertex_to_vertex` field is **structurally always-empty in v0.2** (documented at lines 140, 176-177; pinned by tests at lines 435-436, 470-471, 505-506).
- **Blocks:** any morph of meshes whose surface nodes attach to B-rep vertices (PRD task #5's `OnVertex` branch always returns `ProjectionFailure::MissingCorrespondence`).
- **Note:** Face-to-face and edge-to-edge correspondence work; vertex correspondence is a v0.2 known-empty hole that the PRD does not flag. Phase 3 must decide: restrict morph eligibility to meshes with zero on-vertex surface nodes, or fill the vertex bijection.

### M-003: Combined eligibility predicate `morph_eligible`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/src/eligibility.rs:122-152`; structured `Reason` taxonomy at lines 64-85; comprehensive tests in `eligibility::tests` cover all three reject paths.
- **Blocks:** N/A
- **Note:** Composes Stage A → realization-gate (caller's responsibility) → Stage B per the PRD-documented order. `Reason::NamingLayerError` projected as its own top-level variant for counter routing.

### M-004: `reify-mesh-morph` crate skeleton + `MorphOptions` / `MorphFailure`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/Cargo.toml`; `src/lib.rs:142-158` (public `morph()` signature stub returning `SolverError` on the eligible path — engine logic deferred to task #10); `src/options.rs` (MorphOptions + 4-variant MorphFailure taxonomy with exhaustive-match compile fences).
- **Blocks:** N/A
- **Note:** Public API surface for tasks #5-#9 fully committed; the top-level `morph()` function in `lib.rs` is still a stub-returning-SolverError on the eligible arm. Consumer-neutral surface holds (`VolumeMesh` only, no FEA-specific types). **Workspace audit confirms zero consumers outside `reify-mesh-morph` itself** (search at `options.rs:271-289` documents this) — the crate is reachable but unused by the engine.

### M-005: Boundary-node correspondence + closest-point projection (compute_dirichlet_bcs)

- **State:** PARTIAL
- **Failure mode:** F3 (consumer-side wired; producer-side absent)
- **Evidence:** `crates/reify-mesh-morph/src/boundary.rs:212-276` (compute_dirichlet_bcs). `NodeAttachment` / `BoundaryAssociation` types defined and tested. **No external producer**: `grep BoundaryAssociation /home/leo/src/reify/crates/reify-kernel-{gmsh,occt}` returns zero hits. Surface mesh path emits `VolumeMesh` but does not record per-vertex `NodeAttachment`. Boundary.rs explicitly documents this at lines 32-35 ("the producer is stubbed today; this task defines the consumer-side shape").
- **Blocks:** 2947 (engine wiring) — without a producer, the wiring cannot construct a BoundaryAssociation to feed.
- **Note:** Half of the contract. The Gmsh adapter would need to emit `(node_index, NodeAttachment)` pairs at surface-mesh time. No task currently filed against the producer side that I could find (search did not surface one).

### M-006: Projector trait (closest-point dependency injection)

- **State:** PARTIAL
- **Failure mode:** F2 (trait exists, no concrete OCCT-backed impl)
- **Evidence:** `crates/reify-mesh-morph/src/boundary.rs:161-182` (trait def); `crates/reify-mesh-morph/src/boundary.rs:383` (RecordingProjector — test-only). No `impl Projector for` outside tests in the workspace.
- **Blocks:** 2947 (engine wiring) — engine cannot drive `compute_dirichlet_bcs` without a real `dyn Projector`.
- **Note:** Trait is well-shaped (face/edge/vertex methods, error payload pattern mirrors `SolverErrorPayload`). The real OCCT backing (BRepExtrema_DistShapeShape on the OCCT side) is a follow-on the PRD explicitly defers ("follow-on task accompanying PRD task #10 or #7"). No follow-on task surfaced in fused-memory search.

### M-007: Laplacian quick-pass smoother

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/src/laplacian.rs:1-80` doc-comment + module. P1-only; structured `LaplacianFailure` enum; bit-stable Jacobi iteration order via BTreeSet adjacency. Re-export compile fence at `lib.rs:345-360`.
- **Blocks:** N/A
- **Note:** PRD §"Laplacian quick-pass for trivially small changes" delivered. Engine wiring chooses Laplacian vs. elasticity per `MorphOptions.laplacian_quickpass_threshold`.

### M-008: Linear-elasticity morph (uniform fictitious stiffness)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/src/elasticity.rs:1-80` doc-comment; composes `element_stiffness`, `assemble_global_stiffness`, `apply_dirichlet_row_elimination`, `solve_cg` from `reify-solver-elastic`. Re-export compile fence at `lib.rs:369-400`. `elasticity_morph` and `elasticity_morph_with_cg_opts` public.
- **Blocks:** N/A
- **Note:** PRD "the morph is essentially `solve_elastic_static` called with loads=[], supports=boundary_displacements" — implementation **does not** route through any stdlib `solve_elastic_static` runtime entry point; it directly composes the FEA assembly+solve primitives. This sidesteps GR-001 entirely.

### M-009: Spatially-varying fictitious stiffness (StiffnessRule)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/src/options.rs:30-64` (three-variant enum); `elasticity.rs:79-105` (per-element `youngs_modulus` derivation); MIN_VOLUME/MIN_LENGTH_SQ epsilon clamps; `StiffnessRule::InverseVolume` is default per PRD task #8. Compile fence at `lib.rs:434-439`.
- **Blocks:** N/A
- **Note:** PRD §"Spatially-varying fictitious stiffness" delivered with both PRD-suggested rules (1/V and 1/L²) plus Uniform baseline.

### M-010: Quality check (hard + soft fail thresholds)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-mesh-morph/src/quality.rs` module; 3-variant `QualityVerdict` (Pass / HardFail / SoftFail). Three configurable thresholds on `MorphOptions`. Hard fail on negative scaled-J; soft fail on min < floor, %below-0.25 > pct, or AR factor > max. Degenerate (sj==0) element surfaced unconditionally in SoftFailDetails. Compile fence at `lib.rs:408-427`.
- **Blocks:** N/A
- **Note:** PRD §"Quality threshold for fallback" delivered. Thresholds calibrated by task 2950 + re-calibrated by task 3451 (recorded in options.rs default-impl comments at lines 198-230).

### M-011: Quality-threshold calibration against representative parametric geometries

- **State:** PARTIAL
- **Failure mode:** F5 (test fixture limited to synthetic procedural geometries)
- **Evidence:** `crates/reify-mesh-morph/tests/calibration.rs` exists (plate hole-diameter sweep + L-bracket fillet-radius sweep). The PRD specifies three sweeps ("fillet-sweep on a bracket, hole-diameter sweep on a plate, wall-thickness sweep on a box"); the box sweep was **dropped** during calibration with the documented justification that the hollow-box has zero interior vertices and reduces to identity (lib.rs:55-60). `options.rs:222-229` explicitly flags "this calibration is derived from two synthetic procedural fixtures only — no real-CAD-mesh data point yet".
- **Blocks:** N/A (calibration done, but with self-acknowledged-narrow fixture set)
- **Note:** Materiality factor 1.20 lives at `tests/calibration/sweep.rs::MATERIALITY_FACTOR`. The PRD-promised "calibrated empirically against representative parametric geometries" is met for procedural fixtures; the "real CAD geometry once task #10 lands" caveat is recorded but unfulfilled.

### M-012: VolumeMesh realization wiring (morph-or-remesh dispatch on cache miss)

- **State:** FICTION
- **Failure mode:** F1 (PRD-stated dispatch path absent from the realization code)
- **Evidence:** `crates/reify-eval/src/engine_build.rs:2403-2462` (`dispatch_volume_mesh`) dispatches Tet vs. Swept only — no morph branch. Zero consumers of `reify-mesh-morph` exist outside the crate itself (search: only `Cargo.toml` workspace declarations + comments in `options.rs` reference the crate name). Task 2947 (PRD task #10) is **pending**, dependencies 2924/2940/2943/2944/2946/3092 (some pending, some done).
- **Blocks:** **THE** load-bearing gap. Without task 2947, the PRD's user-visible promise ("morphing is automatic; user does not opt in") is unfulfillable. Also blocks tasks 2948 (counters), 2949 (debug RPC), 2952 (warm-start regression), 2953 (slider benchmark).
- **Note:** This is the gating gap for the entire feature reaching users.

### M-013: Diagnostic counters + verbose logging (morphed / remeshed / ineligible / quality-rejected)

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Failure-mode visibility" missing entirely)
- **Evidence:** Workspace search for `MorphStats|MorphCounter|morph_count|MorphTelemetry|MorphDiagnostic` returned zero hits. No `--verbose` summary surface in `reify-cli` for mesh-morph stats. PRD specifies info-level log on quality-reject-remesh as "load-bearing" — currently impossible because the morph never runs in the first place. Task 2948 pending (depends on 2947).
- **Blocks:** The PRD-named "why was that slider tick slow?" user-debugging case.
- **Note:** Cleanly gated on M-012; no design hazard, just unbuilt.

### M-014: Debug RPC tool for live morph stats (REIFY_DEBUG=1 endpoint)

- **State:** FICTION
- **Failure mode:** F1 (no debug-MCP tool registered)
- **Evidence:** Workspace search for `morph_stats` returned zero non-node_modules hits. The debug-MCP tool registry (gui/src-tauri / reify-mcp) carries the documented set; mesh-morph stats are not among them. Task 2949 pending (depends on 2948).
- **Blocks:** Power-user investigation of slider sluggishness.
- **Note:** Gated on M-013.

### M-015: Morph-source policy — from-most-recent-in-memory only

- **State:** FICTION
- **Failure mode:** F1 (no within-session most-recent-mesh registry)
- **Evidence:** Workspace search for `from_most_recent|most_recent_in_memory|recent_in_memory` returned zero hits. The PRD asserts "Within a session, always morph from the most-recent in-memory mesh." There is no per-body in-memory registry of "last realized mesh" that 2947's wiring could query. The `RealizationCache` in `reify-eval` is keyed by content hash, not "most recent for body X".
- **Blocks:** 2947 wiring; the policy is asserted but the data structure that enforces it is undesigned.
- **Note:** Subtle. Task 2947's description ("if eligible AND a most-recent in-memory mesh exists") presumes this lookup exists; in fact it would have to be built.

### M-016: Morph-chain degradation bounds test (50+ tick parameter sweep)

- **State:** FICTION
- **Failure mode:** F1 (PRD-promised regression test absent)
- **Evidence:** Workspace search for `morph_chain|chain degradation|MorphChain` returned zero hits. Task 2951 pending (depends on 2947).
- **Blocks:** PRD assertion "elasticity morph's per-step BVP framing keeps chain degradation tight" is **unverified**.
- **Note:** Gated on M-012 (no chain to degrade until the engine wiring runs).

### M-017: FEA warm-start preservation regression test (warm-state survives morph)

- **State:** FICTION
- **Failure mode:** F1 (PRD-load-bearing claim is unverified)
- **Evidence:** Workspace search for `warm.start.*morph|morph.*warm` in test code returned zero hits. The mesh-morph crate's BTreeMap iteration discipline in `BoundaryAssociation` (boundary.rs:55) and elasticity.rs:377 are explicitly designed for warm-start bit-stability, BUT no end-to-end test asserts that an FEA solver's warm-state actually survives a morph round-trip. Task 2952 pending.
- **Blocks:** The PRD's headline compounding-speedup claim ("preserves FEA solver warm-start state across parameter ticks").
- **Note:** The infrastructure (BTreeMap-ordered BCs) is in place to **make** preservation possible; the integration that **demonstrates** preservation is absent.

### M-018: End-to-end slider-responsiveness benchmark (≥10× wall-clock reduction at 100K elements)

- **State:** FICTION
- **Failure mode:** F1 (benchmark surface absent, CI-tracked perf metric absent)
- **Evidence:** Workspace search returned no slider-responsiveness or mesh-morph benchmark. Task 2953 pending (depends on 2947, 2948, 2930).
- **Blocks:** PRD's "single biggest interactive-smoothness lever" claim is unbenchmarked.
- **Note:** Gated on M-012. Also gates on FEA task 2930 ("end-to-end example as starting fixture") which is itself pending.

## Cross-PRD breadcrumbs

- **`persistent-naming-v2.md` (v0.2)** — `TopologyAttributeTable` + `CorrespondenceMap.face_to_face`/`edge_to_edge` are wired (task 2590 done; selector resolution complete per task 2652). `vertex_to_vertex` is the documented-empty hole — this would surface as a gap in any v0.2 PNv2 audit too.
- **`structural-analysis-fea.md` (v0.3)** — task 2925 (`ReprKind::VolumeMesh`) is the realization-path gate; mesh-morph's `morph()` operates on `VolumeMesh` directly. The FEA solver primitives mesh-morph composes (`element_stiffness`, `assemble_global_stiffness`, `apply_dirichlet_row_elimination`, `solve_cg`) all live in `reify-solver-elastic` and are wired. Mesh-morph does **not** transitively block on GR-001 because it composes solver primitives directly rather than routing through stdlib `solve_elastic_static`.
- **`persistent-fea-cache.md` (v0.3)** — the PRD explicitly says this PRD's earlier "caching morphed meshes with morph provenance" note should be removed. **Did not check** the persistent-fea-cache PRD to verify the removal landed; out of scope.
- **`mesh-morph-nearest-cached.md` (v0.4 stub)** — PRD names this stub as the deferred follow-on; **the stub file does not exist in `docs/prds/v0_4/`** (ls confirms: only `a-posteriori-error-estimation.md`, `fea-gui-rendering-shells.md`, `structural-analysis-shells.md`). Minor ORPHAN-shaped issue: PRD names a follow-on that has not been filed.
- **`a-posteriori-error-estimation.md` (v0.4)** — "Mesh-morphing composes with parameter values during lazy-refinement at decision time" surfaced in fused-memory; refinement composition not audited here.
- **GUI-rendering PRD** — the morph-badge visualization is "tracked under the GUI rendering PRD". Not audited here.

## Notes for Phase 3

- Eleven of the eighteen mechanisms are gaps; **ten of those eleven are gated on task 2947 (engine wiring, PRD task #10)**. Phase 3 should treat 2947 as a single rate-limiter mechanism rather than ten independent decisions.
- Three of the eleven gaps are independent of 2947 in their own right:
  - **M-002** (vertex_to_vertex always empty): a PNv2-shape decision Phase 3 must make first
  - **M-005** (BoundaryAssociation producer absent on Gmsh side): a kernel-adapter decision
  - **M-006** (no concrete OCCT Projector): a kernel-adapter decision
- The PRD does NOT have GR-001 (struct-ctor runtime eval) as a transitive blocker because the morph algorithm composes FEA primitives directly, not via stdlib `fn solve_elastic_static`. This is a happy surprise; it means morph could ship without any stdlib-side ergonomics for `Support`/`Load`/`LoadCase`/`MaterialName`.
- Note worth flagging: PRD task #4's `morph()` API is designed for the user-visible "always-on automatic morph" promise — but with the engine-wiring (M-012) absent, the only way today to drive the morph is to call the crate's lower-level `elasticity_morph`/`laplacian_smooth` directly, as the calibration tests do. There is currently **no integration test** that exercises the top-level `morph()` function through to a populated boundary association.

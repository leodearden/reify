# PRD: Mesh Morphing for Topology-Preserving Parameter Changes

Status: deferred — candidate v0.3.x or v0.4 follow-on to `structural-analysis-fea.md`. Cross-cuts geometry kernel + FEA + persistent cache. Filed 2026-05-02 from FEA PRD spillover. Design resolved 2026-05-04 — see "Resolved design decisions" below.

## Goal

Avoid mesh-from-scratch on parameter changes that preserve topology (dimensional changes only — no add/remove of features). Detect such changes, morph existing mesh nodes to fit the updated boundary, reuse element connectivity. Big lever for any mesh-consuming workflow (FEA, CFD, toolpath, lattice generation), highest leverage for slider-driven and auto-resolve interactions. Also preserves FEA solver warm-start state across parameter ticks because element-to-DOF mapping survives the morph — compounding the speedup beyond mesh time alone.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships a Gmsh-based volume mesher that runs from scratch on every cache miss. For a typical parametric design — `param thickness : Length = auto` driving an auto-resolve loop, or a user dragging a dimension slider — every parameter tick changes the geometry, blowing the mesh cache on every step. At 100K elements, that's ~3s serial / ~0.3s parallel per tick of mesh time; auto-resolve loops with 50 evaluations pay this 50 times.

Yet for typical dimensional parameter changes — fillet radius, wall thickness, hole diameter — the mesh *topology* (element connectivity, surface-element correspondence) doesn't have to change. Only node positions need to update. Mesh morphing handles this in milliseconds.

Two compounding wins:

1. **Mesh time elided.** Morph is O(milliseconds) where remesh is O(seconds).
2. **FEA warm-start state preserved.** Because the morphed mesh has the same element connectivity as its source, the prior solver iterate's element-to-DOF mapping is still valid. Warm start carries through the morph intact. With remesh, the prior iterate would have to be projected onto the new mesh (an interpolation step) or warm start is abandoned. So morphing saves both the mesh build *and* keeps the FEA solver near its previous solution — the auto-resolve loop converges in many fewer iterations per parameter tick.

This is the single biggest lever for sub-second slider response in FEA workflows. It also benefits any future mesh-consuming op (CFD, EM, CAM toolpaths, lattice infill, voxel-octree builders).

## Why deferred (and why a separate PRD)

- Depends on the v0.3 FEA kernel landing first — both because morph quality is calibrated against an FEA consumer, and because the chosen morph algorithm reuses FEA solver primitives (see "Resolved design decisions").
- Affects geometry-kernel layer, not just FEA — needs careful API design so that morph results integrate with the existing `RealizationNode` / `ReprKind::VolumeMesh` cache.
- Per-purpose tolerance and topology selectors must be sufficiently mature for the diff classifier to express "old face X corresponds to new face X."
- Persistent-naming v2 (`docs/prds/v0_2/persistent-naming-v2.md`) is a hard precondition: morph eligibility relies on a bijection between old-B-rep and new-B-rep face/edge/vertex sets, which persistent naming provides.

## Resolved design decisions (2026-05-04)

**Two-stage topology-preservation classifier.** Morph eligibility is determined in two stages:

- **Stage A — design-tree structural check (pre-realization).** Hash the realization-graph shape, classify each leaf parameter as *dimensional* (length, angle, radius, scalar) or *structural* (pattern count, boolean mode, feature suppression, enum-typed mode parameters). Topology is *potentially* preserved iff (a) the graph shape is unchanged, (b) the only differing leaves are dimensional, and (c) no feature has been added, removed, or reordered. Stage A is a cheap pre-flight that rules out obvious topology-changing cases without doing geometry-kernel work.
- **Stage B — persistent-naming bijection check (post-realization).** Even when Stage A passes, continuous parameter changes can cross topology-changing thresholds: fillet radius reaching 0 (face vanishes), through-hole becoming blind, two faces merging when an offset parameter goes negative. Stage B compares the realized old and new B-reps, asking the persistent-naming layer for a bijection between their face/edge/vertex sets. If the bijection exists, topology is preserved and morphing is eligible. Otherwise the new B-rep flows to from-scratch meshing.

Classification therefore happens *after* the new B-rep is realized but *before* meshing — B-rep realization is cheap relative to meshing, so this ordering is fine. The classifier explicitly rejects when persistent naming itself fails to produce a bijection; morph fragility is bounded by naming reliability, which is the right coupling.

**Algorithm: linear-elasticity morph reusing `reify-solver-elastic`, with a Laplacian quick-pass for trivially small changes.** Treat the existing mesh as a fictitious linear-elastic continuum, prescribe each surface node's displacement (its closest-point projection onto its persistent-naming-mapped face counterpart on the new B-rep) as a Dirichlet boundary condition, solve a fictitious linear-elasticity problem for interior node displacements. The same kernel that computes physical FEA computes the morph; we get warm-start, parallelism, and validated linear-algebra-in-Rust paths for free, and the morph quality is FEA-grade (substantially better than RBF or Laplacian at moderate deformations). For displacements where the maximum boundary movement is below ~1 % of local element size, skip the elasticity solve and run a few iterations of constrained Laplacian smoothing instead — the elasticity overhead exceeds its benefit at that scale.

The implementation cost is minimal because all the heavy primitives (assembly, sparse solve, Dirichlet application, warm start) ship with the FEA kernel. The morph is essentially `solve_elastic_static` called with `loads = []`, `supports = boundary_displacements`, and a fictitious material.

RBF morphing (thin-plate spline, Wendland C2) is *not* taken: it requires its own implementation effort and has no quality advantage over the elasticity morph. Pure Laplacian smoothing is *not* taken as the primary path: it degrades on boundary curvature changes and graded meshes — fine as a quick-pass for small changes but unsuitable as the general algorithm.

**Spatially-varying fictitious stiffness.** Uniform fictitious stiffness causes the elasticity morph to "absorb" most boundary displacement near the largest elements, leaving the small-element regions overstrained. Standard mitigation: scale local stiffness inversely with element size (small elements → stiffer). One simple workable rule is `youngs_modulus_local ∝ 1 / element_volume` (or `1 / edge_length²` for tets). This preserves mesh gradation — small elements near features stay small, large elements in bulk regions absorb the bulk of the displacement.

**Quality threshold for fallback.** Two-tier check on the morphed mesh:

- **Hard fail — element inversion.** Any tetrahedron with negative Jacobian determinant rejects the morph immediately. Fall back to remesh.
- **Soft fail — degraded quality.** Reject when *any* of: min scaled Jacobian anywhere < 0.15; more than 1 % of elements with min scaled Jacobian < 0.25; any element's aspect ratio increased by more than 2× compared to the source mesh.

All three thresholds are tunable via `MorphOptions`. Defaults are calibrated empirically against representative parametric geometries (fillet sweeps, hole-diameter sweeps, thickness sweeps); a calibration task in the validation phase fixes those defaults.

**Morph source policy: from-most-recent-in-memory only.** Within a session, always morph from the most-recent in-memory mesh. The quality check is the sole safeguard against chain degradation — if a long chain of morphs has cumulatively distorted quality, the next morph will be rejected and a fresh remesh resets the chain. For elasticity-based morphing the chain degradation is bounded (each morph is a fresh BVP, not an iterative perturbation), so in practice chains hold up well.

Cross-session cold start uses full from-scratch meshing (or a persistent-cache exact hit, when the parameter value matches a cached entry). **Nearest-cached morph** — querying the persistent cache for a parameter-space neighbour and morphing from it — is a recognised follow-on opportunity but is deferred to a v0.4 PRD (`mesh-morph-nearest-cached.md` stub) once v0.3.x telemetry shows how often the cold-start case matters and how dense the cache typically is in parameter space. Building the spatial index on the cache without that data risks over-engineering.

**Persistent cache integration.** Persistent cache stores from-scratch results only; morphed meshes are never persisted. Reasoning: the morph is path-dependent (different morph-source meshes can produce different valid morph results for the same target), but the persistent cache key is path-independent — so caching morph results either contaminates the cache with path-dependent state or fragments it with a "morph provenance" key dimension. Since morph is sub-second within session and the persistent cache covers cross-session continuity via from-scratch entries, the two layers are orthogonal and complementary. (The persistent-fea-cache PRD is updated to reflect this — its earlier note about caching morphed meshes "with morph provenance recorded" should be deleted.)

**Generic crate scope.** Lives in a new crate `reify-mesh-morph`, consumer-neutral. API operates on `(VolumeMesh, old_BRep, new_BRep)` and produces a `Result<VolumeMesh, MorphFailure>`, plus an eligibility predicate `eligible(old_BRep, new_BRep) -> bool`. Two functions, no consumer-specific surface. FEA imports it; future CFD / EM / CAM consumers import it the same way without further plumbing. Surface meshes (different concerns: UV / normal preservation), voxel grids / SDFs (fundamentally different operation), and octrees / BVHs are out of scope.

The `reify-mesh-morph` crate depends on `reify-solver-elastic` for the elasticity morph but is itself consumer-neutral.

**Failure-mode visibility.** Three layers:

| Event | Visibility |
|---|---|
| Classifier rejects (topology changed) | session counter, no log |
| Morph attempted, succeeded | session counter, trace-level log |
| Morph attempted, quality rejected → remesh | session counter, **info-level log** (this is the "why was that slider tick slow?" case) |
| Morph attempted, panicked | error log + full diagnostic |

Plus a session-level diagnostic counter exposed via:
- `--verbose` flag: prints session totals at exit (`mesh updates: 47 morphed, 4 remeshed, 2 ineligible`).
- Debug RPC tool (`REIFY_DEBUG=1`): live counter readable from the debug MCP for power users / our own profiling.
- GUI: tracked separately as a small badge visualisation under the GUI rendering PRD.

The info-level log on quality-reject-remesh is the load-bearing one. Everything else is for debugging or detailed introspection.

## Sketch of approach

Pipeline on parameter change:

1. **Stage A classification** — pre-realization graph diff classifier. If structural change detected (feature add/remove, pattern count, boolean mode, suppression), skip directly to remesh.
2. **B-rep realization** — build new B-rep from the new parameters. (Cheap relative to meshing.)
3. **Stage B classification** — request face/edge/vertex bijection from the persistent-naming layer between old and new B-reps. If bijection fails or counts differ, skip to remesh.
4. **Surface-node projection** — for each old surface node, project onto its persistent-naming-mapped face counterpart on the new B-rep. Build a Dirichlet BC list (one prescribed-displacement per surface node).
5. **Quick-pass screen** — if max boundary displacement < 1 % of local element size, run a few Laplacian smoothing iterations and skip the elasticity solve.
6. **Elasticity morph** — otherwise solve the fictitious linear-elasticity BVP via `reify-solver-elastic` with the spatially-varying stiffness rule, prescribed boundary displacements, and zero loads. Apply node displacements to produce the morphed mesh.
7. **Quality check** — compute element Jacobians and scaled-Jacobian / aspect-ratio metrics. Reject on hard fail (inversion) or soft fail (threshold breach). On reject, fall back to from-scratch remesh.
8. **Cache integration** — morphed mesh becomes the realization for `(new_geometry_hash, mesh_options)` in the in-memory cache, but is *not* written to the persistent cache. The morph is path-dependent; persistent cache stays from-scratch only.

User-visible API: morphing is automatic; user does not opt in. Failure-mode visibility per the table above.

## Pre-conditions for activating

- v0.3 FEA kernel shipped — provides `reify-solver-elastic` (the morph algorithm reuses it) and a concrete consumer to validate morphing benefit.
- `ReprKind::VolumeMesh` variant landed (FEA task #17, task ID 2925).
- Persistent-naming v2 (`docs/prds/v0_2/persistent-naming-v2.md`) shipped — the bijection check in Stage B is its load-bearing primitive.
- Per-purpose tolerance machinery live — needed for morph quality budget and BC tolerance.
- Topology selectors mature enough to express geometry diff classification (already true after v0.2 selector vocabulary v2).

## Decomposition plan

Sixteen tasks. Several depend on FEA solver-kernel tasks and on `ReprKind::VolumeMesh` from the FEA PRD; those gates are noted per task. Classifier tasks gate on persistent-naming-v2 (PRD-level gate, since individual PNv2 task IDs were re-numbered during decomposition restructuring).

**Classifier (depends on persistent-naming-v2 PRD; independent of FEA solver):**

1. **Stage A — design-tree structural classifier.** Realization-graph shape hash. Per-parameter classification (dimensional vs structural). Predicate `stage_a_eligible(old_graph, new_graph) -> bool`. Lives in the geometry-kernel layer adjacent to the existing realization graph machinery. Independent of FEA solver tasks; gates on persistent-naming-v2 only for shared selector / attribute vocabulary.
2. **Stage B — persistent-naming bijection check.** Given two realized B-reps, ask the persistent-naming layer for a face/edge/vertex bijection. Predicate `stage_b_eligible(old_brep, new_brep) -> bool` plus a returned correspondence map (used by task #5 for surface-node projection). **Gate:** persistent-naming-v2 PRD shipped.
3. **Combined eligibility predicate.** `morph_eligible(old, new) -> Eligibility` returning the correspondence map on success or a diagnostic reason on failure. Wraps tasks #1 and #2 with the post-realization invocation order documented in the Sketch.

**Algorithm (depends on FEA solver primitives + `ReprKind::VolumeMesh`):**

4. **`reify-mesh-morph` crate skeleton.** New workspace crate. Public API surface: `morph(old_mesh, old_brep, new_brep, options) -> Result<VolumeMesh, MorphFailure>`, `eligible(old_brep, new_brep) -> bool`, `MorphOptions`, `MorphFailure` variants. Consumer-neutral over `VolumeMesh`. **Gate:** FEA task #17 (task ID 2925) — `ReprKind::VolumeMesh` variant.
5. **Boundary-node correspondence + closest-point projection.** For each surface node in the old mesh, identify which face of the old B-rep it lies on. Use the Stage B correspondence map to pick the new B-rep's mapped counterpart face. Compute closest-point projection onto that specific face (not closest face globally — corner nodes would jump). Output: a list of `(node_index, prescribed_position)` pairs ready to feed as Dirichlet BCs. Depends on tasks #2, #4.
6. **Laplacian quick-pass.** Constrained Laplacian smoothing: surface nodes pinned to their projected positions, interior nodes iteratively averaged with neighbours. A few iterations. Used when max boundary displacement < 1 % of local element size — cheap path that skips the elasticity solve. Depends on task #4.
7. **Linear-elasticity morph (uniform fictitious stiffness).** Call `reify-solver-elastic` with `loads = []`, `supports = boundary_displacements` (from task #5), uniform fictitious E and ν. Apply solved node displacements to the source mesh. Produces the morphed mesh. **Gate:** FEA tasks #7 (2914), #8 (2915), #9 (2916), #10 (2917), #12 (2919), #13 (2920) — all solver primitives needed.
8. **Spatially-varying fictitious stiffness.** Replace the uniform-E pass in task #7 with element-local E ∝ `1 / element_volume` (or `1 / edge_length²` for tets). Preserves mesh gradation. Depends on task #7.
9. **Quality check — hard + soft fail thresholds.** Compute element scaled Jacobians, aspect ratios. Hard fail on any negative determinant. Soft fail when any of: min scaled J < 0.15 anywhere; >1 % of elements with min scaled J < 0.25; max aspect ratio increase > 2× from source. All thresholds tunable via `MorphOptions`. Depends on task #4.

**Engine integration (depends on classifier + algorithm + FEA wiring patterns):**

10. **VolumeMesh realization wiring.** On a `(geometry, options)` cache miss in `ReprKind::VolumeMesh`: invoke `morph_eligible` (task #3); if eligible and a most-recent in-memory mesh exists, attempt morph (tasks #6 / #7 / #8 selected by displacement-magnitude rule); if quality check (task #9) passes, return the morphed mesh; otherwise fall back to from-scratch remesh. Morph result populates the *in-memory* cache but is not written to persistent cache. **Gates:** tasks #3, #6, #7 (or #8), #9; FEA task #16 (2924) for ComputeNode wiring patterns; FEA task #17 (2925) for the VolumeMesh realization path.
11. **Diagnostic counters + verbose logging.** Session-level counters for morphed / remeshed / ineligible (Stage A reject) / ineligible (Stage B reject) / quality-rejected. Verbose-mode summary at exit. Trace-level logs on success; **info-level log on quality-reject-remesh** (the "why was that slider tick slow?" case); error log on panic. Depends on task #10.
12. **Debug RPC tool for live morph stats.** New tool exposed under `REIFY_DEBUG=1` that returns the current session morph counter snapshot. Useful for power users investigating sluggishness and for our own profiling. Depends on task #11.

**Validation & polish (depends on full algorithm + engine integration):**

13. **Quality-threshold calibration.** Run morph + from-scratch remesh on representative parametric geometries: fillet-radius sweep on a bracket, hole-diameter sweep on a plate, wall-thickness sweep on a box. Calibrate the three quality thresholds (min scaled J floor, % below 0.25, aspect-ratio increase) so morph is rejected only when remesh quality is materially better. Bake the calibrated defaults into `MorphOptions`. Depends on tasks #7 / #8, #9.
14. **Morph-chain degradation bounds test.** Run a parameter-sweep auto-resolve loop (50+ ticks) with morphing enabled. Assert the quality metric distribution does not unboundedly degrade — confirms the elasticity morph's per-step BVP framing keeps chain degradation tight. Depends on tasks #7 / #8, #9, #10.
15. **FEA warm-start preservation regression.** End-to-end: auto-resolve loop with morphing enabled vs. disabled. Assert (a) warm-start state survives morph (element-to-DOF mapping stays valid), (b) per-tick CG iteration count is materially lower with morphing enabled than without. Depends on task #10; FEA task #14 (2921) for warm-state plumbing; FEA task #16 (2924) for end-to-end ComputeNode invocation.
16. **End-to-end slider-responsiveness benchmark.** Wall-clock benchmark: typical bracket geometry, 10K – 100K element mesh, drive a slider through 50 parameter values. Assert ≥10× wall-clock reduction vs. always-remesh baseline at the 100K scale. Surfaces in CI-tracked perf metrics. Depends on tasks #10, #11; FEA task #22 (2930) for the end-to-end example as a starting fixture.

## Out of scope for this PRD

- Full from-scratch remeshing (already in `structural-analysis-fea.md` task #17).
- Adaptive mesh refinement driven by error indicators (separate PRD `a-posteriori-error-estimation.md`).
- Surface remeshing (this PRD is volume-mesh morphing only — surface mesh comes from elsewhere and is an input).
- Topology-changing parameter responses (those genuinely need a remesh; this PRD only addresses the topology-preserving case).
- Nearest-cached morph for cross-session cold start (deferred to v0.4 follow-on PRD `mesh-morph-nearest-cached.md` stub, pending v0.3.x telemetry).
- Surface mesh / voxel grid / SDF / octree morphing (different operations entirely; this PRD is volume-mesh-only).
- GUI badge visualisation of morph events (tracked under the GUI rendering PRD).

## Relationship to other PRDs and tasks

- **Speeds up `structural-analysis-fea.md`** — cuts wallclock for slider/auto-resolve workflows by 10–100×; the single biggest interactive-smoothness lever, and preserves FEA solver warm-start state across parameter ticks (compounding the speedup).
- **Benefits future CFD / EM / CAM PRDs** — any mesh-consuming computation reuses the same morphing layer via the consumer-neutral `reify-mesh-morph` crate.
- **Composes with `persistent-fea-cache.md`** — morphed meshes are *not* written to the persistent cache (only from-scratch results are). The two layers are orthogonal: persistent cache covers cross-session exact hits; morph covers within-session incremental updates. The persistent-fea-cache PRD's earlier note about caching morphed meshes with morph provenance should be removed.
- **Hard-depends on `docs/prds/v0_2/persistent-naming-v2.md`** — Stage B's bijection check is the load-bearing primitive. Morph reliability is bounded by persistent-naming reliability.
- **Independent of `structural-analysis-shells.md`** — shell elements are a separate mesh kind with their own morphing concerns.
- **Future follow-on `mesh-morph-nearest-cached.md`** — deferred v0.4 PRD covering nearest-cached morph for cross-session cold start; activate once v0.3.x telemetry shows the cold-start case matters in practice.
- **Debug-MCP RPC seam-owned by `docs/prds/v0_3/gui-event-channel-inventory.md`** — the `morph_stats` Debug-MCP RPC (inventory §2.3 / Phase 3 task θ) surfaces live morph-engine stats via the debug bridge; upstream prereq chain is task 2949 → 2948 → 2947 (mesh-morph engine wiring). Emitter registration in `debug.rs` is decomposed in the inventory PRD. See also `docs/gui-event-channels.md`.

# PRD: Mesh-Morphing Phase 2 — Engine-Seam Bundle (vertex correspondence + producers + wire)

Status: authored 2026-05-13 as phase-2 sibling to `docs/prds/v0_3/mesh-morphing.md`. Bundles the four coupled engine-seam gaps that today leave the morph engine unreachable. Resolves GR-023 (cluster C-21) and the mesh-morph half of GR-017 (cluster C-14). Origin: 2026-05-12 investigate-further triage (`docs/architecture-audit/phase-3-investigate-further-triage-log.md`).

## §0 — Purpose and supersession

`docs/prds/v0_3/mesh-morphing.md` (committed b19af996dd, 2026-05-04) decomposed mesh-morph but under-scoped its load-bearing engine seams. The 2026-05-12 audit surfaced four coupled gaps whose only common resolution is a coordinated bundle:

- **GR-023 / M-002** — `CorrespondenceMap.vertex_to_vertex` structurally always-empty in v0.2; any P1 tet mesh with B-rep-vertex-attached surface nodes fails morph with `ProjectionFailure::MissingCorrespondence`. Six pinning assertions in `crates/reify-eval/src/morph_stage_b.rs` (lines 435, 470, 505, 749, 834) plus one named test at line 1058 actively assert the empty-bijection contract — a Phase-3 §3 new-pattern-A "active drift pin" that must be deliberately retired.
- **M-005** — `BoundaryAssociation` / `NodeAttachment` producer absent on the Gmsh adapter side. `mesh_surface_to_volume_with_diagnostics` emits `VolumeMesh` but doesn't record per-node B-rep attachment. Parent PRD task #5 spec'd the consumer side; no task was filed against the producer.
- **M-006** — Concrete OCCT-backed `Projector` trait impl absent. `crates/reify-mesh-morph/src/boundary.rs:161-182` defines the trait; only test-only `RecordingProjector` implements it. Real impl backed by `BRepExtrema_DistShapeShape` was deferred per the parent PRD as "follow-on task accompanying PRD task #10 or #7" — never filed.
- **Task 2947** — engine-wire integration gate; the audit (findings/mesh-morphing.md M-012) flagged this as **the** load-bearing gap. Pending; explicitly blocked by the three above.

**This PRD supersedes** the parent PRD's task #2 (Stage B) vertex arm, task #5 (boundary-node correspondence) `OnVertex` consumer wiring, the unfiled M-005/M-006 follow-ons referenced obliquely in parent §"Decomposition plan" tasks #7/#10, and re-points task 2947 (engine wire) via dependency extension. The parent PRD's algorithm tree (tasks #6 Laplacian, #7 elasticity, #8 stiffness, #9 quality, #11 telemetry, #13–#16 validation) is **inherited unchanged**.

Cross-PRD reference: `docs/prds/v0_2/persistent-naming-v2.md` owns the vertex-attribute-table widening per portfolio approach E (cross-PRD seam ownership). One sibling task in this PRD's decompose batch carries `owning_prd` metadata pointing at PNv2; the cross-PRD `add_dependency` edge from engine-wire 2947 to the PNv2-owned task is a real edge per [[preferences_cross_prd_deps_real_edges]].

## §1 — GR-023 + cross-PRD chain

- **GR-023 (cluster C-21).** Disposition refined to "PRD authored — `docs/prds/v0_3/mesh-morphing-phase-2.md`"; see `docs/architecture-audit/gap-register.md`.
- **GR-017 (cluster C-14).** The mesh-morph slice of the engine-integration norm is naturally resolved when this bundle's task ε lands. The norm itself (`docs/prds/v0_3/engine-integration-norm.md`) is the broader resolution; this PRD's tasks plug into §3.2 (realization-kind dispatch — VolumeMesh's morph-or-remesh branch) and §3.4 (ComputeNode dispatch — already covered by parent PRD task #10's metadata-files list).
- **PNv2 (v0.2).** Task α widens its attribute table to vertices symmetrically. PNv2 PRD remains the owning PRD per portfolio approach E; the task is filed in this PRD's batch for sibling-wave dependency wiring per [[procedural-planning-mode-queueing]].

## §2 — Resolved design decisions (Q-MM2-1 … Q-MM2-8 from the session prompt)

### Q-MM2-1 — PNv2 widening shape: **symmetric extension**

PNv2's `TopologyAttributeTable` is keyed by `GeometryHandleId` and is structurally agnostic to sub-shape kind (`crates/reify-types/src/geometry.rs:1963-2006`). Vertex widening therefore:

1. Adds `BRepKind::Vertex` to the enum at `geometry.rs:54-71` (next to existing `Edge` / `Face`).
2. Adds `OcctKernel::extract_vertices(handle) -> Result<Vec<GeometryHandleId>, QueryError>` mirroring the existing `extract_edges` / `extract_faces` shape (`handle.rs:293-318`). Each extracted vertex is registered with `BRepKind::Vertex`.
3. Extends the per-op attribute populators (`crates/reify-eval/src/topology_attribute_propagation.rs` for sweeps; `crates/reify-eval/src/primitive_attribute_seed.rs` for primitives) to seed vertex attributes alongside the existing face/edge seeding.

**Tag generation rule.** Identical to the existing face/edge derivation: `(feature_id, Role, local_index)` with `Role` extended by per-op vertex variants. Concretely:

| Op | New `Role` variants for vertices |
|---|---|
| Extrude / Revolve / Sweep / Loft | `CapCornerVertex { face: CapFace }` (top/bottom cap corners) |
| Box primitive | `CornerVertex { x_face: ±X, y_face: ±Y, z_face: ±Z }` |
| Cylinder / Sphere | none today (no zero-dim B-rep vertices on smooth analytic primitives) |
| Mid-surface (cross-PRD) | inherits the existing `MidSurfaceEdge` / `MidSurfaceFace` discipline; vertex extension is out of scope for shells |

Deterministic `local_index` ordering follows PNv2's existing rule (centroid → normal → construction-order tiebreak — see PNv2 §"Local-index ordering").

### Q-MM2-2 — Cross-PRD seam ownership: **approach E, sibling task in this batch**

Per portfolio approach E. PNv2 PRD owns the vertex-side attribute table conceptually (the data structure lives in PNv2's mental model), but the vertex-widening task is filed in *this* PRD's planning_mode bulk-flip wave. Rationale:

- Avoids the two-stage queueing hazard that [[procedural-planning-mode-queueing]] is built to prevent. Filing the PNv2-owned task in a separate later decompose pass would leave engine-wire 2947 blocked on a task not-yet-in-the-DB.
- Cross-PRD `add_dependency` edges are real scheduler-visible edges per [[preferences_cross_prd_deps_real_edges]] (rule reversed 2026-05-12). Metadata-only `cross_prd_dep` is informational only.
- PNv2 PRD's eventual re-decomposition can absorb the vertex-widening task as `done` via [[preferences_supersession_same_prd_only]] — same-PRD supersession on cancellation is well-defined.

Task α (PNv2 vertex widening) carries `metadata.owning_prd = "docs/prds/v0_2/persistent-naming-v2.md"` and `metadata.consumer_ref = "docs/prds/v0_3/mesh-morphing-phase-2.md §3.1"` so the relationship is visible at filing time.

### Q-MM2-3 — Producer/consumer test discipline (H boundary tests): **each crate owns its side + one engine-level e2e**

See §7 for the full test sketch. Distribution:

- Producer-side (Gmsh `NodeAttachment` shape): `crates/reify-kernel-gmsh/tests/node_attachment_producer.rs`
- Consumer-side (`compute_dirichlet_bcs::OnVertex` resolves): `crates/reify-mesh-morph/tests/boundary_on_vertex.rs`
- OCCT Projector impl: `crates/reify-kernel-occt/tests/projector_impl.rs` (extending the established pattern at `tests/closest_point_on_shape_integration.rs` and `tests/point_on_shape_integration.rs` — both already exercise `BRepExtrema_DistShapeShape` against sub-shape kinds)
- Engine-level end-to-end: `crates/reify-eval/tests/morph_e2e_with_vertex_bcs.rs`

Pattern mirrors `compute-node-contract.md` §7 and `shell-extract-engine-bridge.md` §8. The two-way producer/consumer split is essential for triaging regressions in isolation — a centralized integration suite collapses the two failure modes into a single signal.

### Q-MM2-4 — OCCT Projector spec: **one FFI shape, three thin wrappers**

`BRepExtrema_DistShapeShape(shape, point_as_vertex)` returns the closest point on `shape` regardless of `shape`'s sub-shape kind. Existing OCCT-integration tests (`closest_point_on_shape_integration.rs:81-220`, `point_on_shape_integration.rs:119-380`) demonstrate the pattern works for `TopoDS_Solid`, `TopoDS_Face`, `TopoDS_Edge`, `TopoDS_Vertex`, `TopoDS_Wire`.

The Projector impl is therefore three method arms wrapping the same FFI call with the appropriate `TopoDS_Shape` parameter:

| Trait method | OCCT call | Sub-shape passed |
|---|---|---|
| `project_onto_face(face, point)` | `BRepExtrema_DistShapeShape(face_topods, vertex_from_point).PointOnShape1()` | `TopoDS_Face` (looked up via `kernel.get_handle(face)`) |
| `project_onto_edge(edge, point)` | same | `TopoDS_Edge` |
| `vertex_position(vertex)` | direct vertex coordinates (no closest-point — vertex projection is a snap) | `TopoDS_Vertex` |

`vertex_position` short-circuits the FFI call entirely: the v0.2 `BRepKind::Vertex` registration (per task α) means the kernel already holds the `TopoDS_Vertex`; its coordinates are read directly via `BRep_Tool::Pnt`. This matches the `boundary.rs:177-181` doc-comment ("the old node position is intentionally not passed — vertex projection is a snap to the new vertex's exact coordinates, not a closest-point computation").

### Q-MM2-5 — Active drift pin: **task β explicitly deletes the pinning tests**

Six assertions in `crates/reify-eval/src/morph_stage_b.rs` actively pin the always-empty contract:

| Line | Assertion |
|---|---|
| 435 | `map.vertex_to_vertex.is_empty(), "vertex_to_vertex must be empty for empty input"` |
| 470 | `map.vertex_to_vertex.is_empty(), "vertex_to_vertex must be empty"` |
| 505 | same |
| 749 | same |
| 834 | same |
| 1058 (named test) | `stage_b_eligible_vertex_to_vertex_is_always_empty_in_v0_2` |

These are Phase-3 synthesis §3 new-pattern-A "active drift pin" — same shape as GR-043's `cost_per_byte_does_not_alter_lru_eviction_order` (resolved by deliberate test replacement). Task β's body **explicitly** declares:

1. **DELETE** the six `is_empty()` assertions at the listed lines.
2. **DELETE** the named test at line 1058 (which is unambiguously documenting the v0.2 contract).
3. **ADD** new positive test `stage_b_eligible_populates_vertex_to_vertex_when_vertex_attrs_present` (and a complementary `_handles_count_mismatch_for_vertices` plus `_handles_unmapped_vertex` per the existing face/edge test pattern).
4. The task description calls out the deliberate contract change so reviewers don't gate on the deleted tests (avoid `feedback_verify_review_field_existence`-shape regressions where a reviewer flags the missing `.is_empty()` assertion).

The doc-comment at `morph_stage_b.rs:38-46` (which calls `vertex_to_vertex` "always empty in v0.2") is also rewritten as part of task β.

### Q-MM2-6 — Engine-wire integration gate: **existing task 2952 already captures the user-observable signal**

Parent PRD's task #15 → fused-memory task **2952** ("Mesh morphing: FEA warm-start preservation regression test", confirmed via `get_task(2952)`). Its existing spec:

> Assert (a) warm-start state survives morph (element-to-DOF mapping stays valid), (b) per-tick CG iteration count is materially lower with morphing enabled

is already the correct user-observable signal for the bundle. Its dependency on 2947 means when this PRD's task ε (which extends 2947's deps) lands, 2952 becomes runnable end-to-end. **No spec extension needed.** Task 2952 stays as-authored.

The engine-level e2e test (`reify-eval/tests/morph_e2e_with_vertex_bcs.rs`) is a narrower per-tick correctness check; 2952 is the warm-start-preservation signal that closes the bundle's headline claim ("FEA warm-start state preserved across parameter ticks").

### Q-MM2-7 — Compatibility with existing mesh-morphing PRD tasks

The parent PRD's 16 numbered sections map to fused-memory task IDs as follows (per parent PRD §"Decomposition plan" + landed-task evidence):

| Parent § | Task ID | State | Phase-2 disposition |
|---|---|---|---|
| #1 Stage A classifier | 2937 (assumed) | done | Unaffected. |
| #2 Stage B + CorrespondenceMap | 2939 | **done** | **Revised in place** via task β. The PRD body of 2939 is unchanged; β adds the vertex-arm fill + drift-pin retirement. Done-state stays. |
| #3 Combined eligibility | 2940 | done | Unaffected. |
| #4 Crate skeleton | 2941 | done | Unaffected. |
| #5 compute_dirichlet_bcs | 2942 | done | Consumer-side `OnVertex` arm was correctly anticipated (`boundary.rs:212-276`); becomes reachable once β + γ + δ land. No revision. |
| #6 Laplacian quick-pass | 2943 | done | Unaffected. |
| #7 Elasticity morph | 2944 | done | Unaffected. |
| #8 Spatially-varying stiffness | 2945 | done | Unaffected. |
| #9 Quality check | 2946 | done | Unaffected. |
| #10 Engine wiring | **2947** | **pending** | **Re-pointed** via task ε: 2947's dep list extended with {α, β, γ, δ}. No content rewrite. |
| #11 Diagnostic counters | 2948 | pending | Gated on 2947; unaffected. |
| #12 Debug RPC | 2949 | pending | Gated on 2948; unaffected. |
| #13 Calibration | 2950 | done | Unaffected. |
| #14 Chain degradation | 2951 | pending | Gated on 2947; unaffected. |
| #15 Warm-start regression | **2952** | pending | Confirmed unchanged (Q-MM2-6). |
| #16 Slider benchmark | 2953 | pending | Gated on 2947; unaffected. |

Net diff: **two existing tasks touched** (β amends 2939's vertex arm; ε re-points 2947's deps), **four new tasks filed** (α PNv2 widening, β Stage-B vertex fill, γ Gmsh producer, δ OCCT Projector). No task cancellations; no task content rewrites — only dep-list extensions.

### Q-MM2-8 — Resolution mode: **portfolio approach E + H**

Confirmed: **E** (cross-PRD seam ownership — PNv2 owns task α's data structure; this PRD owns the consumer wiring) + **H** (design-first interface contracts + two-way boundary tests per §7). The PRD body resolves all open design questions before any task is filed; boundary tests in each crate ensure the producer/consumer contracts are pinned facing both ways.

## §3 — Scope

Five tasks bundled under one coordinated DAG (§8). Four new, one re-pointed via dependency extension.

### 3.1 PNv2 vertex-side attribute widening (task α, PNv2-owned)

Extend PNv2's attribute table to vertices symmetrically (§2 Q-MM2-1). Concrete deliverables:

- `BRepKind::Vertex` variant added to `crates/reify-types/src/geometry.rs:54-71`.
- `OcctKernel::extract_vertices(handle)` mirroring `extract_edges` / `extract_faces` at `crates/reify-kernel-occt/src/handle.rs:293-318` and on the stub at `src/stubs.rs:80-95`. Each vertex registered with `BRepKind::Vertex`.
- `Role::CornerVertex` (box / extrude / revolve / sweep / loft cap corners) — covers the cases needed for v0.3 mesh-morph fixtures. Cone / Torus / Tube vertex seeding is deferred per parent PNv2 audit M-008 (independent gap).
- Per-op attribute population extended in `topology_attribute_propagation.rs` and `primitive_attribute_seed.rs` to emit vertex attributes alongside face/edge attributes.
- Stage-B's `stage_b_eligible` `_old_vertices` / `_new_vertices` parameters (already accepted at `morph_stage_b.rs:152-153` as forward-compat placeholders) become live inputs in task β.

### 3.2 Stage-B vertex bijection fill (task β)

`stage_b_eligible` populates `CorrespondenceMap.vertex_to_vertex` by matching `TopologyAttribute` records across the vertex slices. Algorithm reuses `match_one_kind` (the same private helper that handles faces/edges at `morph_stage_b.rs:182-280`); no new matching code.

Active drift pin retirement per Q-MM2-5: delete the six pinning assertions and the named v0.2-always-empty test; add positive vertex-bijection tests mirroring the existing face/edge test pattern.

Doc-comment at `morph_stage_b.rs:38-46` rewritten to describe the now-populated vertex slot.

### 3.3 Gmsh `NodeAttachment` producer (task γ)

`mesh_surface_to_volume_with_diagnostics` at `crates/reify-kernel-gmsh/src/mesh_volume.rs:161` returns `(VolumeMesh, BoundaryAssociation)` instead of `VolumeMesh` alone (or extends its existing return struct with a `boundary: BoundaryAssociation` field — exact API shape decided at task implementation time, see §9 Q-9-2).

Per-node B-rep attachment threads through:

1. Surface mesh enters Gmsh with per-triangle / per-vertex face-id and edge-id metadata (Gmsh's surface mesher already attributes mesh entities to OCCT sub-shapes — that's how `face_id` propagates today for FEA's BC application).
2. The volume tet meshing step preserves this surface-node attribution; interior tet nodes have no attachment (correctly omitted from `BoundaryAssociation`).
3. Producer emits `(node_index, NodeAttachment::OnFace | OnEdge | OnVertex(handle))` pairs into a `BoundaryAssociation`, with `BTreeMap` iteration order preserved for warm-start bit-stability per `boundary.rs:51-58`.

For vertex attachment specifically: nodes coincident (within mesh tolerance) with a B-rep vertex are emitted as `OnVertex`; this requires the OCCT side to expose vertex coordinates so Gmsh can do the snap-to-vertex test. Falls out of task α's `extract_vertices` + the existing `BRep_Tool::Pnt` FFI.

### 3.4 OCCT `Projector` impl (task δ)

`OcctProjector { kernel: OcctKernel }` (or `Arc<OcctKernel>` if lifetime analysis shows the kernel doesn't outlive the morph — see §9 Q-9-3) in `crates/reify-kernel-occt/src/projector_impl.rs` (new file). Impls `reify_mesh_morph::Projector`:

```rust
impl reify_mesh_morph::Projector for OcctProjector {
    fn project_onto_face(&self, face: GeometryHandleId, point: [f64; 3])
        -> Result<[f64; 3], ProjectorPayload> { /* BRepExtrema_DistShapeShape(face, point) */ }
    fn project_onto_edge(&self, edge: GeometryHandleId, point: [f64; 3])
        -> Result<[f64; 3], ProjectorPayload> { /* same with edge */ }
    fn vertex_position(&self, vertex: GeometryHandleId)
        -> Result<[f64; 3], ProjectorPayload> { /* BRep_Tool::Pnt; no projection */ }
}
```

FFI is already in the workspace (per the grep evidence at `crates/reify-kernel-occt/src/stubs.rs:171` and the test files); δ is wiring + test work, not new FFI.

### 3.5 Engine wire — task 2947 dependency extension (task ε)

Task 2947 ("VolumeMesh realization wiring — morph-or-remesh") exists, is pending, and has its body correctly authored. Phase-2 amends only its dependency list via `update_task`:

- **Add deps:** α (PNv2 vertex widening), β (Stage-B vertex fill), γ (Gmsh producer), δ (OCCT Projector).
- **No content rewrite.** The existing description correctly describes the morph-or-remesh dispatch; once {α, β, γ, δ} are done it becomes implementable.

When 2947 fires, the engine-wire constructs `BoundaryAssociation` from γ's producer output, `CorrespondenceMap` (with vertex_to_vertex populated per β) from `stage_b_eligible`, and `OcctProjector` (from δ) bound to the engine's `Engine.kernel` handle. Passes to `compute_dirichlet_bcs` → elasticity / Laplacian morph → quality check → cache. Task 2952 (warm-start regression) becomes the headline user-observable signal.

## §4 — Out of scope for phase 2

- **Algorithm changes** (Laplacian #6, elasticity #7, stiffness #8, quality #9, telemetry #11, RPC #12, calibration #13). Inherited unchanged from parent PRD.
- **Cone / Torus / Tube vertex seeding.** Per parent PNv2 audit M-008, primitive vertex-seeding coverage for non-box primitives is an independent gap; phase 2 covers only what the morph-fixture set demands (boxes + extrude/revolve/sweep/loft cap corners).
- **Manifold `KernelAttributeHook` vertex MeshGL walk.** PNv2 M-018 stub state. Cross-kernel vertex attribute preservation through Manifold remains FICTION; phase 2 limits scope to OCCT-only vertex production.
- **Surface-mesh vertex snapping tolerance design.** §3.3's "within mesh tolerance" snap-to-vertex rule uses the existing per-purpose tolerance machinery (per the parent PRD §"Pre-conditions"). No new tolerance knob.
- **Nearest-cached morph for cold start.** Still deferred to parent PRD's v0.4 follow-on `mesh-morph-nearest-cached.md`.

## §5 — Pre-conditions for activation

All inherited from parent PRD plus one new:

- v0.3 FEA kernel shipped (parent PRD precondition; satisfied).
- `ReprKind::VolumeMesh` variant landed (parent task #17 / 2925; done).
- PNv2 v0.2 shipped (parent precondition; the table-widening of task α is an additive extension to the shipped table, not a rewrite — does not reopen PNv2's v0.2 done state).
- Per-purpose tolerance live (parent precondition).
- **(NEW) Selector vocabulary v2 not required.** Task α's vertex attribute seeding is symmetric with the existing face/edge seeding and does not require any v2 selector to be live — v0.1 attribute-based lookup is sufficient (consumed by `stage_b_eligible` directly, not via surface DSL).

## §6 — Cross-PRD relationship + seam-owner table

| Sub-mechanism | Task | Owner PRD | Consumer | Cross-link |
|---|---|---|---|---|
| PNv2 vertex attribute widening | α | persistent-naming-v2 (v0.2) | this PRD §3.1 + Stage-B's vertex inputs | M-002 vertex_to_vertex; PNv2 M-008 (partial overlap) |
| Stage-B vertex bijection fill | β | mesh-morphing phase-2 (this) | `compute_dirichlet_bcs::OnVertex` arm | finds-out: GR-023 disposition |
| Gmsh `NodeAttachment` producer | γ | mesh-morphing phase-2 (this) | engine-wire 2947 (via `BoundaryAssociation`) | M-005 |
| OCCT `Projector` impl | δ | mesh-morphing phase-2 (this) | engine-wire 2947 (via trait obj) | M-006 |
| Engine wire (re-pointed) | ε (= 2947) | mesh-morphing (parent) | FEA stack via task 2952 warm-start regression | M-012 + GR-017 engine-integration-norm §3.2 |

Engine-integration-norm conformance (§5 G1 checklist of `engine-integration-norm.md`): all four new tasks name their consumer (G1 ✓); all five terminate in user-observable behaviour per §8 (G2 ✓); none introduce new `.ri` grammar (G3 N/A); cross-PRD seam α is owned by PNv2 per metadata + real dep edge (G4 ✓); H boundary tests per §7 (G5 ✓).

## §7 — Boundary-test sketch (two-way, per Q-MM2-3)

Four test files, one per producer/consumer side plus the engine-level e2e.

### §7.1 Producer-side (Gmsh `NodeAttachment` shape)

`crates/reify-kernel-gmsh/tests/node_attachment_producer.rs`:

- Given an OCCT B-rep box with named 6 faces, 12 edges, 8 corner vertices (via task α's `extract_vertices`).
- Call `mesh_surface_to_volume_with_diagnostics`. Expect `(VolumeMesh, BoundaryAssociation)`.
- Assert: every surface-mesh node has exactly one `NodeAttachment` entry; interior tet nodes have none.
- Assert: 8 corner nodes attach as `OnVertex(corner_handle_i)` with distinct handles per corner.
- Assert: edge-interior nodes attach as `OnEdge(edge_handle_i)`; face-interior as `OnFace(face_handle_i)`.
- Assert: `BoundaryAssociation::iter()` yields ascending-`node_index` order (warm-start bit-stability per `boundary.rs:51-58`).

### §7.2 Consumer-side (`compute_dirichlet_bcs::OnVertex` resolves)

`crates/reify-mesh-morph/tests/boundary_on_vertex.rs`:

- Hand-construct `CorrespondenceMap` with populated `vertex_to_vertex: {old_v0 → new_v0}` and `BoundaryAssociation` with `OnVertex(old_v0)` for a single node index.
- Mock `Projector` (extends `RecordingProjector`) returns hard-coded `[1.0, 2.0, 3.0]` for `vertex_position(new_v0)`.
- Call `compute_dirichlet_bcs`; assert returned BC is `(node_idx, [1.0, 2.0, 3.0])` — i.e. **not** `Err(ProjectionFailure::MissingCorrespondence)`.
- Negative companion: `OnVertex(handle_not_in_map)` still produces `MissingCorrespondence` (the diagnostic is still load-bearing for genuinely missing entries).

### §7.3 OCCT Projector

`crates/reify-kernel-occt/tests/projector_impl.rs`:

- Construct `OcctProjector` over a real `OcctKernel` with a box `TopoDS_Solid`.
- For each of: a known face's `project_onto_face`, a known edge's `project_onto_edge`, a known vertex's `vertex_position` — query with a fixed off-shape point (or no point, for vertex).
- Assert: returned point matches the expected `BRepExtrema_DistShapeShape` semantics established by the existing tests at `tests/closest_point_on_shape_integration.rs:81-220`.
- Assert: querying a stub-kernel handle returns `Err(ProjectorPayload { message: "kernel returned error: ..." })` with the OCCT error text preserved.

### §7.4 Engine-level end-to-end

`crates/reify-eval/tests/morph_e2e_with_vertex_bcs.rs`:

- Parametric `.ri` fixture: thin box with a `thickness` param driving extrude height; corners are explicit B-rep vertices.
- Realize at `thickness = 1.0`; mesh; tick to `thickness = 1.05`; call morph through 2947's wiring path.
- Assert: morph succeeds (returns `Ok(VolumeMesh)`, not `MorphFailure::ProjectionFailure(MissingCorrespondence)`).
- Assert: each of the 8 corner nodes in the morphed mesh is located at exactly the new B-rep vertex coordinates (within `MorphOptions.position_tolerance`).
- Assert: quality check passes (`QualityVerdict::Pass`).
- This is the engine-level integration test the parent PRD's task #5 anticipated but couldn't reach.

## §8 — Decomposition plan (vertical-slice DAG; per-leaf user-observable signal)

Five tasks. All terminate in user-observable behaviour per [[feedback_task_chain_user_observable]] (D-discipline).

### Task α — PNv2 vertex-side attribute widening (PNv2-owned)

- `BRepKind::Vertex` variant; `OcctKernel::extract_vertices` impl + stub; per-op attribute population extended (extrude / revolve / sweep / loft / box).
- New `Role::CornerVertex` (variant shape decided at impl time per §3.1).
- Vertex-attribute seeding wired through `engine_build.rs` realization-ops path symmetrically with face/edge seeding.
- **User-observable signal:** new test `extract_vertices_and_attribute_seeding_box` (in `crates/reify-eval/tests/topology_attribute_vertex_seeding.rs`) asserts:
  - `extract_vertices(box_handle).len() == 8`
  - Each vertex carries a `TopologyAttribute` entry with `Role::CornerVertex` and stable `(feature_id, local_index)` across a `thickness`-param tick
- Dependencies: none (foundation of the bundle).
- Metadata: `consumer_ref = "docs/prds/v0_3/mesh-morphing-phase-2.md §3.1"`, `owning_prd = "docs/prds/v0_2/persistent-naming-v2.md"`, `grammar_confirmed = true` (no new DSL syntax).

### Task β — Stage-B vertex bijection fill + drift-pin retirement

- `stage_b_eligible` processes the (already-accepted) vertex slices and populates `vertex_to_vertex`.
- DELETE 6 pinning assertions at `morph_stage_b.rs:{435,470,505,749,834}` (count comprehensive; if any line shifts after rebase, the locations remain identifiable by the assertion text `"vertex_to_vertex must be empty"`).
- DELETE named test `stage_b_eligible_vertex_to_vertex_is_always_empty_in_v0_2` at line 1058.
- ADD positive tests: `stage_b_eligible_populates_vertex_to_vertex_when_vertex_attrs_present`, `_handles_count_mismatch_for_vertices`, `_handles_unmapped_vertex` (mirroring face/edge test pattern).
- REWRITE doc-comment at `morph_stage_b.rs:38-46` to describe the now-populated vertex slot; cite §3 of this PRD.
- **User-observable signal:** new positive test passes; old `_is_always_empty_in_v0_2` test no longer exists in the codebase (Phase-3 §3 new-pattern-A intentional contract change; task body explicitly calls this out so reviewers don't gate on the removed test).
- Dependencies: **α**.
- Metadata: `consumer_ref = "reify-mesh-morph::boundary::compute_dirichlet_bcs"`, `owning_prd = "docs/prds/v0_3/mesh-morphing-phase-2.md"`, `grammar_confirmed = true`.

### Task γ — Gmsh `NodeAttachment` producer

- `mesh_surface_to_volume_with_diagnostics` returns `BoundaryAssociation` alongside `VolumeMesh` (API extension; existing callers updated).
- Surface-mesh path threads per-node B-rep attachment; corner-vertex snap-to-vertex test uses task α's `BRep_Tool::Pnt` access.
- **User-observable signal:** §7.1 boundary test passes — 8 corner nodes attach as `OnVertex`, edge-interior as `OnEdge`, face-interior as `OnFace`; iteration is `BTreeMap`-ordered.
- Dependencies: **α** (needs vertex handles to attach to).
- Metadata: `consumer_ref = "reify-mesh-morph::BoundaryAssociation"`, `owning_prd = "docs/prds/v0_3/mesh-morphing-phase-2.md"`, `grammar_confirmed = true`.

### Task δ — OCCT `Projector` impl

- New file `crates/reify-kernel-occt/src/projector_impl.rs` (or co-located in `handle.rs`).
- `OcctProjector { kernel: OcctKernel }` implementing `reify_mesh_morph::Projector` per §3.4.
- All three methods backed by the existing `BRepExtrema_DistShapeShape` FFI; `vertex_position` short-circuits via direct vertex coordinates.
- **User-observable signal:** §7.3 boundary test passes — closest-point queries on face / edge / vertex sub-shapes match the BRepExtrema semantics pinned by existing OCCT integration tests.
- Dependencies: none (independent of α/β/γ; FFI shipped). Note: δ can land before α/β/γ are wired into 2947 — the Projector impl is testable in isolation against synthetic OCCT handles.
- Metadata: `consumer_ref = "reify-mesh-morph::Projector + engine-wire 2947"`, `owning_prd = "docs/prds/v0_3/mesh-morphing-phase-2.md"`, `grammar_confirmed = true`.

### Task ε — Engine-wire dep extension (no new task; update_task on 2947)

- This is **not** a new task. Phase-2's decompose pass calls `update_task(2947, dependencies = existing + {α, β, γ, δ})`.
- 2947's body is untouched. Its existing test plan / metadata.files list / spec all stand.
- **User-observable signal:** when 2947 lands, `morph()` is reachable end-to-end on the FEA bracket fixture; task 2952 ("FEA warm-start preservation regression") becomes the bundle's headline acceptance signal.
- Dependencies: **2947's existing list extended with {α, β, γ, δ}**.

### DAG

```
α (PNv2 vertex widening, PNv2-owned)
├──▶ β (Stage-B vertex fill + drift-pin retirement)
└──▶ γ (Gmsh NodeAttachment producer)

δ (OCCT Projector impl) — independent

β ─┐
γ ─┼──▶ 2947 (engine wire; deps extended via update_task) ──▶ 2952 (warm-start regression)
δ ─┘
α ─┘   (α also a direct dep — vertex_to_vertex consumer)
```

Per [[procedural-planning-mode-queueing]]: file α/β/γ/δ in planning_mode batch → wire α→β, α→γ intra-batch deps → wire β/γ/δ/α → 2947 cross-batch deps via `add_dependency(2947, X)` → bulk flip α/β/γ/δ pending. 2947 stays pending (was already); the new deps gate it correctly. No commit step (fused-memory owns persistence).

## §9 — Open (tactical) questions

These are implementation-time questions, not design-time blockers — they get resolved during the corresponding task's implementation phase.

**Q-9-1. Vertex-seed primitive coverage.** §2 Q-MM2-1 lists box / extrude / revolve / sweep / loft cap corners. Cylinder/sphere have no B-rep vertices on the smooth analytic surface, but cylinders *do* have edge endpoints where the cap circle meets the axis (or rather: where the seam meets the cap). Task α's impl decides whether those count as vertices for our purposes; the existing PNv2 audit M-008 already flagged Cone/Torus gaps, so any further extension is sequenced after this bundle.

**Q-9-2. `BoundaryAssociation` return-shape API.** §3.3 leaves open whether `mesh_surface_to_volume_with_diagnostics` returns a tuple `(VolumeMesh, BoundaryAssociation)` or extends its existing return struct `MeshSurfaceToVolumeOutput` with a `boundary: BoundaryAssociation` field. Decided at task γ implementation time based on existing consumer-call-site impact. Either way, callers of `mesh_surface_to_volume_with_diagnostics` need a one-line update.

**Q-9-3. `OcctProjector` kernel lifetime.** §3.4 left open whether `OcctProjector` holds `OcctKernel` by value, `&OcctKernel`, or `Arc<OcctKernel>`. The engine holds `Engine.kernel` for the realization's lifetime, which exceeds the morph call — so by-value (cloning the lightweight handle to the kernel thread) or `&'a OcctKernel` likely both work. Decided at task δ implementation time.

**Q-9-4. Producer corner-snap tolerance.** When Gmsh emits a surface mesh, corner nodes land *exactly* on B-rep vertices in the analytic case but may drift slightly under aggressive mesh-density variation. §3.3 punts to "per-purpose tolerance machinery" (parent PRD's precondition). Task γ implementation must pick a concrete tolerance binding; safest default is the surface-mesh's own discretization tolerance (which Gmsh already maintains).

---

## Cross-references

- **Parent PRD:** `docs/prds/v0_3/mesh-morphing.md`
- **Cross-PRD seam owner:** `docs/prds/v0_2/persistent-naming-v2.md`
- **Engine-integration norm (G1 conformance):** `docs/prds/v0_3/engine-integration-norm.md` §3.2 + §5
- **Gap register entries:** GR-023 (resolution mechanism = this PRD), GR-017 (mesh-morph half resolves naturally when ε lands)
- **Audit findings:** `docs/architecture-audit/findings/mesh-morphing.md` M-002 / M-005 / M-006 / M-012; `findings/persistent-naming-v2.md` M-001 / M-002
- **Triage origin:** `docs/architecture-audit/phase-3-investigate-further-triage-log.md` (GR-023 row, 2026-05-12)
- **Session prompt:** `docs/architecture-audit/gr023-mesh-morph-prd-revisit-session-prompt.md` (this PRD's authoring brief)

# Kernel Geometry Queries

Status: contract (follow-up to `docs/prds/v0_3/geometry-handle-runtime.md` / GR-030; decomposed and committed 2026-05-14). Authored 2026-05-14 in interactive session. Pending Leo approval before queueing tasks.

Resolves the v0.3 portion of the 11 deferred eval-side topology-selector names (task 2691 phantom-done drift) plus the broader geometry-query corpus that GR-030 Phase 6 does not cover. Resolution mode is **B + H** per `preferences_implementation_chain_portfolio`: contract section (§4–§7) + boundary-test sketch (§8) facing both producer (kernel adapter) and consumer (`.ri` user) sides.

## §0 — Purpose and supersession

The 2026-05-14 audit-derived state: of ~25 geometry-query helper names registered in `docs/reify-stdlib-reference.md` §3.9 and adjacent, only three (`closest_point`, `is_on`, `angle_between_surfaces` per task 2324) have full eval-side dispatch. GR-030 Phase 6 (task 3608, GHR-ζ) adds OCCT dispatch for four-to-six more (`volume`, `centroid`, `area`, `bounding_box`; stretch `length`, `perimeter`). Every other registered name — including the entire task-2699 topology-selector batch — falls through `_ => return None` at `try_eval_topology_selector` (geometry_ops.rs:1705) to `Value::Undef` at runtime.

**This PRD supersedes the deferred eval-side scope of `docs/prds/topology-selectors.md` and cancels its phantom-done task 2691.**

Task 2691 (`done`, provenance commit `b457cdb8511`) is genuine drift: that commit added 13 lines of test-scaffolding to the `#[ignore]`-gated `fillet_top_edges` eval test, NOT the actual dispatch arms. The dispatcher comments at `crates/reify-eval/src/geometry_ops.rs:1655-1661` still flag every one of the 11 names as `(task 2691)` pending. Confirmed via `git show b457cdb8511` (15 lines, single file: `topology_selector_smoke_tests.rs`).

Cancellation + supersession actions, executed at decompose time:

- `set_task_status(2691, done → cancelled, reopen_reason="superseded by docs/prds/v0_3/kernel-geometry-queries.md Phase 2/3; phantom-done provenance from test-scaffold-only commit b457cdb8511")`.
- `docs/prds/topology-selectors.md` gets a §0 supersession block at this PRD's Phase 7 doc-update task pointing at this file. The PRD's other tasks (1–6, 8: feature-tag scheme, OCCT FFI work, stdlib bindings) remain done; this PRD inherits that work.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (`preferences_implementation_chain_naming`) — is exactly what task 2691 exhibits and what this PRD prevents repeating: producer-side machinery (OCCT FFIs, compile-time registrations) shipped without the eval-side dispatch that closes the chain to a `.ri`-callable user-observable signal.

## §1 — Goal and user-observable surface

After this PRD ships, every helper name in scope (§2) is callable from `.ri` source and evaluates to a non-Undef typed Value. Concretely:

- `distance(box(10mm,20mm,30mm), point3(20mm,0,0))` evaluates to `Scalar<Length>(15mm)` — analytic distance from a 10×20×30mm box (centred at origin) to a point 20mm along +X.
- `contains(box(10mm,10mm,10mm), point3(0,0,0))` evaluates to `Bool(true)`; `contains(box(10mm,10mm,10mm), point3(20mm,0,0))` evaluates to `Bool(false)`.
- `edges(box(10mm,20mm,30mm))` evaluates to a `List<Geometry>` of 12 sub-handles (one per box edge); `len(edges(box(...)))` returns `Int(12)`.
- `moment_of_inertia(box(50mm,30mm,10mm), 7850.0)` evaluates to a `Tensor<2,3,MomentOfInertia>` matching the analytic `(1/12) m (W² + H²)` etc. tensor. (Density arg is a bare numeric Real in v0.3 — `resolve_density_arg` (geometry_ops.rs) rejects a dimensioned Scalar with a Warning; compound-unit literals such as `7850kg/m^3` now parse per spec §2.7 (docs/prds/unit-expressions.md), but the moment_of_inertia density argument accepts only a bare Real — this is a v0.3 dispatch contract, not a grammar limitation.)
- `curvature(sphere(5mm), point3(5mm,0,0))` evaluates to `Scalar<Curvature>(1/5mm)`.

Each of these is exercised by an `.ri` example file in `examples/kernel_queries/` plus an integration test pinning the analytic expected value against the kernel's reply.

## §2 — Scope

**In scope.** Eval-side kernel dispatch for these helper names (each currently registered compile-time, but returning `Value::Undef` at eval):

**Measurement / property queries (GR-030 GHR-α registers; not in GR-030 Phase 6):**

| Helper | Signature | OCCT FFI | Manifold capability |
|---|---|---|---|
| `distance` | `<G1,G2>(a:G1, b:G2) → Scalar<Length>` | `min_clearance` (BRepExtrema_DistShapeShape) — shipped | yes (mesh-to-mesh / point-to-mesh) |
| `contains` | `(Solid, Point3<Length>) → Bool` | **new**: BRepClass3d_SolidClassifier wrapper | yes (raycast inside/outside) |
| `intersects` | `(Geometry, Geometry) → Bool` | `shapes_intersect` — shipped | yes |
| `geo_equiv` | `(Geometry, Geometry, Length) → Bool` | **new**: BRep topology hash + sampled vertex tolerance (§5.1) | yes (mesh-topology + vertex sample) |
| `angle` | `(Vector3<D>, Vector3<D>) → Angle` | pure math; no kernel | n/a |
| `normal` | `(Surface, Point3<Length>) → Vector3<D>` | `surface_normal_at` — shipped | yes at vertices / face barycenters |
| `curvature` (Curve) | `(Curve, Point3<Length>) → Scalar<Curvature>` | needs Curve-form `curvature_at` (today face-only) | **unsupported** (declare via capability flag) |
| `curvature` (Surface) | `(Surface, Point3<Length>) → Matrix<2,2,Curvature>` | `curvature_at` returns Curvature struct — wire matrix form | **unsupported** |
| `length` (Curve) | `(Curve) → Scalar<Length>` | `GeometryQuery::EdgeLength` shipped (sub-handle case); compose for multi-edge Curve | **unsupported** (Manifold has no curves) |
| `perimeter` (Surface) | `(Surface) → Scalar<Length>` | sum of `EdgeLength` over the face's edge loop | **unsupported** |

**Topology selectors (task 2699 compile-wired; eval-deferred — the 11 names this PRD wires):**

| Helper | Signature | OCCT FFI | Manifold capability |
|---|---|---|---|
| `edges` | `(Solid) → List<Geometry>` | `extract_edges` (TopExp::MapShapes canonical) — shipped | yes |
| `faces` | `(Solid) → List<Geometry>` | `extract_faces` — shipped | yes |
| `edges_by_length` | `(Solid, Range<Length>) → List<Geometry>` | `extract_edges` + per-edge `EdgeLength` filter | yes |
| `faces_by_area` | `(Solid, Range<Area>) → List<Geometry>` | `extract_faces` + per-face `SurfaceArea` filter | yes |
| `faces_by_normal` | `(Solid, Vector3<D>, Angle) → List<Geometry>` | `extract_faces` + per-face `FaceNormal` angle filter | yes |
| `edges_parallel_to` | `(Solid, Vector3<D>, Angle) → List<Geometry>` | `extract_edges` + per-edge `EdgeTangent` angle filter | yes |
| `edges_at_height` | `(Solid, Length, Length) → List<Geometry>` | `extract_edges` + per-edge bbox Z-range filter | yes |
| `adjacent_faces` | `(Solid, Surface) → List<Geometry>` | `GeometryQuery::AdjacentFaces` — shipped | yes |
| `shared_edges` | `(Surface, Surface) → List<Geometry>` | `GeometryQuery::SharedEdges` — shipped | yes |
| `center_of_mass` (Solid) | `(Solid, Density) → Point3<Length>` | `GeometryQuery::CenterOfMass` — shipped (density currently ignored; see `geometry.rs:793`; this PRD wires density into the integration) | yes (mesh-volume integration) |
| `moment_of_inertia` (Solid) | `(Solid, Density) → Tensor<2,3,MomentOfInertia>` | `GeometryQuery::InertiaTensor` — shipped | yes |

**Out of scope (covered elsewhere).**

- `volume`, `centroid` (single point), `area`, `bounding_box` → GR-030 Phase 6 (task 3608, GHR-ζ).
- `closest_point`, `is_on`, `angle_between_surfaces` → task 2324 (shipped).
- `center_of_mass(Snapshot, [densities])` Snapshot-Map accessor at `crates/reify-stdlib/src/snapshot.rs:432` — shipped today; THIS PRD wires the `Solid` form which uses the kernel's `CenterOfMass` query (currently dispatch-wired but density ignored).
- Geometry-handle infrastructure (`Value::GeometryHandle`, lowering, freshness, cache) → GR-030 owns. This PRD consumes the variant; it does not extend it.
- Unbounded primitives (`half_space`, `extrude_infinite`) → GR-018 (tasks 3579/3580).
- Additional dimensional aliases beyond Curvature (e.g. extending `NAMED_DIMENSIONS`) → GR-029 / task 3115.
- BRep-from-mesh reconstruction (would be needed for full Manifold parity on curves/surfaces) — explicitly out per multi-kernel-phase-3.md §1.

## §3 — Pre-conditions for activating

**Hard prereqs (cross-PRD edges wired at decompose time per `preferences_cross_prd_deps_real_edges`):**

- **GR-030 Phase 1 — GHR-α (task 3603)** — stdlib registrations + `Curvature` dimensional alias + `Physical` spec-shape restoration. This PRD's compile-time recogniser depends on the names + return types being in `crates/reify-compiler/src/units.rs`.
- **GR-030 Phase 2 — GHR-β (task 3604)** — `Value::GeometryHandle` variant + workspace adapter sweep. This PRD's Phase 3 List<Geometry>-returning queries construct sub-handles as `Value::GeometryHandle` and need the variant to exist.
- **GR-030 Phase 3 — GHR-γ (task 3605)** — compile lowering retires the `is_solid_geometry_param` bypass. Without this, geometry-arg cells stay synthetic and the dispatch arms can't resolve them through `named_steps` / `values` the way the existing task-2324 arms do.

**Soft prereqs (load-bearing but not blocking; verify before each phase):**

- GR-030 Phase 4 — GHR-δ (task 3606) — freshness walk + lazy revalidation. Sub-handles constructed in this PRD's Phase 3 rely on the freshness machinery to invalidate when the parent realization changes.
- GR-030 Phase 5 — GHR-ε (task 3607) — cache-key composition + significance filter. Sub-handles must compose cleanly into the cache key per §4 below.

The Phase 2/3 decomposition tasks are filed as `pending` with `add_dependency` edges on GR-030's GHR-γ (task 3605) at minimum. Phase 5 Manifold-parity tasks add an edge on multi-kernel-phase-3 ε (task 3436 / multi-handle Engine).

## §4 — Contract: the dispatcher seam

This PRD extends `try_eval_topology_selector` (`crates/reify-eval/src/geometry_ops.rs:1687`) — the existing kernel-aware eval-time dispatch sibling to `try_eval_conformance_query` and `try_eval_kinematic_query`. The contract:

```rust
pub(crate) fn try_eval_topology_selector(
    expr:          &reify_types::CompiledExpr,
    named_steps:   &HashMap<String, GeometryHandleId>,
    values:        &reify_types::ValueMap,
    kernel:        &dyn reify_types::GeometryKernel,
    diagnostics:   &mut Vec<Diagnostic>,
) -> Option<reify_types::Value>;
```

**Contract invariants (preserved from task-2324 dispatch; load-bearing for additive expansion):**

1. **Arg-shape contract.** Both args must be `ValueRef`s — literal / inline-call shapes fall through to `None`. Pinned by the `try_eval_topology_selector_*_literal_args_falls_through_to_none` unit tests.
2. **Fall-through is preservation.** `None` means "the cell stays at its compiled default (`Value::Undef`)" — never panic, never partial-construct. The arg-resolution helpers (`resolve_point3_length_arg`, `resolve_geometry_handle_arg`) return `Option`; any resolution failure exits via `?` to `None`.
3. **Kernel-error downgrade.** A kernel `Err(_)` reply OR a malformed payload produces `Some(Value::Undef)` with a `Warning` diagnostic — never panics, never returns success-shaped Undef without a diagnostic.
4. **No double-dispatch within one call.** Each helper name routes to exactly one kernel-query variant; no fan-out at the helper layer (fan-out is the multi-kernel dispatcher's job, §6 below).

**Helper-name to `GeometryQuery` variant mapping (the additive table this PRD lands).** All variants except `Contains` and `GeoEquiv` already exist in `crates/reify-types/src/geometry.rs::GeometryQuery`:

| Helper | New / existing variant | Producer |
|---|---|---|
| `distance` | existing `Distance { from, to }` | OCCT shipped; Manifold new |
| `contains` | **new** `Contains { handle, px, py, pz, tolerance }` | OCCT new (BRepClass3d_SolidClassifier); Manifold new (raycast) |
| `intersects` | existing path via `shapes_intersect` adapter | OCCT shipped; Manifold new |
| `geo_equiv` | **new** `GeoEquiv { left, right, tolerance }` | OCCT new (topology hash + sampled vertices); Manifold new |
| `angle` | none (pure math) | n/a |
| `normal` | existing `FaceNormal(id)` for face barycenter case; needs sibling `FaceNormalAt { handle, px, py, pz }` for arbitrary point case | OCCT shipped (barycenter); OCCT new (at-point); Manifold limited |
| `curvature` (Curve) | **new** `CurveCurvatureAt { handle, px, py, pz }` | OCCT new |
| `curvature` (Surface) | existing `curvature_at` FFI returns Curvature struct; expose `SurfaceCurvatureAt { handle, u, v }` and wrap to Matrix<2,2> | OCCT shipped (struct); wrapping in Rust |
| `length` (Curve) | existing `EdgeLength(id)` per-sub-edge; this PRD composes for multi-edge Curve case | OCCT shipped |
| `perimeter` (Surface) | new compose: sum `EdgeLength` over face's edge loop | OCCT shipped |
| Topology selectors (11) | mix of existing (`AdjacentFaces`, `SharedEdges`, `EdgeLength`, `FaceNormal`, `EdgeTangent`, `SurfaceArea`) plus post-filter Rust | OCCT shipped |

**Sub-handle construction (the resolution to Q-KGQ-5).** Topology selectors return `List<Geometry>`. Each element is a `Value::GeometryHandle` constructed as:

```rust
Value::GeometryHandle {
    realization_ref:       parent.realization_ref,        // unchanged from parent
    upstream_values_hash:  blake3(
        parent.upstream_values_hash
            ++ sub_kind                                   // SubKind::Edge | SubKind::Face (u8)
            ++ canonical_topexp_index                     // u32, the index in TopExp::MapShapes order
    ),
    kernel_handle:         sub_shape_kernel_id,           // the GeometryHandleId minted by extract_edges/extract_faces
}
```

**Why this works without extending `Value::GeometryHandle`.**

- `extract_edges` / `extract_faces` already mint separate kernel-level `GeometryHandleId`s in canonical `TopExp::MapShapes` order. The sub-shape kernel handles are persistent within a session.
- `realization_ref` rides with the parent — when the parent solid's realization invalidates (e.g. a param changes its dimensions), every sub-handle's freshness-walk dependency reads the same realization edge. GR-030 Phase 4 (GHR-δ) handles this cascade.
- `upstream_values_hash` is deterministic per `(parent_hash, sub_kind, index)`. Two different edges of the same solid hash differently → compare unequal under `PartialEq` (kernel_handle excluded from equality per GR-030 §2). Same edge across a re-realization (same parent hash, same topology) hashes the same → cache hits.
- The composed hash is stable across runs because `parent.upstream_values_hash` is content-derived; `sub_kind` is a fixed u8; the TopExp index is canonical (deterministic enumeration).

**SubKind encoding.** `pub(crate) enum SubKind { Edge = 0x01, Face = 0x02 }` in `crates/reify-eval/src/geometry_ops.rs`. Serialized into the hash as `[u8; 1]`. New variants additive when needed (Vertex, Wire, etc.).

## §5 — Resolved design decisions

### §5.1 — `geo_equiv` semantics

`geo_equiv(a, b, tol)` returns `Bool(true)` iff **both**:

1. **Topology equivalence.** `a` and `b` have the same vertex / edge / face counts AND the same topology hash. Topology hash = blake3 over canonical `TopExp::MapShapes` enumeration of `(kind, num_oriented_sub_kind)` tuples. This is purely structural; insensitive to numerical drift; cheap (microseconds).
2. **Sample tolerance.** For each face/edge in canonical TopExp order, sample `N=8` parameter points (uniform in parameter space), evaluate position on both `a` and `b`, assert `|p_a - p_b| < tol`. `N=8` chosen empirically: trades cost (~0.5ms per face) against false-positive rate on simple shapes; tunable as a kernel-side const.

False-positive: two solids with identical topology and matching samples at 8 points per face but diverging between samples → still equivalent. Acceptable for v0.3; users needing exact equivalence use direct hash comparison via the existing realization-cache key. False-negative: numerical drift across kernel re-execution within `tol` → not falsely rejected because `tol` absorbs it.

Future `geo_equiv_strict(a, b, tol)` (symmetric Hausdorff distance) is deferred; filed as Open Question §10.

**Implementation-site breadcrumb (mandatory).** The `geo_equiv` FFI implementation in `reify-kernel-occt` and the eval-side dispatch arm in `reify-eval` MUST each carry a comment at the head of the function pointing forward at the strict variant: *"This is the balanced (topology + N=8 sample) equivalence test — see PRD `docs/prds/v0_3/kernel-geometry-queries.md` §5.1 and Open Question §10 for the strict variant (symmetric Hausdorff distance) considered and deferred. Future designers / implementors should weigh the strict form for cases where adversarial inputs (identical topology + matching sample points but diverging between samples) become a real-world problem."* This breadcrumb is the durable design record; the PRD will drift faster than the code. KGQ-δ's acceptance includes the comments being present.

### §5.2 — Tolerance defaults

Per-helper defaults named in `crates/reify-types/src/geometry.rs` adjacent to existing `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (= 1e-7 m = OCCT `Precision::Confusion()`):

```rust
pub const DEFAULT_CONTAINS_TOLERANCE_M: f64 = 1e-7;   // = DEFAULT_POINT_ON_SHAPE_TOLERANCE_M
pub const DEFAULT_INTERSECTS_TOLERANCE_M: f64 = 1e-7;
pub const DEFAULT_GEO_EQUIV_SAMPLE_COUNT: usize = 8;  // per face / edge
// geo_equiv's tolerance is an explicit user arg, no default constant.
```

**Tolerance threading policy.** Per-call default constant pulled at dispatch time. Tolerance is NOT a user-overridable named arg in v0.3 — matches `is_on`'s precedent (`crates/reify-eval/src/geometry_ops.rs:1738-1745`). An explicit-tolerance overload (`is_on(point, geometry, tol)`, `contains(solid, point, tol)`) is a tactical follow-up (Open Question §10).

### §5.3 — Direct kernel dispatch (no ComputeNode)

All queries in scope use direct kernel dispatch through the existing `kernel.query(&GeometryQuery)` path. No ComputeNode wrap. Matches GR-030 Phase 6 precedent + `compute-node-contract.md` §6 ≥50ms heuristic. Most queries are <10ms; topology selectors are O(n_sub_shapes) and could exceed 50ms on million-poly bodies but profile-first is the rule. ComputeNode wrap for topology selectors is deferred (Open Question §10).

### §5.4 — Manifold-kernel capability flags

Per Leo decision 2026-05-14: full Manifold parity attempted, gated by per-query capability flags routed through the multi-kernel-phase-3.md dispatcher. The shape:

```rust
// in crates/reify-types/src/geometry.rs adjacent to GeometryQuery
impl GeometryQuery {
    pub fn capability_kind(&self) -> QueryCapability {
        match self {
            GeometryQuery::CurveCurvatureAt { .. }   => QueryCapability::BRepOnly,
            GeometryQuery::SurfaceCurvatureAt { .. } => QueryCapability::BRepOnly,
            GeometryQuery::Perimeter { .. }          => QueryCapability::BRepOnly,
            GeometryQuery::EdgeLength(_)             => QueryCapability::BRepOnly,  // Manifold has no curves
            // All others below default to BRepAndMesh:
            GeometryQuery::Distance { .. }           => QueryCapability::BRepAndMesh,
            GeometryQuery::Contains { .. }           => QueryCapability::BRepAndMesh,
            GeometryQuery::Intersects { .. }         => QueryCapability::BRepAndMesh,
            GeometryQuery::GeoEquiv { .. }           => QueryCapability::BRepAndMesh,
            // ...
        }
    }
}

pub enum QueryCapability {
    BRepOnly,
    MeshOnly,
    BRepAndMesh,
}
```

**Dispatch policy.** The eval-side dispatcher reads the parent handle's `produced_repr` (from `RealizationNodeData` per multi-kernel-phase-3.md §3) and consults `capability_kind`:

1. If `parent.produced_repr == ReprKind::BRep` AND `BRepOnly` OR `BRepAndMesh` → route to OCCT kernel.
2. If `parent.produced_repr == ReprKind::Mesh` AND `MeshOnly` OR `BRepAndMesh` → route to Manifold kernel.
3. If `parent.produced_repr == ReprKind::Mesh` AND `BRepOnly` (e.g. `curvature` on a mesh-realized solid) → emit `Diagnostic::QueryNotSupportedOnRepr` naming the query + the repr; cell stays at `Value::Undef`. Fail closed, not silent.
4. Any other (Voxel, Sdf, VolumeMesh) → emit the same diagnostic.

The diagnostic is the user-observable signal of capability gating — if a user writes `curvature(mesh_body, p)` they see a clean diagnostic, not a panic or silent Undef.

**Manifold-side implementation.** `crates/reify-kernel-manifold/src/queries.rs` (new module) implements the `BRepAndMesh`-flagged queries via Manifold's mesh APIs. Each query is a separate Rust function consuming the kernel's mesh handle and producing the same return type as the OCCT path. No new FFI gymnastics — Manifold's Rust API exposes vertex / face iteration plus point-in-mesh test directly.

### §5.5 — Sub-handle list memory representation

Each `Value::GeometryHandle` is approximately 64 bytes (`realization_ref: 24 bytes`, `upstream_values_hash: 32 bytes`, `kernel_handle: 8 bytes`). A box returns 12 edges + 6 faces = 18 handles ≈ 1.2 KB. A complex part with 10k faces ≈ 640 KB.

For v0.3: eagerly materialize. Profile in Phase 6's boundary-test sweep on a multi-million-poly fixture. If memory pressure measurable, defer to a lazy-list optimization (filed as Open Question §10).

## §6 — Cross-PRD relationship (G4)

| Other PRD | Direction | Mechanism | Owner |
|---|---|---|---|
| `geometry-handle-runtime.md` (GR-030) | This PRD **consumes** Phases 3+4+5 | Sub-handles are `Value::GeometryHandle`; freshness rides parent's realization; cache-key composes via `upstream_values_hash` chain | GR-030 owns the variant + lowering + freshness. This PRD owns the sub-hash composition logic. |
| `topology-selectors.md` | This PRD **supersedes** the eval-side scope | Task 2691 cancelled at filing; PRD gets §0 supersession note at this PRD's Phase 7 doc-update | This PRD |
| `multi-kernel-phase-3.md` (GR-020) | This PRD **consumes** | Per-query capability flags routed via `parent.produced_repr`; capability-gated diagnostic on Repr mismatch | multi-kernel-phase-3 owns the dispatcher + `produced_repr` tagging; this PRD owns the `QueryCapability` enum + per-`GeometryQuery` mapping |
| GR-029 (dim aliases / task 3115) | This PRD **consumes** | `Curvature` alias added by GR-030 GHR-α; if 3115 adds further aliases (e.g. `MomentOfInertia` per dimension is already in `NAMED_DIMENSIONS`), curvature/inertia return types tighten further | GR-029 territory; this PRD references the existing aliases |
| `compute-node-contract.md` (GR-002) | Orthogonal | Per §5.3: all queries route direct. Future ComputeNode wrap for topology selectors is filed as Open Question | No cross-PRD dependency for v0.3 |
| GR-018 (unbounded primitives, tasks 3579/3580) | Orthogonal | `half_space` / `extrude_infinite` produce geometry handles this PRD's queries consume uniformly | No new seam; PRD's queries handle the existing `Value::GeometryHandle` shape |
| FEA PRDs (`structural-analysis-fea.md`, `multi-load-case-fea.md`) | Consumers of this PRD's outputs | `mass = volume(g) * density`, `center_of_mass` for moment-arm calculations | This PRD makes the helpers callable; FEA PRDs already cite them in prose. The Phase 7 doc-update task notes the activation but does not modify FEA prose (no flat-scalar workarounds are PRD-cited here). |

## §7 — Producer-side contract (cross-crate)

The seam is between `reify-types` (`GeometryQuery` variant additions + `QueryCapability` enum), `reify-eval` (`try_eval_topology_selector` dispatch arms, `geometry_ops.rs`), `reify-kernel-occt` (FFI gap fills: `contains_solid`, `geo_equiv_topo_sample`), `reify-kernel-manifold` (new `queries.rs` module per §5.4), and `reify-compiler` (registrations already done by GR-030 GHR-α).

**Per-arm responsibility split:**

1. **`reify-types::GeometryQuery`** owns variant definitions + the `capability_kind()` table. Adding a new query means adding a variant here + the capability mapping.
2. **`reify-eval::geometry_ops.rs::try_eval_topology_selector`** owns helper-name → variant routing + arg-shape resolution (`ValueRef` resolution via `named_steps` / `values`) + per-arm `kernel.query(...)` dispatch + return-value typing (`Scalar` / `Bool` / `Point` / `Tensor` / `List<Geometry>`). Adding a new helper name means adding a `match` arm here.
3. **`reify-kernel-occt::lib.rs`** owns OCCT FFI wrappers + `GeometryQuery` arm in `OcctKernel::query()`. Adding a new query means adding an FFI fn (if needed) + an arm in `query()`.
4. **`reify-kernel-manifold::queries.rs`** (new) owns the Manifold path. Same shape as OCCT: one fn per query variant + arm in the kernel's `query()`.
5. **`reify-compiler::units.rs`** owns the helper-name → return-type table. GR-030 GHR-α populates the names this PRD wires; this PRD does NOT modify `units.rs`.

**The bypass that does NOT happen.** No new `try_eval_*` sibling dispatch (we already have three siblings — topology_selector / conformance_query / kinematic_query — adding a fourth is YAGNI). All new helpers extend `try_eval_topology_selector` (despite the slightly mis-named function — note the dispatcher comment at `geometry_ops.rs:1627-1632` already calls it "kernel-aware eval-time dispatch for the topology-selector helper family" which is a broader rubric than just topology selectors).

## §8 — Boundary test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level integration), `crates/reify-kernel-occt/tests/` (FFI gap fills), `crates/reify-kernel-manifold/tests/` (Manifold-parity), and per-module unit suites under the new dispatch arms.

### 8.1 — Producer-side (dispatcher + FFI looks outward at consumers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **`contains` FFI happy path.** `OcctKernel::contains(solid_box_10x10x10, point=(0,0,0), tol=1e-7)` returns `Ok(true)`; same with `point=(20mm,0,0)` returns `Ok(false)`. | Phase 2 `contains` FFI landed (BRepClass3d_SolidClassifier). | New `crates/reify-kernel-occt/tests/contains.rs`: 4 fixture cases (center, on-face, corner, far-outside) pin classifier semantics. |
| **`geo_equiv` topology+sample.** `OcctKernel::geo_equiv(box(10mm), box(10mm), 1e-7)` returns `Ok(true)`; `geo_equiv(box(10mm), box(10mm + 1e-9), 1e-7)` returns `Ok(true)` (within tol); `geo_equiv(box(10mm), box(10.01mm), 1e-7)` returns `Ok(false)`. | Phase 2 `geo_equiv` FFI landed (topology hash + N=8 sample). | New `crates/reify-kernel-occt/tests/geo_equiv.rs`: 3 cases above + 1 "different topology" case (box vs cylinder same bounding box) returns false. |
| **Sub-handle hash determinism.** Construct two `Value::GeometryHandle` sub-handles for the same edge of the same box across two engine sessions; assert `upstream_values_hash` is bit-identical. | Phase 3 sub-handle lowering landed. | New unit test in `crates/reify-eval/src/geometry_ops.rs::tests`: pins the `blake3(parent || sub_kind || index)` composition is stable. |
| **Sub-handle inequality.** Two different edges of the same box: `upstream_values_hash` differs; `PartialEq` returns false. | Phase 3 landed. | Same unit test file: pins edges[0] ≠ edges[1] under handle equality. |
| **Capability-gated diagnostic.** Call `curvature(mesh_realized_solid, p)` from `.ri`; eval dispatcher sees `produced_repr=Mesh` + `QueryCapability::BRepOnly`; emits `Diagnostic::QueryNotSupportedOnRepr`. | Phase 5 Manifold parity + capability gating landed. | New `crates/reify-eval/tests/query_capability_gating.rs`: pin diagnostic emission + cell stays `Value::Undef`. |
| **Kernel-error downgrade.** Kernel returns `Err(QueryFailed(_))` for `geo_equiv` on unsupported shape; dispatcher emits `Warning` diagnostic + returns `Value::Undef`. | Phase 2 dispatch arms landed. | Unit test in dispatch arm module: error reply triggers exactly one `Warning` + `Value::Undef` payload. |

### 8.2 — Consumer-side (`.ri` user looks inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Scalar query end-to-end.** `examples/kernel_queries/distance_box_point.ri`: `let d = distance(box(10mm,20mm,30mm), point3(20mm,0,0))` evaluates `d` to `Scalar<Length>(15mm)` (analytic from box centre +5mm offset on +X). | Phase 2 distance dispatch landed. GR-030 GHR-γ landed. | CLI: `reify eval examples/kernel_queries/distance_box_point.ri` prints `d = 15 mm`. Integration test pins value. |
| **Contains predicate.** `examples/kernel_queries/contains_box.ri`: `let inside = contains(box(10mm,10mm,10mm), point3(0,0,0))` evaluates to `Bool(true)`; `let outside = contains(box(10mm,10mm,10mm), point3(20mm,0,0))` evaluates to `Bool(false)`. | Phase 2 contains dispatch + FFI landed. | CLI prints `inside = true`, `outside = false`. Integration test pins. |
| **Topology selector returns 12 edges.** `examples/kernel_queries/box_edges.ri`: `let n = len(edges(box(10mm,20mm,30mm)))` evaluates `n` to `Int(12)`. | Phase 3 edges dispatch landed. | Integration test pins `n == 12`. |
| **Topology selector list element is GeometryHandle.** Same fixture: `edges(box(...))[0]` evaluates to a `Value::GeometryHandle`; the handle is non-Undef; `length(edges(box(...))[0])` evaluates to one of the box's edge lengths (10mm, 20mm, or 30mm). | Phase 3 lands sub-handle construction + Phase 4 lands `length(Curve)` via `EdgeLength`. | Integration test pins the type and the length-set membership. |
| **Mass-property analytic match.** `examples/kernel_queries/moment_of_inertia_box.ri`: `moment_of_inertia(box(50mm,30mm,10mm), 7850.0)` evaluates to a Tensor whose diagonal matches `(1/12) m (W² + H²)` etc. within OCCT precision. | Phase 4 tensor wiring landed (`reify-types::Value::Tensor` already exists). | Integration test pins each diagonal entry within `1e-9 kg·m²`. |
| **Capability-gated diagnostic visible.** User writes `curvature(extract_mesh(solid), p)`; sees `error[E_QueryNotSupportedOnRepr]: 'curvature' requires BRep representation; this geometry is realized as Mesh`. | Phase 5 capability gating landed. | CLI diagnostic visible; not a panic; not silent Undef. |
| **`geo_equiv` smoke.** `examples/kernel_queries/geo_equiv_smoke.ri`: three calls comparing identical / within-tolerance-mutated / topology-different boxes. Results match §5.1 expected. | Phase 2 geo_equiv landed. | Integration test pins all three results. |
| **Freshness cascade through sub-handles.** Edit parent param `width` of a box; observe that all 12 edge sub-handles' freshness state transitions to `Pending` (via GR-030 GHR-δ Realization→ValueCell edge); next read re-realizes parent + re-mints sub-handles. | GR-030 GHR-δ landed (task 3606). | New `crates/reify-eval/tests/sub_handle_freshness.rs`: pin the cascade. |
| **Sub-handle cache hit across re-evaluation.** Evaluate `edges(box(10mm,...))`, exit engine, restart, evaluate same file. Sub-handles' cache keys match → no re-extract → instrumentation pins no kernel call. | GR-030 GHR-ε landed (task 3607). | New persistent-cache round-trip test. |
| **Un-ignore the achievable scenarios.** Of the formerly `#[ignore]`-gated tests in `crates/reify-eval/tests/topology_selector_smoke_tests.rs`, the `block_inertia` test (formerly `:101`) un-ignores + passes. The two `fillet_top_edges` tests (formerly `:173`/`:241`) call the 3-arg `fillet(solid, edges, radius)` form, which is **out of scope for this PRD** (see §0/§2) and owned by deferred task **3205** — they stay `#[ignore]`-gated until 3205 lands. (Reconciled 2026-06-01 / esc-3626-204: the original row demanded all three, contradicting §0/§2's 3-arg-fillet-out-of-scope declaration; the fillet rows defer to 3205.) | Phase 6 sweep; `block_inertia` already landed via task **3560**. | CI green; `block_inertia` runs unconditionally. The two fillet tests remain ignored pending task 3205 (not a Phase-6 failure condition). |

## §9 — Decomposition plan

Approach **B + H** per portfolio. Phase 1 is a small FFI gap-fill foundation (per Leo's Q-KGQ-2 fold-into-Phase-2 decision is interpreted as "no separate Phase 1 enumeration phase, but inline the FFI work into the per-query tasks that need it"). Phases 2–6 are narrow per-query-family tasks (separate `metadata.files` per task to play well with the orchestrator's narrow-file-lock model per `feedback_orchestrator_narrow_locks_favor_upfront_design`). Phase 7 is gap-register + cross-PRD prose.

Greek-letter labels; task IDs assigned at decompose-mode filing time.

### Phase 2 — Scalar / predicate queries (vertical slice)

Each task lands one helper-name family end-to-end: registration recogniser arm (if needed beyond GHR-α), dispatch arm in `try_eval_topology_selector`, OCCT FFI fill (if new), `.ri` example fixture, integration test against analytic expected.

- **Task KGQ-α** — `distance` dispatch + Manifold parity skeleton.
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `reify-eval/tests/`, `examples/kernel_queries/distance_box_point.ri` (NEW), `crates/reify-kernel-manifold/src/queries.rs` (NEW empty module + Distance arm).
  - **Observable signal:** `reify eval examples/kernel_queries/distance_box_point.ri` prints `d = 15 mm`. Integration test pins. Manifold parity arm: same fixture realized through Manifold (via #kernel pragma) produces same value within mesh-tolerance.
  - **Prereqs:** GR-030 GHR-γ (3605) — `Value::GeometryHandle` lowered cells callable from queries.
  - **Approach E note:** This task introduces the Manifold-queries module; Phase 5's other Manifold-parity tasks extend it.

- **Task KGQ-β** — `contains` dispatch + new OCCT FFI (BRepClass3d_SolidClassifier wrapper).
  - **Crates touched:** `reify-kernel-occt/src/ffi.rs` (new `contains_solid` FFI), `reify-kernel-occt/src/lib.rs` (new `contains` method + `GeometryQuery::Contains` arm), `reify-types/src/geometry.rs` (`GeometryQuery::Contains` variant + `QueryCapability::BRepAndMesh` arm + `DEFAULT_CONTAINS_TOLERANCE_M`), `reify-eval/src/geometry_ops.rs` (dispatch arm), `examples/kernel_queries/contains_box.ri` (NEW), `crates/reify-kernel-manifold/src/queries.rs` (Contains via raycast).
  - **Observable signal:** `reify eval examples/kernel_queries/contains_box.ri` prints expected pair `(inside = true, outside = false)`. Integration test pins center / on-face / corner / far-outside fixtures.
  - **Prereqs:** GR-030 GHR-γ (3605).

- **Task KGQ-γ** — `intersects` dispatch + Manifold parity.
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/intersects_smoke.ri` (NEW), `crates/reify-kernel-manifold/src/queries.rs` (Intersects).
  - **Observable signal:** Smoke fixture: two boxes that overlap → true; same boxes 100mm apart → false. Integration test pins.
  - **Prereqs:** GR-030 GHR-γ.

- **Task KGQ-δ** — `geo_equiv` dispatch + new OCCT FFI (topology hash + sampled vertices, §5.1).
  - **Crates touched:** `reify-kernel-occt/src/ffi.rs` (new `geo_equiv_topo_sample` FFI), `reify-kernel-occt/src/lib.rs`, `reify-types/src/geometry.rs` (`GeometryQuery::GeoEquiv` + `DEFAULT_GEO_EQUIV_SAMPLE_COUNT`), `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/geo_equiv_smoke.ri` (NEW), `crates/reify-kernel-manifold/src/queries.rs` (GeoEquiv).
  - **Observable signal:** Three-case smoke in §8.2; integration test pins.
  - **Prereqs:** GR-030 GHR-γ.

- **Task KGQ-ε** — `angle` dispatch (pure-math, no kernel).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/angle_smoke.ri` (NEW).
  - **Observable signal:** `angle(vec3(1,0,0), vec3(0,1,0))` evaluates to `Angle(90 deg)`. Integration test pins.
  - **Prereqs:** GR-030 GHR-α (registration); does NOT need γ since args are pure value-flow vectors.

- **Task KGQ-ζ** — `normal(Surface, Point3)` dispatch (existing `FaceNormalAt` if FFI gap, else compose).
  - **Crates touched:** `reify-kernel-occt/src/lib.rs` (verify or add at-point variant; today's `surface_normal_at` is the right shape — confirm under task), `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/normal_smoke.ri` (NEW).
  - **Observable signal:** `normal(faces(box(10mm))[0], point3(0,0,5mm))` evaluates to a unit vector matching the analytic outward face normal.
  - **Prereqs:** GR-030 GHR-γ; (transitively) Phase 3 `faces` to construct the arg if the example uses a selector — but the smoke can use a directly-constructed face handle for arg shape.

### Phase 3 — List<Geometry>-returning topology selectors

This is where sub-handle construction (§4) lands. Each task wires one selector family. The **`edges` + `faces` foundation task** is filed first because it carries the sub-handle lowering machinery.

- **Task KGQ-η** — `edges` + `faces` foundation (sub-handle lowering + integration).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs` (sub-handle construction logic + dispatch arms for `edges` and `faces`), `reify-eval/src/topology_selectors.rs` (per-§4 `SubKind` enum + hash composition helper), `examples/kernel_queries/box_edges.ri` (NEW), `examples/kernel_queries/box_faces.ri` (NEW).
  - **Observable signal:** `reify eval examples/kernel_queries/box_edges.ri` prints `n = 12` (or similar); `box_faces.ri` prints `n = 6`. Unit tests pin sub-handle hash determinism + inequality. Integration test pins `edges(box(...))[0]` is `Value::GeometryHandle`.
  - **Prereqs:** GR-030 GHR-γ (3605); GR-030 GHR-δ (3606) for freshness edges to compose correctly.

- **Task KGQ-θ** — filtered selectors (`edges_by_length`, `faces_by_area`, `edges_at_height`) — post-filter sub-handle lists.
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `crates/reify-eval/src/topology_selectors.rs`, `examples/kernel_queries/filtered_edges.ri` (NEW).
  - **Observable signal:** `edges_by_length(box(10mm,20mm,30mm), 15mm..25mm)` returns the 4 edges of length 20mm. Integration test pins length-filter semantics.
  - **Prereqs:** KGQ-η.

- **Task KGQ-ι** — directional selectors (`faces_by_normal`, `edges_parallel_to`).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/directional_selectors.ri` (NEW).
  - **Observable signal:** `faces_by_normal(box(10mm), vec3(0,0,1), 1deg)` returns the 1 top face. Integration test pins.
  - **Prereqs:** KGQ-η; (transitively) KGQ-ζ for `normal` available if the dispatcher composes.

- **Task KGQ-κ** — relational selectors (`adjacent_faces`, `shared_edges`).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/adjacent_faces.ri` (NEW).
  - **Observable signal:** On a box: 4 adjacent faces per face; 1 shared edge per adjacent pair. Integration test pins.
  - **Prereqs:** KGQ-η.

### Phase 4 — Tensor / Matrix returns + mass properties

- **Task KGQ-λ** — `moment_of_inertia(Solid, Density)` eval-side dispatch (FFI shipped; this task wires + plumbs density into the integration that today ignores it; new diagnostic for non-Density Scalar arg).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `reify-kernel-occt/src/lib.rs` (density plumbing in `InertiaTensor` arm — today `geometry.rs:793` notes density unused), `examples/kernel_queries/moment_of_inertia_box.ri` (NEW).
  - **Observable signal:** `moment_of_inertia(box(50mm,30mm,10mm), 7850.0)` returns Tensor matching analytic `(1/12) m (W² + H²)` etc. Integration test pins each diagonal entry.
  - **Prereqs:** GR-030 GHR-γ.

- **Task KGQ-μ** — `curvature(Curve)` + `curvature(Surface)` eval-side dispatch + new OCCT FFI arms (Curve-form `curvature_at` is missing; Surface-form returns full Curvature struct → wrap to Matrix<2,2>).
  - **Crates touched:** `reify-kernel-occt/src/ffi.rs` (new `curve_curvature_at` FFI), `reify-kernel-occt/src/lib.rs`, `reify-types/src/geometry.rs` (`CurveCurvatureAt`, `SurfaceCurvatureAt` variants), `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/curvature_smoke.ri` (NEW).
  - **Observable signal:** `curvature(faces(sphere(5mm))[0], point3(5mm,0,0))` returns Matrix<2,2> with mean curvature `1/5mm` (= `1/r` for a sphere). Integration test pins. Curve form: `curvature(edges(circle(10mm,0))[0], point3(...))` = `1/10mm`.
  - **Prereqs:** KGQ-η (for `faces` / `edges` to construct arg).

- **Task KGQ-ν** — `length(Curve)` + `perimeter(Surface)` eval-side dispatch (FFI shipped via `EdgeLength` for sub-edges; perimeter composes face-edge-loop).
  - **Crates touched:** `reify-eval/src/geometry_ops.rs`, `examples/kernel_queries/length_perimeter.ri` (NEW).
  - **Observable signal:** `length(edges(box(10mm,20mm,30mm))[0])` returns one of `{10mm, 20mm, 30mm}`. `perimeter(faces(box(10mm,10mm,10mm))[0])` returns `40mm` (4 × 10mm). Integration tests pin.
  - **Prereqs:** KGQ-η.

### Phase 5 — Manifold-kernel parity + capability gating

- **Task KGQ-ξ** — `QueryCapability` enum + capability-kind table + dispatcher capability-gating logic.
  - **Crates touched:** `reify-types/src/geometry.rs` (enum + `capability_kind` method), `reify-eval/src/geometry_ops.rs` (read `parent.produced_repr`, consult capability, emit `Diagnostic::QueryNotSupportedOnRepr` for mismatches), `reify-types/src/diagnostics.rs` (new diagnostic code).
  - **Observable signal:** Smoke fixture: `curvature(mesh_realized_solid, p)` from `.ri` source produces the diagnostic; cell stays `Value::Undef`. Integration test pins.
  - **Prereqs:** Multi-kernel-phase-3.md α (task 3432, `produced_repr` tagging on `RealizationNodeData`).

- **Task KGQ-ο** — Manifold queries module: Distance, Contains, Intersects, GeoEquiv (the 4 Phase-2 queries that flag `BRepAndMesh`).
  - **Crates touched:** `crates/reify-kernel-manifold/src/queries.rs` (extends KGQ-α's skeleton), `crates/reify-kernel-manifold/src/lib.rs` (`query()` arms), `crates/reify-kernel-manifold/tests/queries.rs` (NEW).
  - **Observable signal:** Each query has a smoke test matching its OCCT analogue's output to within mesh-tolerance. Integration test runs the same `.ri` fixture from KGQ-α/β/γ/δ through `#kernel(manifold)` pragma; results match within tolerance.
  - **Prereqs:** KGQ-α + KGQ-β + KGQ-γ + KGQ-δ (so the OCCT-side oracle exists for parity comparison).

- **Task KGQ-π** — Manifold queries: topology selectors (Edges, Faces, EdgesByLength, FacesByArea, FacesByNormal, EdgesParallelTo, EdgesAtHeight, AdjacentFaces, SharedEdges) + center_of_mass / moment_of_inertia.
  - **Crates touched:** `crates/reify-kernel-manifold/src/queries.rs`, `crates/reify-kernel-manifold/src/lib.rs`, `crates/reify-kernel-manifold/tests/queries.rs`.
  - **Observable signal:** Smoke tests as for KGQ-ο. Manifold mesh-based topology selectors return mesh-edges / mesh-faces (semantics: per-triangle edges deduplicated; per-face — note: Manifold faces are mesh facets, which has different semantics from BRep faces; document the difference in the Manifold queries module-doc).
  - **Prereqs:** KGQ-η + KGQ-θ + KGQ-ι + KGQ-κ + KGQ-λ.

### Phase 6 — Integration gate + un-ignore + boundary-test sweep

- **Task KGQ-ρ** — End-to-end integration gate + un-ignore the achievable `topology_selector_smoke_tests.rs` `#[ignore]`-gated test (`block_inertia`, formerly `:101`). _(Reconciled 2026-06-01 / esc-3626-204: the two `fillet_top_edges` tests formerly at `:173`/`:241` need the 3-arg `fillet(solid, edges, radius)` form — out of scope per §0/§2, owned by deferred task **3205** — and are NOT part of this task; `block_inertia` was already un-ignored by task **3560**, commit `260b1a5f32`.)_
  - **Crates touched:** `crates/reify-eval/tests/kernel_queries_integration.rs` (NEW — exhaustive end-to-end fixture exercising every helper-name in scope on a complex part), `examples/kernel_queries/all_queries_walk.ri` (NEW — single `.ri` file calling every helper at least once). _(Note: `topology_selector_smoke_tests.rs` is NOT touched by this task — `block_inertia` landed in 3560; the fillet un-ignores belong to 3205.)_
  - **Observable signal:** New integration test walks every in-scope helper on a multi-feature part (box + sphere + fillet) + pins non-Undef typed-Value return for every call. `block_inertia` passes unconditionally (landed in 3560). The §8 boundary-test sketch's every in-scope row is implemented somewhere in the test corpus; the two 3-arg-fillet rows defer to task 3205.
  - **Prereqs:** All Phase 2–5 tasks (β through π).
  - **This is the leaf user-observable for the PRD** (the B+H integration-gate).

### Phase 7 — Gap-register + cross-PRD prose sweep

- **Task KGQ-σ** — Doc-update + gap-register.
  - **Crates touched:** docs only.
  - **Files:** `docs/architecture-audit/gap-register.md` (GR-030 Follow-up sub-section gains a pointer to this PRD); `docs/architecture-audit/findings/topology-selectors.md` (M-001 through M-016 status flips to RESOLVED for the 11 deferred eval names); `docs/prds/topology-selectors.md` (§0 supersession block for the eval-side scope; pointer to this PRD).
  - **Observable signal:** `git diff` shows the four files updated; `grep "topology-selectors" docs/architecture-audit/gap-register.md` includes a reference to this PRD.
  - **Prereqs:** KGQ-ρ.

### Dependency view

```text
GR-030 GHR-α (3603) [stdlib registrations]
   │
   └─→ KGQ-ε (angle — needs only registration)
              │
GR-030 GHR-γ (3605) [Value::GeometryHandle lowered]
   │
   ├──→ KGQ-α (distance + Manifold-queries skeleton)
   ├──→ KGQ-β (contains + new OCCT FFI)
   ├──→ KGQ-γ (intersects)
   ├──→ KGQ-δ (geo_equiv + new OCCT FFI)
   ├──→ KGQ-ζ (normal at-point)
   │       │
   └──→ KGQ-η (edges + faces foundation — also needs GHR-δ for freshness)
              │
              ├──→ KGQ-θ (filtered)
              ├──→ KGQ-ι (directional — composes ζ's normal)
              ├──→ KGQ-κ (relational)
              ├──→ KGQ-λ (moment_of_inertia)
              ├──→ KGQ-μ (curvature curve+surface)
              └──→ KGQ-ν (length + perimeter)

Multi-kernel-phase-3 α (3432) [produced_repr tagging]
   │
   └──→ KGQ-ξ (capability enum + gating)
              │
              ├──→ KGQ-ο (Manifold queries: scalar/predicate set — needs α/β/γ/δ)
              └──→ KGQ-π (Manifold queries: topology + mass — needs η/θ/ι/κ/λ)

All Phase 2–5 above
   │
   └──→ KGQ-ρ (integration gate + un-ignore tests)
              │
              └──→ KGQ-σ (gap-register + cross-PRD prose)
```

## §10 — Open questions (surfaced but not decided in this session)

1. **Explicit-tolerance overload for `contains` / `intersects`.** Per §5.2 these use a per-call default constant (matching `is_on`'s precedent). A future explicit-tolerance form `contains(solid, point, tol)` is desirable but requires registration of the 3-arg overload + an extension to the per-helper dispatch arm. **Suggested resolution:** filed as a tactical follow-up once a user actually needs it. Decide during real-world usage. Not in this PRD's scope.

2. **`geo_equiv_strict` (symmetric Hausdorff distance).** Per §5.1, v0.3 default `geo_equiv` is topology-hash + sampled-vertex tolerance — fast but false-positive-prone on adversarial inputs. A rigorous `geo_equiv_strict(a, b, tol)` would compute symmetric Hausdorff distance. **Suggested resolution:** defer to v0.4 unless a user complains. Decide during real-world usage.

3. **ComputeNode wrap for topology selectors on large meshes.** Per §5.3, all queries route direct kernel dispatch. Topology selectors are O(n_sub_shapes); on a million-poly body `edges(.)` could exceed the CN-contract §6 50ms threshold. **Suggested resolution:** profile under KGQ-ρ's integration-gate fixture on a 1M-poly part; if median > 50ms, file a follow-up to wrap selectors in a ComputeNode consumer. Decide during KGQ-ρ.

4. **Lazy list materialization under memory pressure.** Per §5.5, each `Value::GeometryHandle` is ~64 bytes; 10k-face part → ~640 KB; 1M-face part → ~64 MB raw. If profile shows pressure, a lazy `Value::LazyList(Box<dyn FnOnce() -> Vec<Value>>)` could defer materialization — but adds variant complexity and breaks `Value` equality/hash. **Suggested resolution:** measure first under KGQ-ρ. Decide after KGQ-ρ.

5. **Manifold mesh-faces vs BRep faces semantic gap.** Per KGQ-π note: Manifold's "face" is a mesh triangle (or coalesced triangle set); BRep's "face" is a smooth parametric surface patch. `faces(mesh_solid)` and `faces(brep_solid)` return list-of-sub-handles of different cardinalities for the "same" geometric shape. Users querying `len(faces(...))` will see different counts depending on `#kernel(...)` pragma. **Suggested resolution:** document the semantic difference in `crates/reify-kernel-manifold/src/queries.rs` module doc + a `docs/reify-stdlib-reference.md` note. Decide during KGQ-π.

6. **Curve-form `length` over multi-edge curves.** Per §4 table, `length(Curve)` for a multi-edge `Curve` (e.g. a polyline) sums `EdgeLength` across edges. The "single Curve" abstraction in Reify today is fuzzy — most curves at the kernel level ARE single edges. **Suggested resolution:** KGQ-ν starts with single-edge case; multi-edge composition deferred to a tactical follow-up if a user needs it.

7. **`geo_equiv` sample point count tuning.** §5.1 picks N=8 sample points per face/edge empirically. A multi-feature part may need more for adversarial inputs. **Suggested resolution:** ship N=8 as `DEFAULT_GEO_EQUIV_SAMPLE_COUNT`; if users hit false positives, tune the constant or expose as a named-arg overload.

## §11 — Gap-register companion edits

In conjunction with PRD commit (executed in Phase 7 KGQ-σ):

- **GR-030** — `docs/architecture-audit/gap-register.md` GR-030 Follow-up sub-section gains: "Companion PRD: `docs/prds/v0_3/kernel-geometry-queries.md` (wires the eval-side dispatch for the 17 registered helpers not in GR-030 Phase 6 scope; supersedes topology-selectors.md eval-side scope; cancels phantom-done task 2691)."
- **topology-selectors PRD supersession.** `docs/prds/topology-selectors.md` gains a §0 supersession block: "Eval-side scope (task 2691 + the 11 task-2699 deferred names) superseded by `docs/prds/v0_3/kernel-geometry-queries.md` 2026-05-14. FFI work + feature-tag scheme + stdlib bindings from tasks 1–6, 8 remain done; the eval-side dispatch chain is owned by the follow-up PRD."
- **Findings updates.** `docs/architecture-audit/findings/topology-selectors.md`: M-001 through M-011 status flips to `LOADED_GUN → DISPATCH_WIRED` upon KGQ-ρ landing.
- **Task 2691 cancellation.** Set status `done → cancelled` with `reopen_reason="superseded by docs/prds/v0_3/kernel-geometry-queries.md Phase 2/3; phantom-done provenance from test-scaffold-only commit b457cdb8511"` at this PRD's decompose-mode filing.

---

**META check (against `references/gates.md`):** If `kernel-geometry-queries.md` is decomposed and queued without further oversight, the architecture of what gets implemented is complete (every helper in §2 has an owning task), coherent (sub-handle hash composition + capability-flag dispatch + supersession of 2691 form one design story), cohesive (Phase 2–4 follow the same per-query template; Phase 5 is the Manifold-parity mirror; Phase 6 is the integration gate), and good (the FFI work has been verified to be mostly already shipped; the two FFI gaps are scoped per-task with their OCCT API named; sub-handle semantics resolved without extending the GR-030 Value variant). **Yes — ready to save.**

# Topology-Selector Function Family

## Goal

Ship the eleven topology- and mass-property selector functions called out in
`docs/reify-stdlib-reference.md` §3.9 that aren't already covered by #318/#319,
plus a feature-tag persistent-naming scheme so selector results survive minor
topology changes (added fillets, repositioned features) and degrade gracefully
(returning `undef` plus a diagnostic) when a tag goes ambiguous or stale.

## Background

`#318` shipped `edges()`, `faces()`, `edges_by_length`, `faces_by_area`,
`faces_by_normal`, `edges_parallel_to`, `edges_at_height` (the **filtered list**
selectors over a whole solid). `#319` shipped point-membership and intersection
queries. This PRD covers the remaining §3.9 surface area: selectors that
**relate to a specific feature** (closest_point, on, angle_between_surfaces,
adjacent_faces, shared_edges) and the **mass-property triplet** (centroid is
already in #319; this PRD adds `center_of_mass` with density and
`moment_of_inertia`).

The persistent-naming scheme mirrors Solvespace-style "feature tags" but stays
minimal for v0.1: each compiler-generated face/edge gets a tag derived from
`(source_line, step_kind, sub_index)` — i.e. "the third edge produced by line
42's `fillet(...)` step". When topology changes invalidate a tag (zero or
multiple matches after a downstream edit), the selector returns `undef` and
emits `warn[W_TOPOLOGY_TAG_STALE]` referencing the original source span.
Solvespace's full attribute-persistence scheme is deferred to v0.2.

`#249` (ad-hoc port selectors compiler) is the **reference implementation** for
feature-tag plumbing: its `CompiledAdHocPort` IR carries selector kind +
arguments + memory_hints, and its compiler pass attaches the tag at the
call site. This PRD reuses that machinery; selectors here become first-class
sibling functions, not new IR variants.

Stdlib §3.9 signatures (the eleven targeted by this PRD):

```reify
fn closest_point<G: Geometry>(point: Point3<Length>, geometry: G) -> Point3<Length>
fn on<G: Geometry>(point: Point3<Length>, geometry: G) -> Bool
fn angle_between_surfaces(a: Surface, b: Surface) -> Angle

fn center_of_mass(solid: Solid, density: Scalar<Density>) -> Point3<Length>
fn moment_of_inertia(solid: Solid, density: Scalar<Density>) -> Tensor<2, 3, MomentOfInertia>

fn adjacent_faces(solid: Solid, face: Surface) -> List<Surface>
fn shared_edges(face1: Surface, face2: Surface) -> List<Curve>

// Already prototyped in #318 but re-exposed under feature-tag naming:
fn edges_at_height(solid: Solid, height: Length, tolerance: Length) -> List<Curve>
fn edges_parallel_to(solid: Solid, direction: Vector3<Dimensionless>, tolerance: Angle) -> List<Curve>
fn edges_by_length(solid: Solid, range: Range<Length>) -> List<Curve>
fn faces_by_area(solid: Solid, range: Range<Area>) -> List<Surface>
```

## Worked examples

```reify
// Mass properties on a steel block.
fn block_inertia() -> Tensor<2, 3, MomentOfInertia> {
    let b = box(50mm, 30mm, 10mm)
    let steel_density = 7850 kg/m^3
    moment_of_inertia(b, steel_density)
}

// Topological neighbourhood: fillet only edges adjacent to the top face.
fn fillet_top_edges(b : Solid) -> Solid {
    let top = single(faces_by_normal(b, [0, 0, 1], 1deg))
    let top_edges = flat_map(adjacent_faces(b, top), |f| shared_edges(top, f))
    fillet(b, top_edges, 1mm)
}

// Persistent naming through an edit: edges_at_height(...) on a chamfered solid
// should return the same chamfer-bottom edges as before the chamfer was
// re-parameterized, by matching feature tags rather than absolute Z.
fn chamfer_bottom_ring(b : Solid, h : Length) -> List<Curve> {
    edges_at_height(b, h, 0.01mm)   // returns tagged ring; survives
                                     // downstream b's parameter edits
}
```

## Scope

1. **Three new OCCT FFI entry points** (under `kernel-occt`):
   `closest_point_on_shape` (already prototyped for `closest_point` in #319 —
   re-export under the new name), `surface_angle` (dihedral via face-normal
   dot), `center_of_mass` + `moment_of_inertia` via OCCT's
   `BRepGProp_SurfaceProperties` / `BRepGProp_VolumeProperties` with density.
2. **Topology-relational FFIs**: `adjacent_faces` (via shared-edge iteration
   over the face's edge loop), `shared_edges` (intersection of two faces'
   edge lists).
3. **Feature-tag scheme** in `reify-compiler`:
   - Extend the per-op compiler pass that builds `CompiledGeometryOp` to
     attach a `feature_tag : (source_line, step_kind, sub_index)` to each
     produced face/edge handle as the OCCT shape is realized.
   - Tag storage: append-only on the runtime shape's metadata table (one
     `Vec<FeatureTag>` per `ShapeId`).
   - Selector resolution: at the call site, walk the runtime shape, match
     tags, return matched sub-shapes. Zero or multiple matches → `undef` +
     `W_TOPOLOGY_TAG_STALE` diagnostic with the original source span.
4. **Re-expose the four already-shipped filtered selectors**
   (`edges_at_height`, `edges_parallel_to`, `edges_by_length`, `faces_by_area`)
   to use the feature-tag path so they degrade gracefully across topology
   changes — currently they re-iterate every call. Behaviour change: same
   results when topology stable, `undef`/diagnostic instead of stale results
   when topology changes.
5. **Stdlib bindings** — register the eleven functions as built-ins in the
   stdlib registration pass (analogous to where #318's selectors are wired).
   Re-use `Tensor<2, 3, MomentOfInertia>` from existing tensor work.
6. **Tests**: per-FFI happy-path tests; feature-tag survival across a fillet
   edit (selector returns same result after a parameter tweak); stale-tag
   path emits `W_TOPOLOGY_TAG_STALE` exactly once.

## Out of scope

- Solvespace-style full attribute-persistent naming (v0.2).
- Imported geometry — selectors against imported BREP shapes are out of
  scope; they require their own naming scheme.
- `closest_point` between two surfaces (closest_point on geometry from a
  point only, per signature).

## Acceptance criteria

- `cargo test -p reify-kernel-occt -- selectors` covers the seven new
  FFI entry points against fixture shapes (box, fillet, sphere).
- `cargo test -p reify-eval -- topology_selectors` covers all eleven
  stdlib functions end-to-end through `compile_with_stdlib`.
- `cargo test -p reify-compiler -- feature_tag` covers tag generation,
  resolution, and the stale/ambiguous → `undef` + `W_TOPOLOGY_TAG_STALE`
  path.
- `moment_of_inertia(box(L, W, H), ρ)` returns the analytic
  `(1/12) * m * (W² + H²)` etc. tensor within OCCT precision.
- Stale-tag diagnostic test: edit a profile so the tagged feature
  disappears → selector returns `undef`, exactly one warning, source span
  points back to original selector call site.

## Dependencies

Depends on **`geometry-traits.md`** (this PRD's selectors require `Bounded`
arguments for mass properties; the `Bounded` diagnostic must exist first).
Also references #318, #319 (existing selector FFI pattern) and #249
(`CompiledAdHocPort` feature-tag plumbing reference).

## Task breakdown (queueing aim: 7 tasks)

1. **Feature-tag IR + runtime metadata table**: extend
   `CompiledGeometryOp` to attach `(source_line, step_kind, sub_index)`
   tags; add per-`ShapeId` tag table on the OCCT runtime shape. Wire one
   already-shipped selector (`edges_at_height`) through the new path as
   the proof-of-concept; existing tests must still pass.
2. **OCCT FFI: `closest_point` + `on` + `angle_between_surfaces`**.
   Following #319's FFI pattern. Stdlib bindings + eval wiring + happy-path
   tests.
3. **OCCT FFI: `center_of_mass` + `moment_of_inertia`** via
   `BRepGProp_VolumeProperties` with density. Stdlib bindings; analytic
   tensor verification on box/cylinder.
4. **OCCT FFI: `adjacent_faces` + `shared_edges`**. Iterate
   face-edge topology. Tests on box (4 adjacent faces per face; 1 shared
   edge per adjacent pair).
5. **Re-route the four already-shipped filtered selectors**
   (`edges_at_height`, `edges_parallel_to`, `edges_by_length`,
   `faces_by_area`) through feature-tag resolution. Behaviour change:
   stale → `undef` + warning. Migration tests prove pre-existing tests
   still pass under new path.
6. **Stale-tag diagnostic `W_TOPOLOGY_TAG_STALE`** with source-span
   surface mapping. Test: edit profile → tagged feature disappears →
   exactly one warning emitted, span points to original selector site.
   — **Implemented (task 2332):** `DiagnosticCode::TopologyTagStale` in
   `crates/reify-types/src/diagnostics.rs`; resolver building-block
   `resolve_unique_by_tag` in `crates/reify-eval/src/topology_selectors.rs`
   (covered by three unit tests: 1-match / 0-match / N>1-match).
   Re-routing existing filter selectors through the resolver is tracked
   separately under task 5 (task 2329 in the queue).
7. **Worked-example smoke tests** — the two examples from this PRD
   (`block_inertia`, `fillet_top_edges`) shipped as `.ri` example files
   under `examples/topology_selectors/` and exercised by the eval test
   harness.

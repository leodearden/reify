# Runtime Conformance FFI: `IsWatertight`, `IsManifold`, `IsOrientable`

This document describes the runtime conformance query surface implemented in
`crates/reify-kernel-occt` as **task 4** of the geometry-traits PRD
(`docs/prds/geometry-traits.md`).  The three predicates back the marker traits
declared in `crates/reify-compiler/stdlib/geometry_traits.ri` (dep #2297):
`Watertight : Closed + Manifold`, `Manifold`, and `Orientable`.

Each predicate is exposed as a `GeometryQuery` variant and dispatched through
`OcctKernel::query()`, following the same `get_shape → FFI → Value::Bool` pattern
as `Volume`, `SurfaceArea`, `AdjacentFaces`, and the other existing query arms.

---

## `IsWatertight(GeometryHandleId) → Value::Bool`

**OCCT class:** `BRepCheck_Analyzer`

**Semantics:** returns `true` iff the shape is a valid, closed solid with no
free edges — i.e. it encloses a well-defined volume.

**Shape-type guard:** before invoking the analyzer, the C++ wrapper checks
`shape.ShapeType()`.  Shapes that are not `TopAbs_SOLID`, `TopAbs_COMPSOLID`,
or `TopAbs_SHELL` **always return `false`** regardless of their topological
validity.  COMPOUND is intentionally excluded — `BRepCheck_Analyzer.IsValid()`
on a compound reports topological consistency, not closure, so a compound of
open faces would spuriously pass.  Callers needing per-sub-shape watertightness
should iterate the compound's children.  This guard also prevents `FACE` and
`WIRE` shapes (which pass `BRepCheck_Analyzer.IsValid()` as valid topology,
but enclose no volume) from being incorrectly reported as watertight.

This aligns the predicate with the `Watertight : Closed + Manifold` trait
semantics in `geometry_traits.ri`, where "watertight" means the boundary is a
closed surface bounding an enclosed region.

---

## `IsManifold(GeometryHandleId) → Value::Bool`

**OCCT class:** `TopTools_IndexedDataMapOfShapeListOfShape` (the cached `edge_face_map` slot)

**Semantics:** returns `true` iff every edge in the shape has **at most 2**
incident faces.  Edges with 3 or more parent faces indicate a non-manifold
junction (e.g. a T-junction or shared fin), which is `false`.

**Cache reuse:** the implementation walks the `shape.edge_face_map()` lazy slot,
which is the same `TopExp::MapShapesAndAncestors`-populated incidence map used by
`AdjacentFaces` and `SharedEdges`.  A manifold query issued after a prior topology
query on the same shape reuses the cached map at O(edges) with no rebuild.  The
build counter (observable via `OcctKernel::topology_cache_build_counts`) stays at 1.

**Orthogonality from `IsOrientable`:** `Manifold` and `Orientable` are sibling
traits in `geometry_traits.ri` — neither inherits from the other.  Splitting the
predicates keeps them independently queryable and avoids conflating manifoldness
(≤ 2 parent faces per edge) with orientation consistency (FORWARD/REVERSED
pairing), which would happen if both were routed through `CheckOrientedShells`.

---

## `IsOrientable(GeometryHandleId) → Value::Bool`

**OCCT class:** `ShapeAnalysis_Shell`

**Semantics:** returns `true` iff every connected edge that belongs to two faces
has those faces oriented with opposite senses (one `FORWARD`, one `REVERSED`),
i.e. the boundary admits a consistent global outward normal.

**Implementation:** calls `ShapeAnalysis_Shell::LoadShells(shape)` then
`CheckOrientedShells(shape, alsofree=Standard_False)`.  If `NbLoaded() == 0`
(the shape has no shells — e.g. a bare wire, isolated face, or vertex), the
predicate trivially returns `true`: there is no shell to orient, so the
orientability condition is vacuously satisfied.

**Divergence from PRD task description:** the PRD nominates `ShapeAnalysis_Wire`
for orientability.  `ShapeAnalysis_Wire` checks individual wire properties
(closure, self-intersection, vertex tolerances), not global outward-normal
consistency.  `ShapeAnalysis_Shell::CheckOrientedShells` is OCCT's canonical
predicate for "consistent shell orientation" and is the correct choice for the
solid/shell targets described in the PRD's worked examples (`conforms(p : Solid,
Watertight)`).  See design decision in `plan.json`.

---

## Invocation example

```rust
use reify_types::{GeometryQuery, Value};

let result = kernel.query(&GeometryQuery::IsWatertight(handle))?;
let watertight = match result {
    Value::Bool(b) => b,
    other => unreachable!("conformance query must return Bool, got {other:?}"),
};

// For brevity, a helper closure:
let conformance = |q: GeometryQuery| -> Result<bool, _> {
    match kernel.query(&q)? {
        Value::Bool(b) => Ok(b),
        other => unreachable!("expected Bool, got {other:?}"),
    }
};

let watertight  = conformance(GeometryQuery::IsWatertight(handle))?;
let manifold    = conformance(GeometryQuery::IsManifold(handle))?;
let orientable  = conformance(GeometryQuery::IsOrientable(handle))?;
```

---

## Error handling

All three query arms follow the established kernel error contract:

| Condition | Result |
|-----------|--------|
| Unknown `GeometryHandleId` | `Err(QueryError::InvalidHandle(id))` |
| OCCT exception during analysis | `Err(QueryError::QueryFailed(msg))` |
| Success | `Ok(Value::Bool(b))` |

---

## Next step

The stdlib `conforms(g, Watertight)` helper that consumes these three predicates
is implemented in **PRD task 5** (separate ticket).  Task 5 wires the
`GeometryQuery::IsWatertight` / `IsManifold` / `IsOrientable` arms through the
eval layer into a callable `.ri` stdlib function, allowing `.ri` source code to
write:

```
let ok = conforms(my_solid, Watertight);
```

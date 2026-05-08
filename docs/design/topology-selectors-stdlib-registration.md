# Topology-selector stdlib registration — pattern reference

**Status:** design reference — describes the existing wiring pattern in main as of 2026-05-08 (post task #2324 / #2696 / #2746). Source of truth for how to land the remaining 11 PRD §3.9 selectors under task #2699.

**Revision history:**
- **2026-05-08, original** — α/β/γ design pass after esc-2699-2; surfaced 8 open questions for the next planner.
- **2026-05-08, post-design-call** — §4.2 reframed (3-arg `fillet` is a separate task), §4.3 collapsed to a single approach (existing `selector_vocabulary_v2::adjacent_to_face` pattern), mutability widening called out as a cluster-A prerequisite. See "Decisions made" panel near the top.

**Companions:** `docs/prds/topology-selectors.md` §3.9; tasks #2696 (Tensor surface-syntax + named dimensions), #2698 (`single` / `flat_map` list helpers), #2691 (deepened smoke).

## Decisions made (2026-05-08 post-design call)

| Question | Resolution |
|---|---|
| Kernel mutability widening | **Widen** `try_eval_topology_selector` and `Engine::post_process_topology_selectors` to `&mut dyn GeometryKernel`. Required for cluster A onwards (every selector that calls `extract_*`). RefCell alternative rejected. |
| `shared_edges` parent derivation | **Option A**: `kernel.query(OwnerBody(face_a))` + verify match against `OwnerBody(face_b)`. Warning + empty list on cross-solid input. No new trait method. |
| 3-arg `fillet(solid, edges, radius)` | **Separate task**, not #2699's scope. The parse-only `fillet_top_edges.ri` fixture stays parse-only until that task lands. |
| List element typing | `Type::List(Box::new(Type::Geometry))` for v0.1; defer Edge/Face/Vertex discrimination to #2691 or future PRD revision. |

---

## 1. The α/β/γ choice

The earlier scoping audit (2026-04-29) reported that no obvious stdlib registry surfaces by grep for `register_function` / `StdlibRegistry`. That conclusion was correct: there is no central registry. Re-tracing `box`, `vec3`, `cylinder`, `fillet`, and the already-wired `closest_point` shows the actual pattern is:

> **β with refinement: a Rust-side, name-keyed dispatch with multiple parallel sites.** There is no single registration table; instead, each "kind of call" has its own dispatch site keyed on the function-call name. The compiler-side classifier consts (`crates/reify-compiler/src/units.rs`) and the eval-side `try_eval_*` functions (`crates/reify-eval/src/geometry_ops.rs`) together form the de-facto registry.

The `.ri` files in `crates/reify-compiler/stdlib/*.ri` are **not** for surface functions like `box` or topology selectors. They host **types**: traits (`Manifold`, `Watertight`, `Bounded`), units, materials, structural-physical conventions. They are loaded in sequence by `crates/reify-compiler/src/stdlib_loader.rs` to seed a growing `PreludeContext`. Adding `fn closest_point(...)` declarations to `geometry_traits.ri` is **not** required by the existing wiring pattern (and would not by itself wire dispatch).

(α) was a plausible mental model — it would unify "stdlib feels like .ri-namespaced functions". It is rejected because every primitive currently callable from `.ri` is dispatched from Rust by string match, including the trait-using ones. (γ) is unnecessary as long as Rust-side classifier-and-dispatch is sufficient; the surface-language type-checker only needs the result type, which `topology_selector_result_type` provides without `.ri` fn declarations.

### Evidence — three concretely different paths exist

| Surface call | Recognized at | Lowered to | Dispatched at |
|---|---|---|---|
| `box(w, h, d)` | `crates/reify-compiler/src/geometry.rs:364` (`compile_geometry_call` match `"box"`) | `CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, … }` | `Engine::execute_realization_ops` → `kernel.execute(…)` |
| `cylinder(r, h)` | `crates/reify-compiler/src/geometry.rs:378` (same match) | `CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder }` | same |
| `fillet(target, r)` | `crates/reify-compiler/src/geometry.rs:835` then `crates/reify-compiler/src/geometry_modify.rs:115` (`compile_modify_op` / `compile_modify_2arg`) | `CompiledGeometryOp::Modify { kind: ModifyKind::Fillet, … }` | same |
| `vec3(x, y, z)` | `crates/reify-stdlib/src/geometry.rs:715` (`eval_geometry` match `"vec3"`) | not lowered to a geometry op — pure value constructor | `reify_stdlib::eval_builtin` chain in `crates/reify-stdlib/src/lib.rs:44` |
| `closest_point(p, g)` | `crates/reify-compiler/src/units.rs:151` (const list `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`) + `crates/reify-compiler/src/expr.rs:934` (result-type wiring) | `CompiledExpr::FunctionCall { … }` (no specialized lowering — stays as a generic call) | `crates/reify-eval/src/geometry_ops.rs:1664` (`try_eval_topology_selector`), invoked from `Engine::post_process_topology_selectors` (`crates/reify-eval/src/engine_build.rs:1344`) |

Important consequence: there are **at least three distinct dispatch surfaces**, each with its own registration site. Picking the right one for each of the 14 selectors is 80% of the work for #2699 — once chosen, the actual edits per selector are small and uniform.

---

## 2. The four signature-shape buckets and where each plugs in

The 14 selectors break into the buckets below. Each bucket inherits an existing wiring template; the table maps bucket → template → registration sites.

### Bucket 1 — kernel-query selectors on a Geometry handle (Task 2324 pattern)

This is the canonical pattern. It covers nine of the 14 selectors:

`closest_point` *(done — Task 2324)*, `on` *(done)*, `angle_between_surfaces` *(done)*, plus the ones still to wire: `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `adjacent_faces`, `shared_edges`, `center_of_mass`, `moment_of_inertia`.

**Cluster-A prerequisite (one-time):** widen `try_eval_topology_selector` (`crates/reify-eval/src/geometry_ops.rs:1668`) and `Engine::post_process_topology_selectors` (`crates/reify-eval/src/engine_build.rs:1348`) from `kernel: &dyn GeometryKernel` to `kernel: &mut dyn GeometryKernel`, propagating through the three call sites at `engine_build.rs:519/662/879`. The three call sites already hold `&mut kernel` and downgrade self-imposedly. ~10 lines total. Required because every new selector calls `kernel.extract_edges(...)` / `kernel.extract_faces(...)`, both of which take `&mut self` (they populate the kernel's idempotent extract caches at `crates/reify-kernel-occt/src/lib.rs:495/499`). Sibling `try_eval_conformance_query` and `try_eval_kinematic_query` keep `&dyn` (they only call `kernel.query(...)`).

**Four edits per name:**

1. **Compiler classifier** — `crates/reify-compiler/src/units.rs`
   - Append the name to `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` (or factor a sibling const if the existing list is conceptually only about "v0.1 point/surface helpers" — see "Open questions" below).
   - Add a `topology_selector_result_type` arm that returns the cell type:
     - List-shaped returns (`edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `adjacent_faces`, `shared_edges`) → `Type::List(Box::new(<elem>))`. The element type is `Type::Geometry` for entity-yielding selectors (these surface as tagged sub-handles). Until #2691 deepens semantics, `Type::List(Box::new(Type::Geometry))` is the conservative type that unblocks `fillet(b, top_edges, r)`.
     - `center_of_mass` → `Type::point3(Type::length())`.
     - `moment_of_inertia` → `Type::Tensor { rank: 2, n: 3, quantity: <MomentOfInertia> }` — requires #2696's surface-syntax (now landed) and the named-dimension entry it adds.

2. **Compiler integration** — `crates/reify-compiler/src/expr.rs:934`
   - Already wired generically: the `else if is_geometry_topology_selector(name)` branch will pick up new entries from the const list automatically. **No edit required** as long as #1 uses the same const list.

3. **Eval-time dispatch** — `crates/reify-eval/src/geometry_ops.rs`
   - Extend `try_eval_topology_selector` (function at line 1664; helper enum `TopologySelectorHelper` at line 1741): add a variant per name, route to a `kernel.query(GeometryQuery::*)`, and convert the kernel reply.
   - All target kernel-query variants already exist in `crates/reify-types/src/geometry.rs:618` (`GeometryQuery::AdjacentFaces`, `SharedEdges`, `CenterOfMass`, `InertiaTensor`, plus the existing `EdgeLength`/`EdgeTangent`/`FaceNormal`/`FaceSurfaceKind`/`EdgeCurveKind` building-blocks the selectors compose against). For the filtered selectors (`edges_by_length`/`faces_by_area`/etc.) the dispatch can call straight into the existing per-selector functions in `crates/reify-eval/src/topology_selectors.rs` (`pub fn edges_by_length`, etc.), which already do the per-sub-shape iteration and predicate-filtering.
   - Relax the `args.len() != 2` early return at line 1686: several new selectors are 3-arg (`faces_by_normal(g, dir, tol)`, `edges_parallel_to(g, axis, tol)`, `edges_at_height(g, z, tol)`, `center_of_mass(g, density)`, `moment_of_inertia(g, density)`). Replace with a per-name arity check (or move the arity check into each helper's arm).

4. **Engine post-process** — `crates/reify-eval/src/engine_build.rs`
   - `Engine::post_process_topology_selectors` (line 1344) already iterates every `template.value_cells` whose `default_expr` is a recognised topology selector and patches `try_eval_topology_selector`'s `Some(value)` into `values`. **No edit required** — adding a new arm in `try_eval_topology_selector` automatically extends post-processing.
   - Three call sites that invoke `post_process_topology_selectors` already exist for build / build_snapshot / tessellate paths (lines 519, 662, 879). No new sites are needed.

### Bucket 2 — pure constructors `edges`, `faces`

These are zero-extra-arg topology decompositions: `edges(geometry)` returns the list of edges of a solid, `faces(geometry)` returns its faces.

These fit Bucket 1's pattern: register the name; result type `Type::List(Box::new(Type::Geometry))`; dispatch arm in `try_eval_topology_selector` calls `kernel.extract_edges(handle)` / `kernel.extract_faces(handle)` (`crates/reify-kernel-occt/src/lib.rs:605`/`651` — both already exist and are idempotent per parent), then wraps the resulting handles into a `Value::List`.

Open question: `extract_*` returns `Vec<GeometryHandleId>`. The existing `Value::List` payload for topology results is `Value::List(Vec<Value::Int>)` (per the kernel-query AdjacentFaces/SharedEdges contract at `crates/reify-types/src/geometry.rs:649`). For `edges`/`faces` to compose with `fillet(solid, edges, r)`, the eval side must thread the resulting handles through the topology-attribute-table so fillet's geometry-arg resolution can dereference them. See "Open questions" below.

### Bucket 3 — physical-property returns: `center_of_mass`, `moment_of_inertia`

Already covered under Bucket 1. Two notes:

- `center_of_mass` already has a same-named `eval_builtin` entry at `crates/reify-stdlib/src/snapshot.rs:432` for the kinematic-Snapshot form (`center_of_mass(snapshot, densities_map)`). The PRD-§3.9 form takes a *Geometry* + scalar density. Disambiguation is automatic at runtime: `try_eval_topology_selector` only fires when the function is in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` AND `args[0]` resolves to a `named_steps` `GeometryHandleId`; for the snapshot form `args[0]` is a `Value::Map`, no `GeometryHandleId` resolves, the dispatch returns `None`, and `eval_builtin`'s `center_of_mass` arm in snapshot.rs handles it. **No code conflict, but a unit test pinning both call shapes is mandatory** — drop it in `crates/reify-eval/tests/topology_selector_smoke_tests.rs` once compile-with-stdlib coverage opens up.
- `moment_of_inertia` requires the just-landed Tensor surface syntax (#2696) for its return type to be expressible. The kernel already returns the right-shape `Value::List(rows)` at `crates/reify-types/src/geometry.rs:717` (`GeometryQuery::InertiaTensor`).

### Bucket 4 — topology-graph queries: `adjacent_faces`, `shared_edges`

Already covered under Bucket 1, but with one subtlety worth flagging up-front:

The kernel queries take a parent solid + face indices: `AdjacentFaces { shape, face_index }` and `SharedEdges { shape, face_a, face_b }`. The PRD signatures are `adjacent_faces(solid, face) -> List<Face>` and `shared_edges(face_a, face_b) -> List<Edge>` — i.e. they take entity handles, not solid+index pairs.

The `face` / `face_a` / `face_b` arguments are sub-handles produced by an `extract_faces`-style call (Bucket 2). The dispatch arm needs to:
1. Recover the parent solid from the face sub-handle (already supported: `GeometryQuery::OwnerBody` at line 800).
2. Recover the face index within that parent (the kernel records this on every `extract_*` call — see the comment at `crates/reify-kernel-occt/src/lib.rs:677`; an explicit query variant or a kernel-internal lookup will be needed if one is not already exposed).

For `shared_edges(face_a, face_b)` the additional requirement is that both faces share the same parent solid; mismatched parents should produce an empty list (or a `Diagnostic::warning` per the PRD).

---

## 3. Worked example — wiring `edges(geometry) -> List<Edge>`

`edges` is the simplest target: zero predicate args, a kernel method already exists, no new query variants needed. Use this as the template; the remaining selectors layer arity / predicate-arg / kernel-query specifics on top.

### Files that change

1. **`crates/reify-compiler/src/units.rs`** — append `"edges"` to `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`, add a result-type arm in `topology_selector_result_type`.
2. **`crates/reify-eval/src/geometry_ops.rs`** — add a `TopologySelectorHelper::Edges` enum variant, an arity arm, and a dispatch arm calling `kernel.extract_edges`.
3. **`crates/reify-eval/tests/topology_selector_smoke_tests.rs`** — extend the parse-only fixture coverage to a `compile_with_stdlib`-clean assertion for a fixture that calls `edges(b)`.
4. **`docs/prds/topology-selectors.md`** — add the explicit "task 8: Stdlib language-level wiring" entry (this is the PRD-amendment carry-over from the original #2699 description).

### Exact change — `units.rs`

```rust
pub const GEOMETRY_TOPOLOGY_SELECTOR_NAMES: &[&str] =
    &["closest_point", "on", "angle_between_surfaces", "edges"];
//                                                       ^^^^^^^^

pub(crate) fn topology_selector_result_type(name: &str) -> Option<reify_types::Type> {
    use reify_types::Type;
    Some(match name {
        "closest_point" => Type::point3(Type::length()),
        "on" => Type::Bool,
        "angle_between_surfaces" => Type::angle(),
        "edges" => Type::List(Box::new(Type::Geometry)),
        _ => return None,
    })
}
```

Plus four unit tests in the existing `mod tests` block, mirroring the `is_geometry_topology_selector_recognises_*` and `topology_selector_result_type_*` tests already there (lines 495–571).

### Exact change — `geometry_ops.rs`

Add the variant:

```rust
#[derive(Clone, Copy)]
enum TopologySelectorHelper {
    ClosestPoint,
    On,
    AngleBetweenSurfaces,
    Edges,
}
```

Recognise the name (line 1678–1683):

```rust
let helper = match function.name.as_str() {
    "closest_point" => TopologySelectorHelper::ClosestPoint,
    "on" => TopologySelectorHelper::On,
    "angle_between_surfaces" => TopologySelectorHelper::AngleBetweenSurfaces,
    "edges" => TopologySelectorHelper::Edges,
    _ => return None,
};
```

Replace the `if args.len() != 2` early return at line 1686 with a per-helper arity check:

```rust
let expected_arity = match helper {
    TopologySelectorHelper::ClosestPoint
    | TopologySelectorHelper::On
    | TopologySelectorHelper::AngleBetweenSurfaces => 2,
    TopologySelectorHelper::Edges => 1,
};
if args.len() != expected_arity {
    return None;
}
```

Add the dispatch arm in the outer match (after `AngleBetweenSurfaces`):

```rust
TopologySelectorHelper::Edges => {
    let handle = resolve_geometry_handle_arg(&args[0], named_steps)?;
    // extract_edges is a kernel method, not a query; the dispatch
    // pattern here parallels the query-based arms but calls
    // kernel.extract_edges directly. The kernel's idempotency cache
    // (lib.rs:483) makes repeat calls cheap.
    let sub_handles = match kernel.extract_edges(handle) {
        Ok(ids) => ids,
        Err(e) => {
            diagnostics.push(Diagnostic::warning(format!(
                "edges({:?}): kernel error {:?}",
                handle, e
            )));
            return Some(Value::Undef);
        }
    };
    Some(Value::List(
        sub_handles.into_iter().map(|h| Value::Int(h.0 as i64)).collect()
    ))
}
```

### Exact change — `topology_selector_smoke_tests.rs`

Add a fixture `examples/topology_selectors/edges_count.ri` and a third smoke test:

```ri
structure def EdgesCount {
    let b = box(50mm, 30mm, 10mm)
    let es = edges(b)
}
```

```rust
#[test]
fn edges_count_compiles_with_stdlib() {
    let source = std::fs::read_to_string(EDGES_COUNT_PATH)
        .expect("examples/topology_selectors/edges_count.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "expected no errors, got: {:?}",
        errors_only(&compiled)
    );
}
```

That's the entire change for one selector. The other 13 follow the same template with arity / kernel-query / result-type tweaks.

---

## 4. Open questions and risks for the remaining 13 selectors

These are honest unknowns; surfacing them up-front saves an implementer one round-trip with the planner.

### 4.1 List element typing — `Type::List(Box::new(Type::Geometry))` is conservative but lossy

`edges`, `faces`, and the seven filtered selectors all return lists of *tagged* sub-handles. Today the type system has no `Edge` / `Face` distinction beyond `Type::Geometry`. The PRD example `flat_map(adjacent_faces(b, top), |f| shared_edges(top, f))` will type-check under `Type::List(Box::new(Type::Geometry))` because both `adjacent_faces` and `shared_edges` flow through Geometry, but no static check prevents `shared_edges(edge_a, edge_b)` (which is meaningless and would fault at runtime).

**Recommendation:** ship 2699 with `Type::List(Box::new(Type::Geometry))` — it is the path of least resistance and matches what `fillet(b, edges, r)` already accepts. Tightening to a discriminated `Edge` / `Face` / `Vertex` follows in #2691 (deepened smoke) or a future PRD revision.

### 4.2 [RESOLVED] `fillet` is currently 2-arg; the 3-arg form is a separate task

The original draft of this section worried that `fillet(b, edges, r)` in `examples/topology_selectors/fillet_top_edges.ri` requires the result-type of `edges`/`faces` to compose with `fillet`'s edges-arg. Reading `crates/reify-compiler/src/geometry_modify.rs:115` resolves this: `fillet` today is **2-arg**: `fillet(target, radius)`. There is no edges-list parameter. The 3-arg form is aspirational PRD example syntax that doesn't exist yet, which is why `fillet_top_edges.ri` is parse-only.

**Resolution:** `Type::List(Box::new(Type::Geometry))` is fine as the v0.1 placeholder for `edges`/`faces`/filtered-selector results — it does not need to compose with any current `fillet` signature. The 3-arg `fillet(solid, edges, radius)` lives in a sibling task (out of #2699's scope), and `fillet_top_edges.ri` stays parse-only until that task lands. PRD task-8 amendment should explicitly note this scope boundary.

### 4.3 [RESOLVED] `adjacent_faces` / `shared_edges` reuse the existing `selector_vocabulary_v2` pattern

The original draft listed three options for face-index recovery: new `GeometryQuery::FaceIndexOf` variant / direct kernel method / parallel attribute table. **All three were unnecessary** — `crates/reify-eval/src/selector_vocabulary_v2.rs:840` (`adjacent_to_face`) and `:918` (`ancestor_faces_of_edge`) already implement face-index recovery using a fourth, simpler approach:

```rust
let faces = kernel.extract_faces(parent)?;          // idempotent cached lookup
let face_index = faces.iter().position(|id| *id == face_handle).ok_or_else(...)?;
let value = kernel.query(&GeometryQuery::AdjacentFaces { shape: parent, face_index })?;
```

`extract_faces` is **idempotent and cached** in OcctKernel (`extracted_faces: HashMap<u64, Vec<...>>` at `lib.rs:496`). The linear `position()` scan is O(n_faces) on cache miss, O(1) on subsequent dispatches against the same parent. No new `GeometryQuery` variant, no new trait method, no parallel table.

**`adjacent_faces(solid, face)` resolution.** PRD signature already takes the parent solid explicitly, so dispatch is direct:

```rust
"adjacent_faces" => {
    let parent = resolve_geometry_handle_arg(&args[0], named_steps)?;
    let face_handle = resolve_geometry_handle_arg(&args[1], named_steps)?;
    match selector_vocabulary_v2::adjacent_to_face(kernel, parent, face_handle) {
        Ok(handles) => Some(Value::List(handles.into_iter()
            .map(|h| Value::Int(h.0 as i64)).collect())),
        Err(QueryError::QueryFailed(msg)) => {
            diagnostics.push(Diagnostic::warning(msg));
            Some(Value::Undef)
        }
        Err(other) => { /* other error mapping */ }
    }
}
```

**`shared_edges(face_a, face_b)` resolution.** PRD signature does NOT include a parent — it must be derived from the face handles. Use `OwnerBody` query (already exists at `crates/reify-types/src/geometry.rs:800`):

```rust
"shared_edges" => {
    let face_a = resolve_geometry_handle_arg(&args[0], named_steps)?;
    let face_b = resolve_geometry_handle_arg(&args[1], named_steps)?;

    // Derive parent via OwnerBody; verify both faces share the same parent.
    let parent_a = match kernel.query(&GeometryQuery::OwnerBody(face_a)) {
        Ok(Value::Int(i)) => GeometryHandleId(i as u64),
        _ => return Some(Value::Undef),
    };
    let parent_b = match kernel.query(&GeometryQuery::OwnerBody(face_b)) {
        Ok(Value::Int(i)) => GeometryHandleId(i as u64),
        _ => return Some(Value::Undef),
    };
    if parent_a != parent_b {
        diagnostics.push(Diagnostic::warning(format!(
            "shared_edges: faces have different parent solids ({:?} vs {:?})",
            parent_a, parent_b
        )));
        return Some(Value::List(vec![]));
    }

    // Recover face indices and dispatch.
    let faces = match kernel.extract_faces(parent_a) {
        Ok(v) => v, Err(_) => return Some(Value::Undef),
    };
    let idx_a = faces.iter().position(|h| *h == face_a)?;
    let idx_b = faces.iter().position(|h| *h == face_b)?;

    let value = match kernel.query(&GeometryQuery::SharedEdges {
        shape: parent_a, face_a: idx_a, face_b: idx_b,
    }) {
        Ok(v) => v, Err(_) => return Some(Value::Undef),
    };

    // Map result int-indices back to edge handles via extract_edges(parent).
    let edges = match kernel.extract_edges(parent_a) {
        Ok(v) => v, Err(_) => return Some(Value::Undef),
    };
    let indices = match &value { Value::List(items) => items, _ => return Some(Value::Undef) };
    let out: Vec<Value> = indices.iter().filter_map(|item| {
        if let Value::Int(i) = item {
            edges.get(*i as usize).map(|h| Value::Int(h.0 as i64))
        } else { None }
    }).collect();
    Some(Value::List(out))
}
```

5 kernel ops on the hot path: 2× `OwnerBody`, `extract_faces`, `SharedEdges`, `extract_edges`. The `extract_*` cache makes repeat calls free.

**Cross-solid behaviour decision:** warning + empty list, *not* hard error. Matches the silent-degraded contract used elsewhere in v0.1 selector code (e.g. `try_eval_topology_selector`'s existing fallthrough path returns `Value::Undef` for malformed kernel replies rather than diagnosing).

**Net result for #2699's scope:** §4.3 was the largest open question in the original design pass; it now requires zero new infrastructure. The two topology-graph selectors fit comfortably in cluster C of the original sequencing recommendation and could even land alongside cluster A/B if the lock footprint is acceptable.

### 4.4 `faces_by_normal` / `edges_parallel_to` predicate-arg shape

These take a direction vector (`Value::Vector` of three Real or Length-dimensioned scalars) plus a tolerance angle (`Value::Scalar { dimension: ANGLE, … }`). The existing per-selector functions in `crates/reify-eval/src/topology_selectors.rs` (`pub fn faces_by_normal`, `pub fn edges_parallel_to`) take `[f64; 3]` + tolerance. The arg-extraction helper `parse_xyz_value` is already at `topology_selectors.rs:337`. Unit-handling of the angle threshold needs an `as_radians()`-style coercion mirroring the existing pattern in `try_eval_topology_selector`'s `On` arm (which hard-codes `1e-7m`); cross-reference with #2746 for the Vector3-lowering convention.

### 4.5 `edges_at_height` — height arg is a `Length` but the underlying selector takes a bbox-z extent

The eval-side helper at `crates/reify-eval/src/topology_selectors.rs:634` already takes `parse_bbox_z_extents`. The PRD calls it `edges_at_height(geometry, z: Length, tol: Length)` — single z, not an extent pair. The dispatch arm can build a `(z - tol, z + tol)` extent on the fly, but **double-check the PRD against the implementation** — if the arity intent is `(geometry, [z_min, z_max])` the existing two-extent helper applies directly; if it's `(geometry, z, tol)` a small wrapper is needed.

### 4.6 `moment_of_inertia` 3-arg axis form vs 2-arg full-tensor form

PRD §3.9 lists `moment_of_inertia(solid, density) -> Tensor<2,3,MomentOfInertia>`. The kernel has both `MomentOfInertia { handle, axis: [f64;3] }` (returning a scalar) AND `InertiaTensor { handle, density }` (returning the full 3×3 tensor). The PRD wants the tensor form, so dispatch to `InertiaTensor`. A future axis-projected overload `moment_of_inertia(solid, density, axis)` is forward-compatible without a name change.

### 4.7 Worked example `fillet_top_edges.ri` mixes #2698 and #2699 dependencies

`fillet_top_edges.ri` uses `single`, `flat_map`, `faces_by_normal`, `adjacent_faces`, `shared_edges`. `single` and `flat_map` are #2698's scope, not #2699's. The smoke test for #2699 should not block on #2698 — if `compile_with_stdlib` of `fillet_top_edges.ri` requires both, the smoke fixture for #2699 alone should be a smaller subset (e.g. just `let es = faces_by_normal(b, vec3(0,0,1), 1deg)` — no list helpers needed).

### 4.8 Lock-footprint risk

The 2699 work touches the same files Task 2324 and the kernel-query work landed in: `units.rs`, `expr.rs`, `geometry_ops.rs`, `engine_build.rs`, `crates/reify-types/src/geometry.rs` (if §4.3 needs a new query variant). These are central files. If multiple selectors land as parallel tasks (the steward's option B), serialise them or accept some merge friction — they all touch the same const list and the same `try_eval_topology_selector` match.

---

## 5. Concrete recommendation for sequencing

After this design pass lands, #2699 can be re-filed with the investigation step removed and the body pointing here. If the implementer still finds the 11-selector scope too large in one task (the steward's primary concern), the natural split is:

**Cluster A — kernel-mut widening + simplest selectors:** ~5 names plus the one-time mutability widening (see §2 cluster-A prerequisite). Covers `edges`, `faces`, `center_of_mass`, `moment_of_inertia`, plus the `try_eval_topology_selector` / `post_process_topology_selectors` signature change to `&mut dyn GeometryKernel`. Pure four-edit template per name. PRD task-8 amendment rides here (and notes the 3-arg-fillet scope boundary — see §4.2).

**Cluster B — predicate-arg filtering:** `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`. 5 names. Exercises the predicate-arg shape (vector + tolerance angle) and the unit-aware coercion patterns. Builds directly on cluster A's mutability widening.

**Cluster C — topology-graph queries:** `adjacent_faces`, `shared_edges`. 2 names. With §4.3 now resolved (reuse `selector_vocabulary_v2::adjacent_to_face` + `OwnerBody` for parent derivation), this cluster has no special blockers and can land in parallel with B if lock-footprint allows.

Each cluster is one merge. All three clusters touch the same `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` const list and the same `try_eval_topology_selector` match — serialise the merges to avoid trivial conflicts.

If the user instead wants 2699 to remain a single task, the implementer should:
- Pre-seed `metadata.memory_hints` with this doc + the four wiring sites listed in §2.
- Constrain the planner to "extend the Task 2324 pattern uniformly across N names" rather than re-deriving the dispatch.
- Cap to ≤ 1 lock domain per merge by serialising; the 9-file-touch problem the steward called out is the main reason the 121-turn architect thrashed.

**Out-of-scope sibling task to file:** 3-arg `fillet(solid, edges, radius)` — currently `fillet` is 2-arg `fillet(target, radius)` per `crates/reify-compiler/src/geometry_modify.rs:115`. The PRD example syntax that takes a curated edge list is aspirational and needs its own task, gated on #2699 (which produces the `edges`/`faces_by_normal`/`adjacent_faces`/`shared_edges` selectors that supply the edge list).

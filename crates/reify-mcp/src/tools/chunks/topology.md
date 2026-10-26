# Topology Selectors
<!-- TOPOLOGY-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/topology_chunk_smoke.rs verifies BOTH
     directions over the table below — every member of
     reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES has a row (the registry is iterated
     directly, so a selector added to the compiler and not to this table is RED on the next run),
     and every name in the table's FIRST COLUMN is a real member of that registry rather than a
     phantom or a name borrowed from a sibling family. A row-count floor guards against the table
     being reformatted into a shape the scan cannot read.

       topology_chunk_smoke.rs::topology_selector_family_documented_in_topology_chunk

     THE TABLE SHAPE IS LOAD-BEARING for that guard: a catalogue row is a `|`-leading line whose
     FIRST cell backticks the selector it is about. Rewriting this into bullets is RED. The other
     columns are free-form — nothing matches on them.

     THE CALL FORM COLUMN is therefore UNPINNED, and it is the column whose errors are silent.
     A wrong arity is not a compile error: the compiler types a topology-selector call from its
     NAME alone, and eval's arity gate — crates/reify-eval/src/geometry_ops.rs::expected_arity —
     simply declines to dispatch, so the cell stays `Value::Undef` with no diagnostic. That is
     exactly the shape of the 2026-07-24 finding (`rotate(geo, axis, angle)` documented at a
     signature the compiler had never been shown, tasks #5347 / #5364). Check an argument list
     against that arity table, or against docs/reify-stdlib-reference.md §3.9, before trusting it;
     making this column registry-checkable the way the first column is needs that arity table
     exposed outside reify-eval.

     The Result and Eval columns are transcribed from
     crates/reify-compiler/src/units.rs::topology_selector_result_type and from the eval dispatch
     in crates/reify-eval/src/geometry_ops.rs. NOTHING IN THIS HARNESS CHECKS THEM — they are
     UNPINNED prose, and a result-type change in units.rs will not turn this table red. Re-read
     both sources before relying on a cell.

     The `TOPOLOGY-SECTION` marker on the line above is what scopes the scan, matched
     byte-exactly — NOT this heading's wording. Keep it directly under the heading it opens; the
     scan runs from it to the next `##` heading. -->

**A selector is not a list.** This is the one fact to carry away before reading the table. Most of
these constructors evaluate to a symbolic `Selector` value — a *query* over a body's topology, not
the resolved sub-handles. The compiler bridges `Selector` to `List<Geometry>` by inserting a
`ResolveSelector` coercion, and it does so at exactly **three** consumption sites: binding the
selector to a function/feature **parameter**, passing it to `single()` or another list helper, and
**indexing** it (`sel[0]`). Anywhere else — a bare `let all_faces = faces(b)`, or the value handed to
something that expects a list without being one of those three sites — it is still a `Selector`, and
you get a silent wrong answer rather than an error.
Note this differs from `docs/reify-stdlib-reference.md` §3.9, which documents the *post-coercion*
surface type (`fn faces(solid: Solid) -> List<Surface>`); §3.9 describes what a consumption site
sees, this table describes what the value IS.

**`edges` and `faces` are selectors here, argument names elsewhere.** In the `geometry` chunk,
`edges` and `faces` appear only as the ARGUMENT of a fillet / chamfer / shell call
(`fillet(solid, edges, radius)`), where they name a parameter and say nothing about how to produce
one. This table is where they are documented as the selectors that produce it — that is what fills
that argument.

| Selector | Call form | Result | Eval |
|---|---|---|---|
| `faces` | `faces(solid)` | `Selector(Face)` | kernel-free mint |
| `edges` | `edges(solid)` | `Selector(Edge)` | kernel-free mint |
| `vertices` | `vertices(geometry)` | `Selector(Vertex)` | kernel-free mint |
| `mid_surface` | `mid_surface(body)` | `Selector(Face)` | kernel-free mint (shell-extract mid-surface faces) |
| `face` | `face(geometry, name)` | `Selector(Face)` | kernel-free mint (named leaf) |
| `edge` | `edge(geometry, name)` | `Selector(Edge)` | kernel-free mint (named leaf) |
| `vertex` | `vertex(geometry, name)` | `Selector(Vertex)` | kernel-free mint (named leaf) |
| `solid_body` | `solid_body(geometry, name)` | `Selector(Body)` | kernel-free mint (named leaf; `body` is the RBD mechanism ctor, not this) |
| `faces_by_area` | `faces_by_area(solid, range)` | `Selector(Face)` | kernel-free mint |
| `edges_by_length` | `edges_by_length(solid, range)` | `Selector(Edge)` | kernel-free mint |
| `faces_by_normal` | `faces_by_normal(solid, direction, tol)` | `Selector(Face)` | kernel-free mint; `tol` is an **Angle** |
| `edges_parallel_to` | `edges_parallel_to(solid, direction, tol)` | `Selector(Edge)` | kernel-free mint; `tol` is an **Angle** |
| `edges_at_height` | `edges_at_height(solid, height, tol)` | `Selector(Edge)` | kernel-free mint; both are **Length** |
| `faces_perpendicular_to` | `faces_perpendicular_to(solid, direction, tol)` | `Selector(Face)` | kernel-free mint |
| `edges_perpendicular_to` | `edges_perpendicular_to(solid, direction, tol)` | `Selector(Edge)` | kernel-free mint |
| `faces_by_surface_kind` | `faces_by_surface_kind(solid, kind)` | `Selector(Face)` | kernel-free mint |
| `edges_by_curve_kind` | `edges_by_curve_kind(solid, kind)` | `Selector(Edge)` | kernel-free mint |
| `extremal_by_bbox` | `extremal_by_bbox(solid, axis, sense, tol)` | `Selector(Face)` | kernel-free mint |
| `extremal_by_centroid` | `extremal_by_centroid(solid, axis, sense, tol)` | `Selector(Face)` | kernel-free mint |
| `created_by_feature` | `created_by_feature(solid, f)` | `Selector(Face)` | kernel-free mint; `f` comes from `feature(g)` |
| `split_by_feature` | `split_by_feature(solid, f)` | `Selector(Face)` | kernel-free mint; matches a split at ANY history position |
| `adjacent_faces` | `adjacent_faces(solid, face)` | `List<Geometry>` | kernel-bearing — needs a realized handle |
| `shared_edges` | `shared_edges(face1, face2)` | `List<Geometry>` | kernel-bearing |
| `siblings_of_face` | `siblings_of_face(parent, face)` | `List<Geometry>` | kernel-bearing (every face of `parent` except `face`) |
| `ancestor_faces_of_edge` | `ancestor_faces_of_edge(parent, edge)` | `List<Geometry>` | kernel-bearing (the faces owning `edge`) |
| `split` | `split(solid, plane)` | `List<Geometry>` | kernel-bearing |
| `closest_point` | `closest_point(point, geometry)` | `Point3<Length>` | kernel-bearing |
| `is_on` | `is_on(point, geometry)` | `Bool` | kernel-bearing |
| `angle_between_surfaces` | `angle_between_surfaces(a, b)` | `Angle` | kernel-bearing |
| `center_of_mass` | `center_of_mass(solid, density)` | `Point3<Length>` | kernel-bearing; `density` must be **dimensioned** |
| `moment_of_inertia` | `moment_of_inertia(solid, density)` | `Tensor<2, 3, MomentOfInertia>` | kernel-bearing; `density` must be **dimensioned** |

**Reading the Eval column.** *Kernel-free mint* means the value is produced without OCCT: the
selector is symbolic, so it survives a kernel-less `reify eval` and only its RESOLUTION needs a
realized body. *Kernel-bearing* means the call resolves against a realized handle during
`reify build`, so under `reify eval` / `reify check` — or with OCCT unavailable — the cell stays
`undef` and a constraint over it reads `INDETERMINATE`. The same let-bind-the-operand arg-shape rule
from the `measurement` chunk applies to every row.

The relational rows (`adjacent_faces`, `shared_edges`, `siblings_of_face`,
`ancestor_faces_of_edge`) take a face or edge HANDLE as their second argument, not a selector. The
canonical way to produce one is `single(...)` over a selector, inlined at the call site —
`adjacent_faces(body, single(faces_by_normal(body, up, 1deg)))`.

**Worked reference:** `examples/topology_selectors/all_topology_selectors_wiring.ri` wires the
family end-to-end; read it as a runnable example rather than as normative prose, since its own
header still describes eval dispatch as pending. `docs/reify-stdlib-reference.md` §3.9's "Topology
selectors" block holds the normative signatures, which this table deliberately does not restate.

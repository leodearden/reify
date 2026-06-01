# Capability manifest — geometry-primitive-constructors

Mechanizes G3 + G6 per leaf for `docs/prds/geometry-primitive-constructors.md`. Built at
decompose time (2026-06-01); every binding below resolved **PASS** — no FAIL
(`declared-only`/`test-only`/`producer-downstream`/`producer-absent`/`fixture-ERROR`/`bound≤floor`).
Empty-value sentinel: `Value::Undef`. Production entry paths grepped on `main` @ `14f6123aaa`.

## α — dimensionality refinement + `Planar` + profile-precondition diagnostic

| Capability | Evidence | Verdict |
|---|---|---|
| Per-op trait inference table to extend | `grep:crates/reify-compiler/src/geometry_traits_inference.rs:100` `pub struct InferredTraits {bounded,connected,convex}` — wired into compile (producer task 2315/2312 `done`, on main) | PASS (wired) |
| `geometry_traits.ri` marker file to add `Planar`/dim markers | `grep:crates/reify-compiler/stdlib/geometry_traits.ri:15-44` seven traits declared (producer task 2297 `done`) | PASS |
| Use-site diagnostic pattern to mirror | `grep:crates/reify-compiler/src/conformance/mod.rs:377` `pub(crate) fn emit_geometry_unbounded` + `DiagnosticCode::GeometryUnbounded` | PASS (wired) |
| Profile-consumer compiler arms to hook | `grep:crates/reify-compiler/src/geometry.rs:999` extrude, `:1047` revolve, `:1121` sweep, `:955` loft | PASS (wired) |
| No novel syntax (grammar) | constructor calls + type annotations already parse; no new production | PASS (G3 grammar N/A) |
| Signal premise (rejection diagnostic + permissive accept) | end-to-end capability delivered by α itself; no downstream dep | PASS (branch-3 self-delivered) |

## β — `cone`

| Capability | Evidence | Verdict |
|---|---|---|
| `BRepPrimAPI_MakeCone` substrate | same OCCT `BRepPrimAPI` module as `MakeBox`/`MakeCylinder`/`MakeSphere` already used (`crates/reify-kernel-occt/cpp/occt_wrapper.cpp:16-17,301-338`) | PASS |
| `make_cone` FFI + `GeometryOp::Cone` + kernel arm | β's own deliverable (op-execute seam, exhaustive match — adding a variant forces the kernel arm at the same diff) | PASS (self-delivered) |
| volume ≤2% of `(π/3)·h·(r1²+r1r2+r2²)` | floor: OCCT analytic-solid volume error ≪2%; achievability basis = existing `assert_volume_near(…,0.02,…)` torus tests pass at this tol | PASS (`bound>floor`) |

## γ — `torus`

| Capability | Evidence | Verdict |
|---|---|---|
| `BRepPrimAPI_MakeTorus` substrate | OCCT `BRepPrimAPI`; revolve-based torus already validated (`grep:crates/reify-kernel-occt/src/lib.rs:6320` `assert_volume_near(…,0.02,"circle torus full")`) | PASS |
| `make_torus` FFI + `GeometryOp::Torus` + kernel arm | γ's own deliverable | PASS (self-delivered) |
| volume ≤2% of `2π²·R·r²`; `convex=false` inferred | achievability basis = the existing 0.02 torus-volume test | PASS (`bound>floor`) |

## δ — `wedge`

| Capability | Evidence | Verdict |
|---|---|---|
| `BRepPrimAPI_MakeWedge` substrate | OCCT `BRepPrimAPI` | PASS |
| `make_wedge` FFI + `GeometryOp::Wedge` + kernel arm | δ's own deliverable | PASS (self-delivered) |
| volume ≤2% vs trapezoidal-prism closed form | analytic; OCCT volume error ≪2% | PASS (`bound>floor`) |

## ε — `cylinder_centered` + `box_centered`

| Capability | Evidence | Verdict |
|---|---|---|
| existing `Cylinder` + `Translate` ops to compose | `grep:crates/reify-kernel-occt/src/lib.rs:1814` Cylinder execute, `:1908` Translate execute | PASS (wired) |
| `box` already centroid-centred (alias correctness) | `grep:crates/reify-kernel-occt/cpp/occt_wrapper.cpp:303` `gp_Pnt corner(-w/2,-h/2,-d/2)` | PASS |
| centroid z≈0 / mesh-identity signals | composition + alias, no new substrate | PASS (self-delivered) |

## ζ — `rectangle` + `circle`

| Capability | Evidence | Verdict |
|---|---|---|
| `make_circle_face` to expose | `grep:crates/reify-kernel-occt/src/ffi.rs:564` `fn make_circle_face(radius,z_height)` + `occt_wrapper.h:675` | PASS (wired) |
| `make_rectangle_face` + profile `GeometryOp` variants | ζ's own deliverable | PASS (self-delivered) |
| `dimension=Surface` inference for the new constructors | `producer:task-α` upstream (dep wired ζ→α); α is upstream of ζ | PASS (DAG-direction ✓) |
| `extrude(rectangle(20mm,10mm),3mm)` volume ≈ area·h ≤2% | analytic prism; OCCT error ≪2% | PASS (`bound>floor`) |

## η — `polygon` + `ellipse`

| Capability | Evidence | Verdict |
|---|---|---|
| `List<Point2<Length>>` point-list surface | `producer:task-320` (`done`, curve constructors `interp`/`bezier` accept point lists) | PASS (wired) |
| `make_polygon_face`/`make_ellipse_face` + variants | η's own deliverable | PASS (self-delivered) |
| `dimension=Surface` inference | `producer:task-α` upstream (dep wired η→α) | PASS (DAG-direction ✓) |
| `extrude(polygon([…]),h)` vol ≈ shoelace-area·h ≤2% | analytic; OCCT error ≪2% | PASS (`bound>floor`) |

## θ — docs + LSP completion + task-303 reconciliation

| Capability | Evidence | Verdict |
|---|---|---|
| LSP `BUILTIN_FUNCTIONS` list to extend | `grep:crates/reify-lsp/src/completion.rs:296` `const BUILTIN_FUNCTIONS` | PASS (wired) |
| `reify-stdlib-reference.md` §3.2 to correct | `grep:docs/reify-stdlib-reference.md:309-347` | PASS |
| task 303 to reconcile | `get_task(303)` = `done` phantom-done (no implementing commit) | PASS |
| all constructors landed before documenting | `producer:{β,γ,δ,ε,ζ,η}` all upstream (deps wired θ→each) | PASS (DAG-direction ✓) |

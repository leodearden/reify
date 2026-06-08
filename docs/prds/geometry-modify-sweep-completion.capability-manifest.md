# Capability manifest — geometry-modify-sweep-completion

Mechanizes G3 + G6 per leaf for `docs/prds/geometry-modify-sweep-completion.md`. Built at decompose
time (2026-06-02); every binding below resolved **PASS** — no FAIL
(`declared-only`/`test-only`/`producer-downstream`/`producer-absent`/`fixture-ERROR`/`bound≤floor`).
Empty-value sentinel: `Value::Undef` (and the kernel's `OperationFailed`). Production entry paths
grepped on `main` @ `c3f269f8c7`. Op-execute seam = the exhaustive modify/sweep match in
`crates/reify-kernel-occt/src/lib.rs:2002-2419` (adding a `GeometryOp` variant forces a kernel arm at
the same diff — no orphan-op possible).

## α — curated edge/face selection seam (re-homes 3205)

| Capability | Evidence | Verdict |
|---|---|---|
| `edges`/`faces`/`edges_at_height`/`faces_by_normal` resolve to `List<Geometry>` sub-handles | `producer:task-3616`(KGQ-η,`done`) + `task-3560`(`done`); resolution `grep:crates/reify-eval/src/topology_selectors.rs:572-864`, dispatch `try_eval_topology_selector` `geometry_ops.rs:~1664` | PASS (wired on main) |
| existing all-edges `Fillet` to extend with `edges` | `grep:crates/reify-ir/src/geometry.rs:552` `Fillet { target, radius }`; kernel `lib.rs:2010` `fillet_all_edges`; cpp `occt_wrapper.cpp:1659` | PASS (wired) |
| per-edge `BRepFilletAPI_MakeFillet::Add(r,edge)` | `BRepFilletAPI_MakeFillet` already `#include`d (`occt_wrapper.h:27`); per-edge `Add` is α's own FFI deliverable on the op-execute seam | PASS (self-delivered) |
| `resolve_subhandle_list` helper (decode KGQ sub-handle, verify of-parent) | α's own deliverable; reuses KGQ `SubKind` decode (open question 2) | PASS (self-delivered) |
| anti-zero-edges: empty selection → blocking diagnostic, not silent all-edges | α's own deliverable; closes the documented trap `task-3295`(`pending`, depends on α) | PASS (self-delivered; numeric floor = resolved-set ≠ 0 ∧ ≠ all) |
| two-way signal (resolved set == 4 ∧ volume ≠ unfilleted ∧ ≠ `fillet_all`) | volumes are analytic OCCT solids; the "≠ all-edges" delta is gross (4 of 12 edges) ≫ any tol | PASS (self-delivered, `bound>floor`) |
| no novel syntax (grammar) | 3-arg `fillet(b, edges_at_height(...), 2mm)` parses (`reify check /tmp/prd-gate-fixtures/modify-sweep-1.ri` → only semantic arity error) | PASS (G3 N/A) |

## β — per-edge `chamfer` + `chamfer_asymmetric` (supersedes phantom-done 315)

| Capability | Evidence | Verdict |
|---|---|---|
| selection helper to reuse | `producer:task-α` upstream (dep wired β→α) | PASS (DAG-direction ✓) |
| `BRepFilletAPI_MakeChamfer::Add(d,edge)` / `Add(d1,d2,edge,face)` | `BRepFilletAPI_MakeChamfer` `#include`d (`occt_wrapper.h:28`); all-edges form live (`lib.rs:2021` `chamfer_all_edges`, cpp `:1675`); per-edge + asymmetric `Add` is β's FFI deliverable | PASS (self-delivered) |
| `ChamferAsymmetric` IR variant + kernel arm | β's deliverable on the op-execute seam (open question 4) | PASS (self-delivered) |
| asymmetric setbacks 1mm:2mm within 5% (bbox/section) | OCCT chamfer-face geometry is exact; 5% ≫ meshing error | PASS (`bound>floor`) |
| phantom-done 315 absent on main (no silent re-ship) | `rg chamfer_asymmetric crates/` = 0 production hits; `get_task(315)`=`done` (no implementing commit) | PASS (regression confirmed) |

## γ — `shell_open(solid, thickness, open_faces)` (supersedes phantom-done 316/shell_open)

| Capability | Evidence | Verdict |
|---|---|---|
| `Shell` op + `MakeThickSolid` face-removal already wired | `grep:crates/reify-ir/src/geometry.rs:765` `Shell { target, thickness, faces_to_remove }`; eval `geometry_ops.rs:446-501`; kernel `lib.rs:2174`; cpp `occt_wrapper.cpp:2035` `MakeThickSolid` | PASS (wired) |
| selection helper to map `open_faces:List<Geometry>` → face sub-handles | `producer:task-α` upstream (dep wired γ→α) | PASS (DAG-direction ✓) |
| `shell_open` named fn + face-handle removal | γ's deliverable (extends existing index-based `faces_to_remove` to sub-handles) | PASS (self-delivered) |
| hollow-box-open-top: 1 face removed, wall vol within 3% | OCCT thick-solid volume analytic; 3% covers thin-wall meshing | PASS (`bound>floor`) |
| phantom-done 316 absent | `rg shell_open crates/` = 0 production hits | PASS (regression confirmed) |

## δ — `draft(solid, faces, angle, neutral_plane)` (4-arg face-selection form)

| Capability | Evidence | Verdict |
|---|---|---|
| `Draft` op + `DraftAngle` + neutral-plane arg already wired | `grep:crates/reify-ir/src/geometry.rs:754` `Draft { target, angle, plane }`; eval `geometry_ops.rs:502-518`; cpp `occt_wrapper.cpp:2071` `DraftAngle` | PASS (wired) |
| `Plane` value constructible (neutral_plane) | `grep:crates/reify-stdlib/src/geometry.rs:804-811` `plane_xy/xz/yz`; `make_plane:1091` | PASS (wired — gap-register "missing" grepped only compiler/eval) |
| selection helper for `faces:List<Geometry>` | `producer:task-α` upstream (dep wired δ→α) | PASS (DAG-direction ✓) |
| per-face `DraftAngle::Add` (4-arg form) | δ's deliverable (today's draft is 3-arg, all-faces) | PASS (self-delivered) |
| drafted +X face normal tilts 3°±0.1°, others unchanged | OCCT draft is exact-angle; 0.1° ≫ numeric noise | PASS (`bound>floor`) |

## ε — `offset_solid` + `fillet_all` (supersedes phantom-done 316/offset_solid)

| Capability | Evidence | Verdict |
|---|---|---|
| `BRepOffsetAPI_MakeOffsetShape` substrate | already `#include`d (`occt_wrapper.h:41`) + used by `thicken` (`occt_wrapper.cpp:2007`, ffi `ffi.rs:584`) | PASS (wired) |
| `offset_solid` FFI + `GeometryOp::OffsetSolid` + kernel arm | ε's deliverable on the op-execute seam | PASS (self-delivered) |
| `fillet_all` = alias to all-edges `Fillet` | lowers to existing `Fillet { edges: [] }` (`lib.rs:2010`); no new kernel path | PASS (wired) |
| offset± volume monotonicity; over-shrink → diagnostic | OCCT offset volume analytic; gross sign change | PASS (`bound>floor`) |
| phantom-done 316 absent | `rg offset_solid crates/` = 0 production hits | PASS (regression confirmed) |

## ζ — `split(solid, tool: Plane) → List<Solid>` (supersedes phantom-done 316/split)

| Capability | Evidence | Verdict |
|---|---|---|
| `Plane` value constructible (cutting tool) | `grep:crates/reify-stdlib/src/geometry.rs:804-811` `plane_xy` etc. | PASS (wired) |
| multi-output `List<Geometry>` value shape | selector path already yields `List<Geometry>` (KGQ, `task-3616`); `split` reuses it (open question 1) | PASS (wired) |
| `BRepAlgoAPI_Splitter` FFI + `GeometryOp::Split` + kernel arm | **absent today** (`rg BRepAlgoAPI_Splitter` = 0) — new `#include` + `split_shape` FFI is ζ's deliverable | PASS (self-delivered; new FFI, not an unmet assumption) |
| split-into-2 each vol ≈ 500mm³ within 2%; non-intersecting → len-1 | analytic half-volumes; OCCT boolean error ≪2% | PASS (`bound>floor`) |

## η — `nurbs_surface(...) → Surface` (first free-standing Surface producer)

| Capability | Evidence | Verdict |
|---|---|---|
| `dimension=Surface` / `Planar` inference record | `producer:task-4155`(`done`); `grep:crates/reify-compiler/src/geometry_traits_inference.rs:115-151` `GeomDim {Curve,Surface,Solid}`+`planar` | PASS (wired on main) |
| nested `List<List<Point3<Length>>>` + scalar args parse | `reify check /tmp/prd-gate-fixtures/modify-sweep-2.ri` → no parse error | PASS (G3 N/A) |
| `Geom_BSplineSurface`→face FFI + `GeometryOp::NurbsSurface` + kernel arm + inference arm | η's deliverable on the op-execute seam; signature per PRD decision 5 (mirrors `nurbs` curve, `task-320` `done`) | PASS (self-delivered) |
| sampled point on patch within tol; non-`Closed` → `GeometryProfileRequired` as a profile | `emit_geometry_profile_required` wired (`grep:crates/reify-compiler/src/conformance/mod.rs:444-484`) | PASS (`bound>floor`) |

## θ — `offset_surface(surface, distance) → Surface`

| Capability | Evidence | Verdict |
|---|---|---|
| input `Surface` producer (rectangle) | `producer:task-4160`(primitives PRD, `pending`) — **hard dep wired θ→4160** | PASS (DAG-direction ✓; θ blocked until 4160 lands — honest, not fake-done) |
| `MakeOffsetShape` surface-mode FFI + `GeometryOp::OffsetSurface` + `dimension=Surface` inference | θ's deliverable; `MakeOffsetShape` `#include`d (`occt_wrapper.h:41`) | PASS (self-delivered) |
| centroid offset +2mm along normal; area ≈ input within 2% | OCCT offset analytic | PASS (`bound>floor`) |

## ι — `offset_curve(curve, distance)` (3 overloads)

| Capability | Evidence | Verdict |
|---|---|---|
| `Curve` producers (`arc`/`line_segment`/…) | `producer:task-320`(`done`) | PASS (wired) |
| `vec3` value for the direction overload | `grep:crates/reify-stdlib/src/geometry.rs:923` `vec3`→`Value::Vector` (`:1068-1088`); passable as arg | PASS (wired) |
| reference-Surface for overload 2 | a `faces()` sub-handle (KGQ, live) or η; not a hard new dep | PASS (wired) |
| `BRepOffsetAPI_MakeOffset` FFI + `GeometryOp::OffsetCurve` (3 arg-shapes) + kernel arm | ι's deliverable | PASS (self-delivered) |
| offset arc r=10→12mm within 2%; all 3 overloads non-`Undef` | analytic planar offset | PASS (`bound>floor`) |

## κ — `thicken_asymmetric(surface, above, below) → Solid`

| Capability | Evidence | Verdict |
|---|---|---|
| input `Surface` producer (rectangle) | `producer:task-4160`(primitives, `pending`) — **hard dep wired κ→4160** | PASS (DAG-direction ✓; blocked until 4160) |
| symmetric `thicken` precedent | `grep:crates/reify-kernel-occt/src/lib.rs:2168` `Thicken`→`thicken_shape` (ffi `:584`, cpp `:2007`) | PASS (wired) |
| two-sided offset FFI + `GeometryOp::ThickenAsymmetric` + kernel arm | κ's deliverable | PASS (self-delivered) |
| total thickness 3mm, bbox z∈[−2,+1]mm, vol within 3% | analytic; asymmetry is gross | PASS (`bound>floor`) |

## λ — `extrude_to(profile, target: Surface) → Solid`

| Capability | Evidence | Verdict |
|---|---|---|
| profile producer (circle) | `producer:task-4160`(primitives, `pending`) — **hard dep wired λ→4160** | PASS (DAG-direction ✓; blocked until 4160) |
| target `Surface` (tilted) | `producer:task-η`(this batch) or a `faces()` sub-handle (KGQ, live) — dep wired λ→η (open question 3) | PASS (DAG-direction ✓) |
| `BRepFeat_MakePrism` "until" / prism+`Cut` FFI + `GeometryOp::ExtrudeTo` + kernel arm | λ's deliverable | PASS (self-delivered) |
| top face conforms to target; max height within 5%; non-overlap → diagnostic | OCCT feature/boolean error ≪5% | PASS (`bound>floor`) |

## μ — docs + LSP completion + 315/316 reconciliation

| Capability | Evidence | Verdict |
|---|---|---|
| LSP completion list to extend | `grep:crates/reify-lsp/src/completion.rs` `BUILTIN_FUNCTIONS` (primitives PRD θ cites `:296`) | PASS (wired) |
| `reify-stdlib-reference.md` §3.5-3.6 to correct | `grep:docs/reify-stdlib-reference.md:373-405` | PASS |
| 315/316 to reconcile | `get_task(315)`/`get_task(316)` = `done` phantom-done (deliverables absent on main) | PASS |
| all ops landed before documenting | `producer:{α,β,γ,δ,ε,ζ,η,θ,ι,κ,λ}` all upstream (deps wired μ→each) | PASS (DAG-direction ✓) |

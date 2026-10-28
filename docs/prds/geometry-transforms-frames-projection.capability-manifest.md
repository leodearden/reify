# Capability manifest — geometry-transforms-frames-projection

**AS-AUTHORED GATE ARTIFACT (2026-06-01) — do not refresh.** This manifest records the pre-decomposition evidence check that cleared this PRD's leaves to queue; its verdicts are statements about binding quality at author time, not about landed state. All leaves have since landed (α #4164 … ι #4172, plus #4879) — see the parent PRD's SHIPPED header (`docs/prds/geometry-transforms-frames-projection.md`). Its forward-looking phrasings and hard `file:line` anchors are 2026-06-01 provenance and may have drifted; rewriting them would destroy the record of what was actually gated. Mechanizes G3 + G6 per leaf for `docs/prds/geometry-transforms-frames-projection.md`. Built at
decompose time (2026-06-01); every binding below resolved **PASS** — no FAIL
(`declared-only`/`test-only`/`producer-downstream`/`producer-absent`/`fixture-ERROR`/`bound≤floor`).
Empty-value sentinel: `Value::Undef`. Substrate verified empirically against `target/debug/reify`
(built 2026-06-01 12:37) and grepped on `main` @ `7eb5b1bab7`.

> **Premise correction (logged for the gate trail).** The gap register's HIGH claims that
> `plane_*`/`axis_*` constructors and `Plane`/`Axis` constructibility are *missing* are **FALSE** —
> verified live (`plane_xy(5mm)` → `plane(point(0,0,0.005),vec(0,0,1))`; `axis_z(...)` → real
> `Value::Axis`). The survey missed the `reify-stdlib` value-builtin layer. This manifest therefore
> binds the *real* gaps (orphan values + the genuinely-`Undef` transforms), not the phantom ones.

## α — `apply_transform(geometry, Transform<3>)`

| Capability | Evidence | Verdict |
|---|---|---|
| rigid `GeometryOp::ApplyTransform` op + kernel exec to consume | `grep:crates/reify-ir/src/geometry.rs:604` `ApplyTransform {rotation:[f64;4],translation:[f64;3]}`; kernel arm `crates/reify-kernel-occt/src/lib.rs:2110`; FFI `apply_transform_to_shape` `ffi.rs:454` — producer task **3901** `done`, on main `623dc77d8d` | PASS (wired) |
| `Value::Transform { rotation, translation }` to decode | `grep:crates/reify-ir/src/value.rs:905-908`; constructed by `transform3` `reify-stdlib/src/geometry.rs:179-196` (verified live: `transform3(...)` → real Transform) | PASS (wired) |
| compiler name-registration + arm template | `grep:crates/reify-compiler/src/units.rs` `GEOMETRY_FUNCTION_NAMES` (translate/rotate/scale ~47-49); `geometry_transform.rs:8-107` arm pattern | PASS (wired) |
| no novel syntax | `apply_transform(g, t)` is a plain call — parses today (probe: silently accepted, evals `Undef`) | PASS (G3 N/A) |
| signal premise: rigid ⇒ volume-preserving within 0.1% | floor: `gp_Trsf` is an exact B-rep isometry (zero geometric error); 0.1% is mesh-tessellation slack ≫ floor; capability self-delivered (α + landed 3901) | PASS (`bound>floor`, self-delivered) |

## β — `project(point, Frame<3>)` + `project(vector, Frame<3>)`

| Capability | Evidence | Verdict |
|---|---|---|
| `Value::Frame { origin, basis }` to decode | `grep:crates/reify-ir/src/value.rs:900-903`; `frame3` constructor `reify-stdlib/src/geometry.rs:159-176` (verified live) | PASS (wired) |
| quaternion-rotate + frame-decode template to mirror | `grep:crates/reify-stdlib/src/geometry.rs:218-309` `frame_to_frame` (origin+basis decode); `quat_rotate` `orientation.rs:595` | PASS (wired) |
| generic builtin dispatch reaches `eval_geometry` | `grep:crates/reify-stdlib/src/lib.rs:106` `geometry::eval_geometry(name,args)` on the live eval path | PASS (wired) |
| signal premise: world→local correctness | pure quaternion algebra (no kernel, no numeric floor); `project(point3(1,2,3), frame3(point3(1,0,0), id)) ≈ point3(0,2,3)` is exact | PASS (self-delivered) |

## γ — `rotate(geometry, Orientation<3>)` overload

| Capability | Evidence | Verdict |
|---|---|---|
| existing `Rotate` op + `rotate_shape` FFI to reuse | `grep:crates/reify-ir/src/geometry.rs:570` `Rotate{axis,angle_rad}`; eval `geometry_ops.rs:546`; FFI `ffi.rs:420` | PASS (wired) |
| `Value::Orientation` to decode → axis-angle | `grep:crates/reify-ir/src/value.rs:892-898`; `orient_log`/axis-angle helpers `reify-stdlib/src/orientation.rs` | PASS (wired) |
| arg-count dispatch site | `grep:crates/reify-compiler/src/geometry_transform.rs:37-55` existing 5-arg `rotate` arm (probe confirms hard 5-arg check) | PASS (wired) |
| signal premise: 2-arg ≡ axis+angle bbox-equal | quaternion→axis-angle is exact; no numeric floor | PASS (self-delivered) |

## δ — `scale(geometry, Vector3<Real>)` per-axis overload

| Capability | Evidence | Verdict |
|---|---|---|
| non-rigid `gtransform_shape` FFI (`gp_GTrsf`) to consume | `grep:crates/reify-kernel-occt/src/ffi.rs:479` `gtransform_shape(shape,m00..m22,tx,ty,tz)` — producer task **3959** `done` | PASS (wired) |
| existing uniform `scale` arm to extend (arg-shape dispatch) | `grep:crates/reify-compiler/src/geometry_transform.rs:57-72`; eval `geometry_ops.rs:554` | PASS (wired) |
| new `GeometryOp::ScaleNonUniform` + kernel arm | δ's own deliverable (op-execute seam; exhaustive match forces the kernel arm at the same diff) | PASS (self-delivered) |
| no collision with AffineMap `AffineApply` (3963) | 3963 description: "do NOT redirect the existing uniform scale op through gp_GTrsf" — the `scale(Vector3)` overload is explicitly unowned by AffineMap | PASS (anti-inversion: distinct surface) |
| signal premise: vol = ∏factors·V₀ within 0.1% | floor: `gp_GTrsf` diagonal is exact at B-rep (task 3959's landed `diag(1,1,2)` kernel test asserts exactness); 0.1% ≫ floor | PASS (`bound>floor`) |

## ε — `arbitrary_pattern(geometry, List<Transform<3>>)` (supersede 323)

| Capability | Evidence | Verdict |
|---|---|---|
| current translation-only impl to extend (not regress) | `grep:crates/reify-eval/src/geometry_ops.rs:774-811` parses `t{i}_dx/dy/dz` triples; IR `ArbitraryPattern{transforms:Vec<[f64;3]>}` `geometry.rs:640` — task **323** `done` (phantom-partial) | PASS (wired; supersede target identified) |
| per-instance rigid transform op to compose | landed `ApplyTransform` op (3901, see α) | PASS (wired) |
| `List<Transform>` literal arg parses | verified live: `arbitrary_pattern(b, [transform3(...), ...])` rejected only by the arg-count arm (semantic), not the parser — list literal parses (same grammar as `interp([...])`) | PASS (G3 N/A) |
| back-compat: triple form keeps working | `examples/pattern_composition.ri:74` `arbitrary_pattern(w, 10,0,0, 0,20,0)` is the regression guard | PASS (guard named) |
| signal premise: rotated instance bbox ≠ un-rotated | per-instance rigid `ApplyTransform` (exact); self-delivered | PASS (self-delivered) |

## ζ — `orient_look_at` + `EulerConvention` enum-value path

| Capability | Evidence | Verdict |
|---|---|---|
| `orient_basis` to reuse for look-at | `grep:crates/reify-stdlib/src/orientation.rs:89-161` (orthonormal-basis → quaternion, Shepperd's method) | PASS (wired) |
| `orient_euler` arm to extend (accept enum value) | `grep:crates/reify-stdlib/src/orientation.rs:46-88` (currently lowercase-string only) | PASS (wired) |
| qualified enum-value lowering substrate | verified live: `OutputFormat.STEP` → `OutputFormat::STEP` (real `Value::Enum`) — tasks **2525/2558/4108** `done`; new enum NAME seeded via prelude self-compile (4108 mechanism) | PASS (wired, verified) |
| enum decl grammar | `enum EulerConvention {…}` parses under the real compiler (probe: only "unresolved XYZ" = semantic seeding, not grammar); precedents `io.ri:27` `OutputFormat`, `materials_mechanical.ri:93` `HardnessScale` | PASS (G3 N/A) |
| signal premise: `EulerConvention.XYZ` ≡ `"xyz"` | both map to the same convention dispatch; exact quaternion equality | PASS (self-delivered) |

## η — Plane/Axis value consumers + shared decode helper

| Capability | Evidence | Verdict |
|---|---|---|
| `Value::Plane`/`Value::Axis` to decode (producer NOT orphan after this) | `grep:crates/reify-ir/src/value.rs:910-918`; produced by `plane_*`/`axis_*` `reify-stdlib/src/geometry.rs:597-604` (verified live) | PASS (wired; closes orphan) |
| current `mirror`/`circular_pattern` arms to extend | `grep:crates/reify-compiler/src/geometry.rs:847-899` (circular_pattern 9-arg, mirror 7-arg); eval `geometry_ops.rs:631-708` — probe confirms "expects 7/9 arguments" on value args | PASS (wired) |
| no novel syntax | `mirror(b, plane_xy(0mm))` parses today (rejected at arg-count, semantic) | PASS (G3 N/A) |
| two-way boundary (decode round-trip + wrong-variant reject) | H component: `decode_plane(plane_xy(z))≡([0,0,z],[0,0,1])`; `mirror(box, axis_z(...))` → typed diagnostic | PASS (self-delivered) |
| cross-PRD: half_space (3465) consumes the decode helper | 3465 `pending`, design open ("design the surface in this task"); seam is soft (3465 may land independently) | PASS (no hard dep) |

## θ — runtime trait queries `is_closed`/`is_connected`/`is_bounded` + `Geometry`/`Transformable`

| Capability | Evidence | Verdict |
|---|---|---|
| runtime conformance-query path to extend | `grep:crates/reify-eval/src/geometry_ops.rs:1331-1429` `try_eval_conformance_query` (is_watertight/is_manifold/is_orientable) — tasks **2320/2318** `done` | PASS (wired) |
| `GEOMETRY_QUERY_HELPER_NAMES` + `GeometryQuery` enum to add to | `grep:crates/reify-compiler/src/units.rs:75`; `reify-ir/src/geometry.rs` `IsWatertight/IsManifold/IsOrientable` (~924-937) | PASS (wired) |
| OCCT predicates for the 3 (closed/connected/bounded) | `BRepCheck_Analyzer` closedness (already used by is_watertight, FFI 2318); shape-connectivity + bbox-finiteness are tractable kernel queries | PASS (wired/tractable) |
| `is_convex` deferred (no false-premise) | `BRepCheck_Analyzer` does NOT test convexity; real test = convex-hull comparison → out of scope, documented | PASS (G6: avoided `bound≤floor`) |
| `Geometry`/`Transformable` markers | `grep:crates/reify-compiler/stdlib/geometry_traits.ri:9-10` file explicitly defers these "to a follow-up task" — this is it | PASS |
| soft-coordinate with 4155 on `geometry_traits.ri` | 4155 adds `Planar`/dim markers to same file; disjoint lines | PASS (soft seam) |

## ι — docs + LSP + gap-register/task-323 reconciliation

| Capability | Evidence | Verdict |
|---|---|---|
| stdlib-reference §3.1/3.7/3.8/3.10 to correct | `grep:docs/reify-stdlib-reference.md:239-240,402,405-406,418,527-532` | PASS |
| LSP completion list to extend | `grep:crates/reify-lsp/src/completion.rs` `BUILTIN_FUNCTIONS` (sibling PRD cites `:296`) | PASS (wired) |
| gap register P6/P7 rows to annotate | `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` G-C rows | PASS |
| task 323 to reconcile | `get_task(323)` = `done` phantom-partial (translation-only) | PASS |
| all mechanisms landed before documenting | `producer:{α,β,γ,δ,ε,ζ,η,θ}` all upstream (deps wired ι→each) | PASS (DAG-direction ✓) |

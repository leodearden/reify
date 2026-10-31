# Direction typing — team study record (2026-10-29)

**Dated snapshot.** Measured at main `ba2ae8e34e` (code) with a debug binary built 2026-10-29 13:09; task state as of 2026-10-29. Do not re-anchor. The live contract is `docs/prds/v0_6/direction-typing.md`; this note is the evidence it was decided on. Seat outputs and the 316-cell acceptance-matrix probe files lived under `/tmp/dir-study/` (ephemeral); everything load-bearing is reproduced here.

Run as `/team --fable` (seven seats: core types, acceptance matrix, consumers/kernel, corpus census, history, skeptic, fable critic; ≈1.5M subagent tokens). Rulings taken from it: Leo 2026-10-29/30, recorded in the PRD §3.

---

Main ff417c7c9c (code ba2ae8e34e). Evidence: /tmp/dir-study/seats/S1..S7. Probes: /tmp/dir-study/ (gen.py / run.py rerun the 316-cell acceptance matrix in ~9 min).

## 0. What this asks Leo to do

This recommendation asks Leo to **reverse two of his own rulings**:
- **#6090 (Leo, 2026-08-07, pending):** "Keep BOTH spellings … Unify-on-Direction contradicts 5848/5955/decision 7 and is unimplementable today (no constructor, no conformance, no arithmetic; 14 fields + corpus). Delete-Direction un-decides a locked design and erases the only unit-norm carrier."
- **#5848 (steward ruling, done a2a451675a):** "A direction/axis field is typed DIMENSIONLESS (`Vec3<Dimensionless>`), NOT `Vec3<Length>` and NOT a new `Direction` type." Its two grounds (the grammar gap had closed; Invariant V, dimensionless == `Value::Real`) are both compatible with `Direction` (inline f64, no `Scalar{DIMENSIONLESS}`); neither argues against it.

What changed since 08-07:
- "Unimplementable" has expired. Branch `task/6090` (22 commits, 24 files, +1183/−29, one merge conflict in crates/reify-expr/src/lib.rs, never built against current main) carries `dir()` with a `DegenerateDirection` diagnostic, Direction conformance promotion, `±` rejection, negation, dot/cross/magnitude via one widening helper (`crates/reify-stdlib/src/direction.rs`).
- The migration is LARGER than the 08-07 text said: 23 direction fields (not 14), ~172 unitless `vec3` direction literals + 63 zero placeholders, ~89 bare-scalar triples, plus embedded `.ri` in `.rs/.md`.
- New evidence Leo did not have: the material-frame k⁴ rescale (fenced, not fixed); `PointLoad`/`Gravity` scale force by |direction| with docs demanding unit input; `Revolute(axis: vec3(0,0,0))` accepted; `Revolute(axis: vec3(1mm,0,0))` → silent undef; a `Direction` slot accepts `"hello"` and launders it into exact-match fn calls and relations; `frame_at` accepts anything statically; five disagreeing zero thresholds; and the uniform-direction question is now load-bearing for V2 (#9568, deferred on it) and the one-verdict relation (#9571).

## 1. What exists today (verified)

| # | Spelling | Where | Unit length | Checked at ctor/default | Checked at fn-call |
|---|---|---|---|---|---|
| a | `Direction` | produced ONLY by `.dir/.normal/.x/.y/.z` projections and the kernel; 6 `let` annotations in 2 examples; ZERO stdlib fields/params | "assumed"; unit by upstream construction (unit3 in projections, gp_Dir in the kernel), unchecked on the Rust side | NOT checked (unexamined family #7958): `A(d: "hello")` passes | exact match only |
| b | `Vector3<Dimensionless>` | 23 stdlib direction fields; stdlib-reference documents ~20 signatures this way; decision 7 `AxisAngle.axis` (#6004, pending) | none; zero vector accepted everywhere | shape + quantity (Length arg rejected, 6159) | exact: `f(vec3(0,1,0))` REJECTED (`Vector3<Int>`), `f(vec3(0.0,1.0,0.0))` accepted |
| c | unitless `vec3` at a `Vector3<Length>` slot | ZERO literal/ctor-keyword sites in the corpus (indirect flows untraced); the idiom ty.rs defends was retired by 5848 | — | silently accepted, stored unitless (5766) | rejected |
| d | three bare scalars | rotate, linear_pattern(_2d), mirror, half_space, revolve, rotate_around, extrude_infinite, arc: ~89 triples; deliberately unslotted | none | none | none |

Also: 18 dimensioned axes in prj (`vec3(1m,0mm,0mm)` to `orient_axis_angle`, which has no declared axis type, normalises, ignores dimension; plus 3 `orient_look_at` sites); `PressureLoad.direction : String = "normal"`; the datum-level joint DSL uses `Axis`; `MassProperties.origin : Frame3` is **dead data** — minted with zero axes from Rust (`dynamics/eval.rs:153 default_frame3`, comment "ZERO magnitude is load-bearing"; `dynamics_ops.rs:211`), pinned by a test, written as 21 zero-axis literals in examples/tests, and read by nothing.

Consumers disagree on what a direction value is (heuristic 10 has no home):
- joints: reject dimensioned (at check, 6159; eval gate second), reject zero at use, NORMALISE at use (stored raw).
- material frame → `rotate_voigt`: raw rows; non-unit axis rescales D_global by k⁴ (fence test elastic_static.rs:12941). Its real invariant is ORTHONORMALITY, which three Directions cannot carry.
- `PointLoad.direction`, `Gravity.direction`, modal transient forcing: raw magnitude scales the load; docs say "callers are expected to supply a unit vector" — intent is unit, scaling is a documented footgun.
- FEA `read_direction_or_neg_z` → −Z, modal `extract_reference_direction` → +Z, modal `read_vec3` → zero, as_printed degenerate → +Z: silent defaults, one of them a pinned "absent ≠ wrong" contract.
- `frame_at`: only `Value::Direction`; a Vector → undef, rc 0, note only.
- relations: `Direction` or `Axis` (lifted); `Vector3<Dimensionless>` is a compile error. Residuals for plane/axis coincidence, offset, on, distance assume unit operands without normalising.
- kernel: every axis is a bare `[f64;3]` in the IR; `gp_Dir` self-normalises; zero thresholds 1e-12 (|v|), 1e-12 (|v|²), 1e-30, 1e-10, 2e-308 across eval/Rust/C++; circular_pattern scalar form, split, moment-of-inertia have no reify-side guard.
- value shape split: kernel mints `Plane.normal`/`Axis.direction` as `Value::Direction`; `.ri` constructors and the solver's `transform_datum` re-mint them as `Vector`; reify-eval `decode_plane/decode_axis` and reify-expr `datum_vec3` REJECT the kernel form.
- `Direction` arithmetic: `d+d`, `-d`, `dot/cross/normalize/magnitude(d)` type-check and eval to undef (rc 0); `normalize(d)` is typed `Vector0<Real>`. Pre-existing holes beyond Direction: `let x : Vector3<Length> = "hello"` passes and does not retype; `vd + vl` (Dimensionless + Length) type-checks.

## 2. Semantics

- A direction is a UNIT, DIMENSIONLESS, SIGNED 3-vector. Dimensionless and unit are orthogonal properties (heuristic 3): mode shapes, Jacobian columns, curl fields are dimensionless and not directions. `Vector3<Dimensionless>` cannot be "the direction type" without conflating two axes of variability — the material-frame rescale is that conflation.
- Signed: `parallel` vs `antiparallel` are distinct in relate_solve; sign-insensitive consumers (`edges_parallel_to`, `perpendicular`) consume a signed type fine. An undirected line is the `Axis` datum. No unsigned direction type is needed.
- Rotation vectors (`orient_exp/log`, `Twist`): |ω| is the angle, dimension Angle — not directions; they stay `Vector3<Angle>`.
- 2D: no 2D direction anywhere in the corpus. Not a reason against a 3D-only type now.
- Kernel: `gp_Dir` ≡ `Direction`. The boundary is already type-agnostic; only eval decoders change.
- GUI: `ri_literal` serialises Bool/Int/Real/String/Scalar only; a `Vector3<Dimensionless>` param is already uneditable, so Direction is parity (follow-up: a `dir(x,y,z)` literal arm).
- **No compelling reason against uniformity was found.**

## 3. Candidates

**A. `Direction` is the single direction type.** Every field/param/arg that MEANS a direction is `Direction`; `Vector3<Dimensionless>` stays for dimensionless non-directions. `dir(x,y,z)` / `dir(v)` is the ONLY Vector→Direction path (normalising by explicit contract, loud `DegenerateDirection` on zero/non-finite). The unit invariant is owned by one newtype-backed `Value::Direction` with a private constructor (an enum variant with public fields cannot be a chokepoint; 32 non-test construction/match sites change), two named constants in reify-core (degeneracy floor; `||v|−1| ≤ tol` asserted loudly at every decode through one shared `decode_direction`, never silently renormalised), and the kernel/C++ floors kept as defence-in-depth until wave 2 retires the bare-scalar IR axes. Arithmetic through ONE widening helper (`as_dimensionless_vector`, as on the branch): dot/cross/scale come for free, `-d` stays Direction, `normalize(d)` identity, `d ± d` rejected (heuristic 11 — not a parallel operator table).
Heuristics served: 1 (name says unit), 3 (dimensionless ≠ unit), 10 (one invariant, one home, construction + decode), 11 (one definition), 12 (a value instead of a triple of meaningful scalars or a `"normal"` string). Loud errors: zero axis, dimensioned axis, string at a Direction slot all become errors.
Against: largest migration of the three; reverses 08-07 and 5848 (acknowledged above); broadens 4382's "projection codomain" framing to "the direction type" (4382's own design doc already calls it "the new first-class type (dimensionless, unit-normalized)").

**B. `Vector3<Dimensionless>` everywhere, delete `Direction` + one shared normalise-or-reject-at-use helper.** Near-zero migration; every ctor keeps accepting `vec3(0,0,1)`. Fails because the unit property has no TYPE home (mode shapes = axes, heuristic 3), magnitude bugs are caught only at runtime, a silently-normalising helper is exactly the silent behaviour Leo is removing, and it undoes a locked design while mismatching `gp_Dir`.

**C. Keep both (the 08-07 ruling; branch exists).** Defensible axis (projection-minted vs authored), lowest effort to green. Fails because provenance is not a type property: the unit invariant lives on the 6-`let` side while the 23 fields and ~235 literals stay magnitude-sensitive, two homes for one concept (11), and the corpus stays split — the opposite of the brief.

**Recommendation: A, implemented with 6090's mechanism** (`dir()`, the widening helper, promotion, rejection arms), not a fresh build.

## 4. Sub-decisions A forces

1. **Language-level implicit widening Direction→Vector3<Dimensionless>.** KEEP (6090): a sound upcast, a justified implicit; it must live in the one ArgVerdict relation (#9571) so fn-call widens too, which makes #8586 REQUIRED (after γ₂ #9363), else the esc-8739-2 fn/ctor disagreement recurs for Direction. DROP: "no coercion either way" is simpler to state; #8586 moot; price = explicit `vector(d)` at every user fn taking `Vector3<Dimensionless>` and at the 6 non-direction DL fields. Lead leans DROP (least surprise, one fewer relation row), presented as a trade.
2. **Frames.** `MaterialFrame` (constitutive.ri:56-61) and `Frame3` (ports.ri:61-66) are 6 of the 23 "direction fields" but their invariant is orthonormality. Options: three `Direction`s (per-axis unit, still possibly non-orthogonal) or a frame/`Orientation`-backed type with its own invariant (`Frame` is already a writable type name; `Frame3` the struct is deliberately not — two homes for "frame", heuristic 11). Producers in Rust: as_printed_material.rs:399-406, _r0.rs:211-216.
3. **`MassProperties.origin : Frame3`** is unread, minted zero from two Rust sites with a pinned test, and spelled as 21 zero-axis literals. Disposition: `Option<Frame3>` / drop / real default. A unit check at construction breaks the minters and the test, not just the literals.
4. **Bare-scalar builtins** (~89 triples, 40+ calls): Direction-taking forms now, or wave 2.
5. **`PressureLoad.direction : String`** → a sum type (`Normal | Fixed(Direction)`), not `Direction` (heuristic 12).
6. **`PointLoad`/`Gravity`**: a unit `Direction` makes them honest directions (behaviour change at ~5 unit-today corpus sites); `force : Real` stays undimensioned — `s * d → Vector3<Q(s)>` makes `100N * dir(0,0,-1)` spellable, which is the natural end state but an FEA-surface question.

## 5. Migration cost (A)

Compiler/type: `dir(x,y,z)` + `dir(v)` (branch has `dir(v)`), DegenerateDirection; Direction conformance promotion (closes #7958's Direction row and the laundering); arithmetic via one helper; `let` annotation checking for non-scalar types (pre-existing hole, independent); Direction row in #9571's one relation; the `Vector3<Int>` literal-typing row (user fns reject `vec3(0,0,1)`) stays for non-direction DL fields.
Value: newtype `Value::Direction` (32 non-test sites / 11 files; ~130 test lines); one decoder; `.ri` Plane/Axis constructors and `transform_datum` mint `Value::Direction`; reify-eval `decode_plane/decode_axis` and reify-expr `datum_vec3` accept it; the four silent-default readers become loud (invert the pinned `extract_loads_malformed_direction_defaults_to_neg_z`); Display prints `dir(x, y, z)` so output round-trips.
Stdlib: 17 direction fields → `Direction` (kinematic 5, ports_mechanical 2, fea_multi_case 2, modal_analysis 5, modal_analysis_fns 2, fdm 1) + 6 frame axes per sub-decision 2; readers → one decoder (#8585 seed); builtin slots faces_by_normal / edges_parallel_to / *_perpendicular_to / orient_axis_angle / orient_look_at / orient_to_axis_angle → Direction; decision 7 `AxisAngle.axis` → Direction (PRD + #6004 text edit, cheap only because unlanded); docs/reify-stdlib-reference.md ~20 signature lines; LSP row.
Corpus: ~172 direction-role unitless literals → `dir(..)` (examples 61, tests 72, crates fixtures 16, prj 8, designs 10, stdlib 4, gui 1); 18 prj dimensioned axes + 3 orient_look_at; 63 zero Frame3 axes per sub-decision 3; embedded `.ri`: `.rs` prismatic(vec3 73 / faces_by_normal( 151 / build_direction 67 / reference_direction 75; `.md` faces_by_normal( 62 / reference_direction 11 / orient_axis_angle( 9 / edges_parallel_to( 7.
Wave 2: bare-scalar builtins → Direction forms; retire scalar forms and the IR `[f64;3]` axes; collapse the kernel floors.
Goes away: 5766/6159 arg-side tolerance (V2), 6090's language-level widening (if DROP), #8586's question, `"normal"` string.

## 6. Live tasks

- **#6090**: live in the dispatch pipeline (pending, plan authored 2026-10-28, architect/opus/xhigh). Recommend Leo DEFERS it now (only status=deferred parks) pending this ruling, then re-scope to A rather than discard the branch.
- **#9568 (V2)**: releasable with target = `Direction`; the measured corpus has no literal Vector-slot hits (indirect flows and embedded `.ri` unverified), so its migration step is near-empty, but any hit that IS a mistyped direction needs `dir()` to exist — sequence after the first migration leaf. printer.ri's `1m` axes are not V2 hits (no slot).
- **#9571**: Direction row joins the one verdict relation; widening lives there if KEEP.
- **#8586**: required if KEEP, moot if DROP — flag, do not edit.
- **#7958, #8585, #8587, #9256, #5351**: absorbed/sequenced by the migration.
- New, with Leo's say-so: frame type; `MassProperties.origin` disposition; value-shape SPOT; silent-default readers; let-annotation hole; Add/Sub vector guard; one threshold pair; `.md`-embedded `.ri`.

---

## Appendix A — acceptance-site verdict matrix (S2, 316 probed cells)

Static arg types (show(String) trick): vec3(0,1,0)=Vector3<Int>; vec3(0.0,1.0,0.0)=Vector3<Real>; vec3(0,1.0,0)=Vector3<Int> (FIRST element wins, math_signatures.rs:614); vec3(0mm,1mm,0mm)=Vector3<Scalar[m]>; normalize(vec3(0,2,0))=Vector3<Real>; axis_y(p).dir=Direction; axis_y(p)=Axis. Vector3<Dimensionless>/<Real>/Vec3<Real>/Vec3<Dimensionless> are ONE type printed Vector3<Real>.
Codes: NMO=NoMatchingOverload (Error); TNCV=TypeNotConformingToVector "argument 'd' has type X but param requires vector type P" (Error); ATMq=ArgTypeMismatch quantity-slot (Error); DPU=DatumProjectionUnavailable (Error). A=accepted silently.

S-fn (user fn, exact structural equality, type_compat.rs:815 resolve_function_overload / overload.rs:563 slot_matches_wildcard_tier; NO Int->Real widening):
  P1 Vector3<Length>: A only for r03/r04/r06 (Length-typed). vec3(0,1,0), vec3(0.0,..), vd, Direction, Axis: NMO.
  P2 Vector3<Dimensionless>: A for vec3(0.0,1.0,0.0), vd, normalize(..); NMO for vec3(0,1,0) (Int≠Real!), vec3(0,1.0,0), vec3(0,0,0), Direction, Axis, mm vectors.
  P3 Direction: A only for a Direction value. P5 Axis: A only for Axis.
S-ctor (conformance/mod.rs:330 check_trait_arg_conformance -> walk_param_against_arg :762; severity Error :75):
  P1 Vector3<Length>: A for ALL vector args incl vec3(0,1,0) -> stored vec(0,1,0) WITHOUT units; vec3(0,0,0) A; Direction/Axis/String/Int -> TNCV.
  P2 Vector3<Dimensionless>: A for unitless/Real/normalize/vd/zero; mm vectors -> ATMq (6159); Direction/Axis/String/Int -> TNCV.
  P3 Direction and P5 Axis: A for EVERYTHING (vec3, mm vec3, "hello", 5, Axis at Direction, Direction at Axis) — unexamined family (mod.rs:2184, owner #7958; allowlist :2211 Bool|Int|String|Scalar only). Eval stores the raw value.
S-default (check_param_default_conformance mod.rs:495 -> same walker): IDENTICAL to S-ctor in all 70 cells (the esc's "secondary disagreement" did not show on these rows).
S-let (entity.rs:1038 check_let_annotation_type returns early unless Int|Scalar :1068): ALL 70 cells A, incl `let x : Vector3<Length> = "hello"`, `let x : Direction = 5`; annotation does NOT retype the cell (let l1 : Direction = vec3(0.0,1.0,0.0) stays Vector3<Real>; takes_dir(l1) NMO).
S-builtin:
  Revolute(axis:) (Vec3<Dimensionless>, kinematic.ri:191): A for unitless/Real/vd/normalize/ZERO (axis vec(0,0,0)!); mm -> ATMq; Direction/Axis/String/Int -> TNCV.
  frame_at(o, X, dir): check A for EVERYTHING (units.rs:779 arity/type-blind); eval: only Direction works (-> frame(...)); every other arg -> undef + "op contract failed (OpContractViolation)" note, rc 0.
  relate { parallel(X, ref) } (relation_signatures.rs:571 lift_to_datum): A for Direction and Axis (lifts); DPU for Vector3<anything>, String, Int.
Ops on dd=axis_y(..).dir: dd+dd, dd-dd, dd+5 (typed Direction!), dd+vd (Direction), vd+dd (Vector3<Real>), vl+dd, vd+vl (Dimensionless+Length typechecks!) -> all static A, eval undef (OpContractViolation note), rc 0. -dd typed Direction, eval undef NO note. dot(dd,dd) Real/undef; cross Vector3<Real>/undef; normalize(dd) typed Vector0<Real>/undef; magnitude undef; dd.x Real -> 0 correct. dd*2, dd/2, dd*dd -> loud ArithOperandKind error. dd==dd -> loud comparison error. dir(vec3(..)) -> WARNING unresolved function, check rc 0, eval undef.

Disagreement table (esc-8739-2 rows confirmed):
  Vector3<Length> <- unitless vec/vd/normalize: fn NMO | ctor A (no units) | default A | let A
  Vector3<Dimensionless> <- vec3(0,1,0)/vec3(0,1.0,0)/zero: fn NMO (Int≠Real) | ctor A | default A | let A | Revolute A
  Vector3<Dimensionless> <- mm vector: fn NMO | ctor ATMq | default ATMq | let A | Revolute ATMq
  Vector3<L|D> <- Direction/Axis/String/Int: fn NMO | ctor TNCV | default TNCV | let A | Revolute TNCV
  Direction <- any Vector3/String/Int/Axis: fn NMO | ctor A | default A | let A | frame_at check A eval undef | parallel DPU (Axis A)
  Axis <- Direction/String/Vector: fn NMO | ctor A | default A | let A
Four acceptance relations: fn exact-strict; ctor/default shape+quantity for Vector, unchecked for Direction/Axis; let unchecked; relate has its own lift.

Direction × Vector3<Dimensionless> interop: no type_compat cross-arm CONFIRMED (type_compat.rs:514,789 passive leaves; implicitly_converts_to :68-194; type_compatible :236). But "mutually unassignable at every site" is REFUTED: Vector->Direction silently accepted at ctor/default (unexamined), both at let. LAUNDERING (t07): A(d: vec3(0.0,0.0,1.0)) or A(d:"hello") makes a.d statically Direction -> passes takes_dir(a.d) by exact match (eval returns vec/"hello"); a.d.x -> undef NO note; relate{parallel(h.d, ref)} passes check and eval silently.
6090's "axis_x/plane_xy fallback-typed Scalar[m]" is STALE: they type Axis/Plane and .dir works.

Surprises: (1) S-let checks nothing for non-scalar annotations and doesn't retype. (2) Direction slots launder any value. (3) Add/Sub has NO operand-kind or dimension guard for vectors (vd+vl typechecks, undef at eval). (4) normalize(Direction) typed Vector0<Real>. (5) first-element literal typing; vec3(1mm,0,0) -> Vector3<Real>, accepted at Dimensionless slots, evals undef, ctor shows A{d: undef} no note (t09). (6) user fn Vector3<Dimensionless> param rejects vec3(0,0,1) while stdlib ctors accept it (fdm.ri:107 default is exactly that). (7) zero vector accepted everywhere incl Revolute. (8) dimensionless vec at Length ctor slot stored unitless. (9) frame_at checks nothing statically. (10) dir() absent -> warning only. (11) BRANCH task/6090 HOLDS 22 COMMITS NOT ON MAIN (merge-base fc0180c880, last 4bbe133354 2026-10-20): direction_widens_to at ctor/default Vector arm, dir() with DegenerateDirection diagnostic, Direction leaf-family promotion, +/- rejection, dot/cross/magnitude widening, normalize identity, Direction negation. Status still pending. Not built/probed.
Unverified: codes mapped from message text; let-annotation retype site not traced; Plane/Frame/Option/List<Direction>/Point rows not probed; relation over laundered value on solve path; --explain-undef/--strict unused; task/6090 branch not built; LSP/GUI not probed.

## Appendix B — consumer inventory (S3)

Prelims: Value::Direction = 3 inline f64, no dim tag; as_f64 accepts Int|Real|Scalar{any} silently dropping dim; IR geometry ops never carry a Direction — every axis/normal is bare [f64;3] (reify-ir geometry.rs:615-715), flattened at eval before the kernel.
31 reader rows. Direction ACCEPTED by only 5: frame_at decode_direction (Direction ONLY; Vector->Undef), stdlib decode_plane (Vector or Direction), feature_datum datum_from_value/vec3_from_value, relate_solve vec3_of/dir_of, as_printed field_vec3. 22 rows reject or n/a.
Normalisation is per-reader: joints unit_axis_from_map normalises at USE (stored axis raw; joint_axis returns raw :676); orient_axis_angle normalises (dim ignored); orient_basis checks orthonormality 1e-6; eval decode_plane/decode_axis unit_vector3 (zero<1e-12 -> Err), but REJECT Value::Direction (pinned test tests.rs:25991 "still fails the pre-delta way"); resolve_vec3_arg (faces_by_normal etc.) Real/dimensionless-Scalar only, Int NOT accepted (vec3(0,0,1) with Int comps would be rejected — untested; corpus uses 0.0), normalised at topology_selectors.rs:477; datum_vec3 (reify-expr) rejects Direction -> .dir on a kernel-minted Axis would be Undef (by reading, not run).
MAGNITUDE-SENSITIVE: (1) rotate_voigt/from_law raw frame, k^4 (fenced test, constitutive.ri:36-49 documents unenforced precondition) — it's a FRAME (rotation matrix), Direction per-axis would fit as 3 Directions or an Orientation. (2) FEA loads elastic_static.rs:4914/:4956: PointLoad.direction, Gravity.direction scale force/gravity by |direction|; fea_multi_case.ri:318-349, :517 DOCUMENT "callers are expected to supply a unit vector; non-unit vectors silently scale" — i.e. intent is UNIT, magnitude-scaling is a documented footgun, not a feature. (3) modal transient forcing read_vec3 (modal_ops.rs:3063) raw coeff = φ·dir. (4) orient_exp: rotation VECTOR, |ω| = angle, dim ANGLE — not a direction (also orient_log, Twist). (5) relate_solve residuals plane_coincidence :769, offset :883, on :961, distance :910, axis_coincidence :753 assume unit operands without enforcing (parallel/antiparallel/perpendicular/angle/tangent DO normalise). (6) kernel make_prism gp_Vec magnitude = extrusion length — internal only, (0,0,dist). (7) joint axes stored raw, observable via joint_axis.
SILENT FALLBACKS given a Direction value: read_direction_or_neg_z -> -Z; extract_reference_direction -> +Z; modal read_vec3 -> zero; as_printed degenerate -> +Z.
Dimension handling inconsistent: required DIMENSIONLESS (helpers.rs:311, resolve_vec3_arg, extract_vec3_si); required ANGLE (orient_exp); ignored (modal read_scalar_si, orient_axis_angle, point3_components); shared-any (stdlib decode_plane).
KERNEL BOUNDARY: eval flattens to [f64;3]; OCCT Rust arms pre-check then C++ gp_Dir(x,y,z) which self-normalises (gp_Dir.hxx:46-54) and raises Standard_ConstructionError only at norm <= gp::Resolution (~2e-308). Per-op zero guards: Rotate/RotateAround/Mirror/Arc/Revolve/ExtrudeInfinite Rust arm mag²<1e-12 (op_args.rs:15 AXIS_MAG_SQ_MIN); Revolve/ExtrudeInfinite also eval (1e-12) and C++ 1e-30; HalfSpace exact-zero only (lib.rs:2170); LinearPattern C++ manual normalise CPP_DIR_MAG_MIN 1e-10; CircularPattern scalar form, Split, MomentOfInertia: NO guard, gp_Dir only. Thresholds: 1e-12 (mag), 1e-12 (mag²), 1e-30 (mag²), 1e-10 (mag), 2e-308 — none agree. Error form: wrap_occt_call -> "OCCT <op>: <msg>" -> GeometryError::OperationFailed. Uniform-type implication: kernel boundary is already type-agnostic; only eval decoders change.
GUI: no special handling; Value::format "direction(x, y, z)" vs "vec(...)"; ri_literal.rs:788 rejects Direction param as UnsupportedValueKind (GUI param editing of a Direction impossible).
Other kernels (manifold, fidget, openvdb, gmsh): no direction-bearing op arms.
Surprises: kernel-minted Axis/Plane (Direction inside) rejected by eval decode_axis/decode_plane AND by datum_vec3; four silent fallbacks; dimension handling inconsistent; nothing in the language builds a Direction; residual correctness depends on producers' unit-ness.
Unverified: eval_named_arg_f64 coercions; compile-time Direction at Vector3<DL> param (S2 answered: TNCV); OCCT strings; Surprise 1/2 end-to-end; mesh-morph/fdm/gcode/sweep consumers of build_direction/SweepParams; flexure/RollerSupport downstream; Tauri JSON.

## Appendix C — corpus census (S4)

Partitions: stdlib 47 | examples 266 | prj 2 | designs 10 | tests(prd-gate) 308 | crates_fx 189 | docs 54 | gui 14 | tsreify 27.

SUMMARY (non-comment sites)
cat                                         stdlib examples prj designs tests crates_fx docs gui tsreify
A  `: Direction` decl (all `let`)              0       6     0     0      0      0       0   0    0
B  Vec3/Vector3<Dimensionless|Real> decl      29*     2     0     0      2      0       0   0    0   (*23 direction, 6 non-direction)
C  Length-typed vec that is really a dir       0       0     0     0      0      0       0   0    0   (legit Length vecs: stdlib 4, tests 5)
D1 String-typed direction (PressureLoad)       1       1     0     0      1      0       0   0    0
D2 Axis-typed params/lets                      8       14    0     0      5      9       6   0    2
D3 three-scalar direction triples (calls)      0      ~39    8    27      5      9       1   0    0
E  unitless vec3 literal, direction role       4      61     8    10     72     16       0   1    0   (+63 zero-vector Frame3 placeholders: ex 33, tests 30)
   (E into a Vec3<Length> slot)                0       0     0     0      0      0       0   0    0
F  dimensioned vec3 literal as direction       0       0    22     0      2      0       0   0    0
G  Vec2 direction-like                         0       0     0     0      0      1       0   0    0   (smoke value, not a direction)
H  datum projections used as directions        0      15     0     0      7      2       0   0    0
I  normalize( calls                            0       4     0     0      0      0       0   0    0   (hand-rolled unit division: prj 5 lines)

A: 6 `let x : Direction = <projection>` in examples/datum_projections.ri:21,25,30,31,32 and examples/geometric_relations/feature_datum_axis.ri:33. NO `param : Direction` anywhere. No .ri constructs a Direction from vec3. The other "Direction" substring hits are enums (ports.ri:34 Directionality, ports_mechanical.ri:42 ThreadTighteningDirection, tolerancing.ri:28 SurfaceDirection) or prose.
B stdlib direction (23): constitutive.ri:58-60 MaterialFrame axes; fdm.ri:107 build_direction; fea_multi_case.ri:349 PointLoad.direction, :529 Gravity.direction; kinematic.ri:169,191,204,213,214; modal_analysis.ri:526 reference_direction, 637, 704, 773, 843 (force directions); modal_analysis_fns.ri:168,209; ports.ri:63-65 Frame3 axes; ports_mechanical.ri:140,154 port axes. Non-direction dimensionless (6): kinematic.ri:323/324 JacobianColumn; modal_analysis.ri:338 mode shape; solver_buckling.ri:191, solver_buckling_fns.ri:112, solver_elastic.ri:546 curl. Only alias: trajectory.ri:120 `pub type Vec3<Q: Dimension> = Vector3<Q>`.
C: NONE LEFT (5848 migrated all). Remaining Length vectors are positions/displacements: ports.ri:62 Frame3.origin, solver_elastic.ri:505, tensegrity.ri:445, trajectory.ri:366.
D1: fea_multi_case.ri:427 `param direction : String = "normal"` (PressureLoad); users fea_pressure_smoke.ri:36, shell_route_pressure_thin_plate.ri:13. extrude_infinite trailing "positive" string selects sign. `List<Real>` direction: GONE (task 5905 retyped PointLoad/Gravity).
D4 Axis datum as direction: joints.ri:59,80,95 (revolute/prismatic/cylindrical datum-level joint DSL takes Axis; "Axis lifts to Direction via .dir"). examples/kinematic/relate_mounted_revolute.ri:51,86 note revolute(axis,…) expects dimensionless Vector3, NOT an Axis.
D5 Three bare scalars (THE LARGEST POPULATION): rotate (5-arg, 29 triples), linear_pattern_2d (16 calls, 32 triples), linear_pattern, mirror(7-arg nx,ny,nz), circular_pattern(9), half_space(px..nz), revolve/revolve_full (axis triple deliberately UNSLOTTED bare scalars, builtin_signatures.rs:1038-1058), rotate_around, extrude_infinite(dx,dy,dz + "positive" string), arc. ≈82 triples across 40+ calls. builtin_signatures.rs:625-651 says normal/axis deliberately unslotted so it doesn't get a Length slot.
D7: orient_look_at(forward, up) takes Length-dimensioned vec3s in prj (printer.ri:1395-1397).

E per-callee (call sites; declared slot):
 1 prismatic arg0: 51 (ex10 prj5 tests35 gui1) — Prismatic.axis Vec3<Dimensionless>; eval gate validate_dimensionless_unit_axis_vec3 (reify-stdlib helpers.rs:311) -> Undef if DIMENSIONED. 42 unitless lits, 9 idents (all unitless).
 2 faces_by_normal arg1: 31 — "dir Vec3 (unchecked)" builtin_signatures.rs:470-471; 1 dimensioned lit (faces_by_normal_symbolic_eval_silent.ri:31).
 3 rotate scalars: 30.  4 orient_axis_angle arg0: 28 (ex3 prj18 designs3 tests2 cfx2) — NO declared param type; result Orientation(3); runtime tensor_components_f64 ignores quantity, normalizes, zero->Undef (orientation.rs:601-626). 9 unitless, 17 dimensioned, 2 mixed.
 5 edges_parallel_to: 25.  6 revolute arg0: 18 — Revolute.axis Vec3<Dimensionless>, same gate.  7 linear_pattern_2d: 16.  8 Frame3 named axes: 23 (21 all-zero).  9 ModalOptions.reference_direction: 16, all unitless.  10/11 prb_* flexure arg5: 13 untyped.  12 linear_pattern 5. 13 circular_pattern 5. 14 mirror 8. 15 frame_at args 1,2: 6 — the ONE builtin with Direction-typed slots (units.rs:691 doc form); all args are projections.
 others: PointLoad.direction 3, Gravity 2, StepForce 2, half_space 3, orient_look_at 3, revolve 2, revolve_full 2, planar 2, faces_perpendicular_to 2, edges_perpendicular_to 2, rotate_around 1, extrude_infinite 2.
 vec3 literal fed DIRECTLY into a Vec3<Length>/Vector3<Length> slot: 0. (Only Length vec slots: Frame3.origin, transform3 translation.)
 prj E (8): printer.ri:2088,2092,2469 reference_direction; :2942,:2996,:3279,:3281,:3283 prismatic.
Signatures: extrude/extrude_symmetric have NO direction arg (distance Length). revolve(profile, ox,oy,oz, ax,ay,az, angle) origin Length-slotted, axis unslotted. mirror 7-arg or 2-arg Plane. half_space 6 scalars. plane_through(a,b,c)/axis_through(a,b) take POINTS only. frame_at(o: Point, x: Direction, z: Direction). axis_x/y/z(o), plane_xy/xz/yz(δ) are functions.

F dimensioned-direction literals: printer.ri 14 literal (467,468,501,502,626,722,827,863,953,954,1009,1010,1799,1800) + 2 mixed (1388,1389) to orient_axis_angle; dev_capstan.ri:320,321 (1mm convention); adt_relation_verbs.ri:13 (vec3(0mm,0mm,1m)); orient_look_at up: printer.ri:1395-1397; unused lets printer.ri:1936-1938 (comment :1935 "unit-magnitude convention: 1 m"); faces_by_normal_symbolic_eval_silent.ri:31. Convention NOT uniform (1m vs 1mm). Joint ctors REJECT dimensioned axes at eval; none of the prismatic/revolute sites carry one.
G 2D: zero. Sketches: point/line/arc; no spelled directions.
H: .dir/.normal 3 lines (examples); Frame/Direction components 21 lines (datum_projections.ri, first_class_frame_datums.ri, s8_frame_datum_planes.ri, datum_units_eta_*.ri). axis_x/plane_xy calls: ~20, cfx-heavy. stdlib/prj use none.
I: normalize( 4 calls in examples only (dimensional_consistency.ri:46, math_linalg.ri:43,49). Hand-rolled sqrt/divide printer.ri:1347-1349, 1364-1367.

Judgment: R3 name heuristic 45 sites; idents to prismatic/revolute/faces_by_normal/edges_parallel_to traced 100% to unitless lets. R4 ambiguous 30 excluded (basis vectors in linalg demos, dot operands, restraint lists, twist vectors); add ~13 if angle() operands and restraint lists count.

Surprises:
- Direction and Vector3<Dimensionless> are DISJOINT worlds in the corpus, joined only via frame_at(…, Direction, Direction).
- 21 Frame3 literals pass x/y/z_axis = vec3(0,0,0) (63 zero literals) as MassProperties placeholders: closed_2prismatic_idyn.ri:62-64, closed_4bar_idyn.ri:133-138, pendulum_idyn.ri:29, toolhead_motor_sizing.ri:81 (11 frames); tests s6_idyn_*.ri (10 frames). A unit-length invariant would break these.
- Three-bare-scalar spelling (~82 triples) is the biggest direction population, by design.
- orient_axis_angle has NO declared axis type; prj feeds it dimensioned axes (18) while joint ctors enforce dimensionless at eval: same logical arg, two incompatible conventions.
- Embedded .ri in .rs/.ts/.md unscanned: `prismatic(vec3` 73 in .rs (gui engine_tests.rs ~25), `faces_by_normal(` 151, `reference_direction:` 14, `build_direction` 67.
- fdm.ri:168 second literal spelling inside stdlib (MaterialPropertyProvenance).
Unverified: Direction param accepting vec3 default; faces_by_normal list literal; .md-embedded .ri; 16 cited lines spot-checked OK.

## Appendix D — skeptic verdicts (S6)

C1 CONFIRMED (6 lets, 2 example files; 0 stdlib). C2 CONFIRMED (dir()/direction() unresolved warning, eval undef). C3 CONFIRMED. C4 CONFIRMED (laundering re-probed incl. param default). C5 CONFIRMED. C6 CONFIRMED.
C7 PARTIAL: zero unitless literals into Length slots CONFIRMED (indirect flow via variables not traced); "bare scalars are the biggest population" REFUTED: ~172 unitless vec3 direction literals (+63 zero) vs ~89 scalar triples — vec3 outnumber ~2:1; dominated by prismatic/revolute (69), faces_by_normal 31, orient_axis_angle 28, edges_parallel_to 25.
C8 CONFIRMED (21 Frame3 literals, 7 files, all MassProperties(origin: Frame3(...))).
C9 PARTIAL: orient_axis_angle normalises and ignores dimension (vec3(1m,0mm,0mm) == vec3(1,0,0) result); 18 prj sites. CORRECTION: a dimensioned LITERAL axis at a joint ctor is rejected at CHECK (ATMq), the eval gate is a second line that bites only for what passes typing: `Revolute(axis: vec3(1mm, 0, 0))` (first-element typing -> Vector3<Real>) passes check and evals axis: undef with NO note. The eval gate also rejects Value::Direction.
C10 CONFIRMED contents (22 commits, 24 files +1183/-29; direction_widens_to one-way; add_sub_direction_reject; conformance Vector arm admits Direction + Direction in leaf allowlist; "dir" in DATUM_CONSTRUCTOR_NAMES; direction.rs 211 lines; linalg widening; negation). Merge NOT clean: `git merge-tree --write-tree main task/6090` -> ONE conflict in crates/reify-expr/src/lib.rs (main moved 1217 files since base). Task still pending. 6090 = the dual-spelling design; a single-type ruling must explicitly supersede/re-scope it.
C11 CONFIRMED: rotate_voigt raw (fence test 12941); PointLoad force*dir (:4965), Gravity density*magnitude*dir (:4979) raw; modal reference_direction, as_printed build_direction, joints, orient_axis_angle, relate_solve normalise. Both FEA readers silently ignore Value::Direction.
C12 CONFIRMED. C13 PARTIAL: 57 lines/20 files non-test reproduced; 137/24 if inline #[cfg(test)] modules counted; 186/37 everything — 6090's "~120/21" ≈ inline-test-inclusive at an older base.
Additional: (1) value-shape split kernel vs .ri (not re-verified); (2) Direction is signed; -d undef no note (6090 adds negation); (4) a unit check AT CTOR TIME breaks the 21 zero Frame3 placeholders and Revolute(axis: vec3(0,0,0)); reject-at-use would not; (6) making PointLoad.direction a true unit Direction is a behaviour change at ~5 corpus sites (3 PointLoad + 2 Gravity); (7) Revolute(axis: vec3(1mm,0,0)) -> silent undef; (8) frame_at accepts anything at check.
Unverified: branch build/tests; indirect flows; .ri in .rs/.ts/.md; Prismatic eval rejection.

## Appendix E — builtin signature table for the θ/ι migration (S9, main `f2c2220302`, 2026-10-30)

# Direction migration: definitive signature table
HEAD = f2c2220302bacfa42f2ffb02a765c61b0d174c96 (main). Read-only. Nothing was built or run, so every behaviour claim comes from reading code. Prior census: /tmp/dir-study/seats/S4-corpus.md and S3-consumers.md. My counting script is /tmp/dir-study2/count.py.

Abbreviations:
- Compile-side lowering is in crates/reify-compiler/src/geometry.rs (G), geometry_transform.rs (GT) and geometry_curve.rs (GC).
- Eval side is crates/reify-eval/src/geometry_ops.rs (GO).
- Stdlib eval is crates/reify-stdlib/src/{geometry,orientation,joints,helpers}.rs.
- Slot table is `builtin_arg_slots(name, arg_count)` at builtin_signatures.rs:~455.
- "unslotted" means the compiler's arg-type table has no row for the arg, so nothing checks it at compile time.
- "gate" means `eval_named_arg_f64`: it accepts Real, Int, or Scalar of any dimension, and silently drops the dimension.
- Corpus counts are `.ri` call sites over all 917 tracked `.ri` files. Comments are stripped and arity is counted with a paren-balanced scan. They are written `n calls / m files`.
- Embedded counts are raw lines matching `(?<![\w.:])NAME\s*\(` in `*.rs` and `*.md`. They are upper bounds, because Rust helper calls and prose are included.

## A. Geometry builtins taking three bare scalars (the D5 population)

| name / arity | current signature (positional) | direction args: where / how typed | eval arm and magnitude handling | corpus `.ri` / embedded rs, md | PROPOSED | notes |
|---|---|---|---|---|---|---|
| rotate, 5-arg | `rotate(target, ax, ay, az, angle)`. Slot table: only `angle_arg(4)` (`"rotate" if arg_count==5`). | args 1-3. Unslotted bare scalars. | `transform_rotate` (GO:~3722). It reads `ax`/`ay`/`az` through the gate and puts the raw triple in `GeometryOp::Rotate{axis}`. No normalising or zero check in eval. The kernel Rust arm rejects mag² < 1e-12 and C++ `gp_Dir` normalises. | 29 calls / 11 files. rs 112, md 39 (raw upper bounds, includes Rust `rotate(`). | `rotate(target: Geometry, axis: Direction, angle: Angle)`, arity 3. | It must be a new arity-3 arm. The existing 2-arg Orientation form never carried a direction. |
| rotate, 2-arg | `rotate(target, orientation)` | none (Orientation). | Same `transform_rotate`. It is dispatched on the presence of an `orientation` named arg. `decode_orientation_to_axis_angle` turns it into axis-angle. | 1 call / 1 file. | unchanged. | Not a Direction arg. Only the 3-arg form is added. |
| rotate_around, 8-arg | `rotate_around(target, px,py,pz, ax,ay,az, angle)`. Slots: px,py,pz LENGTH at 1-3; `angle_arg(7)`. | args 4-6, unslotted bare. | `transform_rotate_around` (GO:~3790). Gate read, raw triple, no eval normalise. Kernel guard mag² < 1e-12. | 1 call / 1 file. rs 20, md 3. | `rotate_around(target, pivot: Point3<Length>, axis: Direction, angle: Angle)`, or `(target, axis: Axis, angle)`. | Pivot stays Point3<Length>. The existing px,py,pz LENGTH slots disappear from the new arity. The stdlib reference (reify-stdlib-reference.md:745) already documents the Vector3 form as "not implemented". |
| mirror, 7-arg | `mirror(target, ox,oy,oz, nx,ny,nz)`. Slots (arity-guarded): ox,oy,oz LENGTH at 1-3 only. | args 4-6, unslotted bare. | `pattern_mirror` scalar branch (GO:~4262). Origin goes through the `required_length_origin3` gate. Normal is raw `f64_arg`, with no eval normalise. Kernel mag² guard 1e-12. | 3 calls / 3 files. rs 73, md 57. | `mirror(target, origin: Point3<Length>, normal: Direction)`, arity 3. | The 2-arg `mirror(target, Plane)` already exists. Its decoder `decode_plane` calls `unit_vector3` (mag < 1e-12 is an error) but rejects `Value::Direction`. |
| mirror, 2-arg (value form) | `mirror(target, plane)` | The Plane's normal is a `Value::Vector` (stdlib-minted) or a Direction (kernel-minted). | `decode_plane` (GO:2088). Origin is LENGTH-gated. Normal goes through `point3_components`, then `unit_vector3`, which errors on mag < 1e-12. | 5 calls / 5 files. | unchanged, but `decode_plane` must accept `Value::Direction` in the normal. | The pinned test tests.rs:~25991 ("still fails the pre-delta way") must flip. |
| half_space | `half_space(px,py,pz, nx,ny,nz)`. Slots: px,py,pz LENGTH at 0-2. Arity is exactly 6 (`check_arg_count_exact` at G:1945). | args 3-5, unslotted bare. | `prim_half_space` (GO:3061). The point is group-gated LENGTH. The normal is raw `eval_arg`, stored as a `Value` in `GeometryOp::HalfSpace`. The kernel checks only `norm_sq == 0.0` or non-finite (lib.rs:~2170). | 3 calls / 3 files (`examples/half_space.ri`, a prd-gate fixture, a compiler test fixture). rs 62, md 25. | `half_space(origin: Point3<Length>, normal: Direction)`, arity 2. | The reference documents `half_space(plane: Plane)` (line 522), arity 1. LSP has a row with a `DIMENSIONLESS_NORMAL_NOTE` plus a `LENGTH_GATED_EXAMPLES` row, both behaviourally tested. |
| revolve | `revolve(profile, ox,oy,oz, ax,ay,az, angle)`. Arity is exactly 8. Slots: ox,oy,oz LENGTH at 1-3; `angle_arg(7)`. | args 4-6, unslotted. | `sweep_revolve` (GO:~4575). The origin is gated. The axis is read as `[f64;3]`. Eval rejects mag < GEOMETRY_EPSILON (1e-12) with a **Warning** and drops the op. The raw (unnormalised) axis goes to the IR. | 2 calls / 2 files. rs 25, md 10. | `revolve(profile, origin: Point3<Length>, axis: Direction, angle: Angle)`, arity 4, or `(profile, axis: Axis, angle)`, arity 3. | The builtin_signatures HAZARD block, with the `revolve(profile, axis_value, angle)` worked example, names exactly this change. The slots at 1,2 would fire on the wrong args without an arity guard. |
| revolve_full | `revolve_full(profile, ox,oy,oz, ax,ay,az)`. Arity is exactly 7. It injects τ as a literal angle. | args 4-6. | The same `Sweep{Revolve}` arm as `revolve`. | 2 calls / 2 files. rs 12, md 10. | `revolve_full(profile, origin, axis: Direction)`, or `(profile, axis: Axis)`. | none. |
| linear_pattern, 6-arg | `linear_pattern(target, dx,dy,dz, count, spacing)`. Slot `spacing` LENGTH at 5 (guarded `arg_count==6`). | args 1-3, unslotted. | `pattern_linear` (GO:~4078). Gate read, raw. The C++ side normalises (`CPP_DIR_MAG_MIN` 1e-10). | 5 calls / 3 files. rs 30, md 6. | `linear_pattern(target, direction: Direction, count: Int, spacing: Length)`, arity 4. | **No 4-arg value form exists at HEAD**, despite the brief. G:1965 enforces exactly 6, and `pattern_linear` has no value-form branch. Only a comment promises it ("task 5351 lands…") and the reference documents it at line 763 as spec. The 4-arg `spacing` slot is at index 3. |
| linear_pattern_2d | `linear_pattern_2d(target, dx1,dy1,dz1, count1, spacing1, dx2,dy2,dz2, count2, spacing2)`. Arity is exactly 11. Slots (guarded `==11`): spacing at 5 and 10. | args 1-3 and 6-8. | `pattern_linear2d` (GO:~4303), the same gate and raw handling for both directions. | 16 calls / 7 files. rs 27, md 5. | `linear_pattern_2d(target, dir1: Direction, count1, spacing1, dir2: Direction, count2, spacing2)`, arity 7. | In the 7-arg form index 5 is `count2` (Int). The existing arity-11 guard exists to prevent a false LENGTH mismatch there. |
| circular_pattern, 9-arg | `circular_pattern(target, ox,oy,oz, ax,ay,az, count, angle)`. Slots (guarded `==9`): ox,oy,oz LENGTH at 1-3; `angle_arg(8)`. | args 4-6, unslotted. | `pattern_circular` scalar branch (GO:~4215). Origin gated, axis raw. The kernel has no zero guard here (`gp_Dir` only). | 3 calls / 3 files. rs 48, md 19. | Remove, or keep, with the Axis form as primary. A new form `circular_pattern(target, origin, axis: Direction, count, angle)` is optional. | The 4-arg `circular_pattern(target, axis: Axis, count, angle)` already exists. |
| circular_pattern, 4-arg (value form) | `circular_pattern(target, axis, count, angle)`. Slot `angle_arg(3)` (guarded `==4`). | `axis: Axis` value. | `decode_axis` (GO:2168). Origin LENGTH-gated; the direction goes through `unit_vector3` (error if mag < 1e-12). It rejects a Direction inside the Axis. | 2 calls / 2 files. | unchanged, but `decode_axis` must accept a Direction in `Axis.direction`. | A kernel-minted Axis (Direction inside) is rejected here today. |
| extrude_infinite | `extrude_infinite(profile, dx,dy,dz, direction: String)`. Arity is exactly 5 (G:2258). No slots at all. | args 1-3 are bare scalars. Arg 4 is a **sign-selecting String**. | `sweep_extrude_infinite` (GO:4751). It rejects mag < 1e-12 (Warning, drops the op). The string folds into `(axis, both)`: "positive" keeps the axis, "negative" negates it, "both" keeps it with `both=true`, anything else is an Error. | 2 calls / 2 files. rs 18, md 9. | `extrude_infinite(profile, direction: Direction, sense: String = "positive")`. See notes. | Sign folds into the Direction (`dir(0,0,-1)`), but **"both" has no Direction analogue**. Keep a 3-arg form `(profile, dir, "positive"/"negative"/"both")`, or add a separate `extrude_infinite_symmetric`. The reference documents `extrude_infinite(profile, axis: Vector3<Dimensionless>, direction: String)` (line 531). |
| arc | `arc(cx,cy,cz, radius, start_angle, end_angle, ax,ay,az)`. Arity is exactly 9. Slots: `angle_arg(4)` and `angle_arg(5)` only. | args 6-8, unslotted. | `curve_arc` (GO:~5010). cx,cy,cz,radius are group-gated LENGTH. The axis is raw `f64_arg`. Kernel mag² guard 1e-12. | arc/9: 1 call / 1 file. A different `arc` has arity 3, a 2D sketch arc; the arity split already separates them. rs 27, md 11. | `arc(center: Point3<Length>, radius: Length, start: Angle, end: Angle, normal: Direction)`, arity 5. | The reference (lines 601-603) documents a 4-arg form with no axis. The axis here is a plane normal, not a travel direction. |
| offset_curve, 3-arg direction overload | `offset_curve(curve, distance, third)`. `third` is a "neutral" arg. Slot: `length_arg(1,"distance")`. | `third`: **either** a reference Surface handle **or** a direction vec. | `modify_offset_curve` (GO:~3665). It tries geometry-handle resolution first, then `point3_components`, then `unit_vector3` (normalises, zero is an Error). | 1 call / 1 file (2-arg only). rs 32, md 13. | same arity; `third: Surface \| Direction`. | Disambiguated by `Value` variant at eval, not by arity. `point3_components` rejects Direction, so the Direction arm must be added. The reference documents `direction: Vector3<Dimensionless>` (line 699). |

## B. Selectors (same arity; widen or replace in place)

| name | current signature | direction arg | eval arm | corpus / embedded | PROPOSED | notes |
|---|---|---|---|---|---|---|
| faces_by_normal | `faces_by_normal(solid, dir, tol)`. Slot `angle_arg(2,"tol")` only. | arg 1, "Vec3 unchecked" (documented as `dir: Vec3` in units.rs:184). | `try_eval_topology_selector` → `resolve_vec3_arg` (GO:10354). Accepts only a `Value::Vector` of 3 Real or dimensionless-Scalar components. Int comps, Direction and dimensioned comps are rejected with one Warning (not an Error). Normalised in `topology_selectors.rs:477-491`; zero or non-finite is an Error. | 30 calls / 18 files, plus 1 arity-2 call (a malformed-call fixture). rs 135, md 62. | `faces_by_normal(solid, dir: Direction, tol: Angle)`. | `resolve_vec3_arg` is the single chokepoint for 4 names and also serves `angle(a,b)` at GO:8897. |
| edges_parallel_to | `edges_parallel_to(solid, dir, tol)`, same slot. | arg 1. | Same helper (GO:7218). Sign-agnostic in `topology_selectors.rs:842`. | 25 calls / 11 files. rs 18, md 7. | `(solid, dir: Direction, tol: Angle)` | Sign is irrelevant. |
| faces_perpendicular_to | `faces_perpendicular_to(solid, dir, tol)`. In the code the arg is named `axis`. | arg 1. | Same helper (GO:~7250). `selector_vocabulary_v2.rs:241` rejects zero or non-finite. | 2 calls / 2 files. rs 11, md 1. | as above. | Sign-agnostic. |
| edges_perpendicular_to | same shape. | arg 1. | GO:~7264. | 2 calls / 2 files. rs 8, md 1. | as above. | Sign-agnostic. |

## C. Orientation constructors and decomposers

| name | current | direction args | eval | corpus / embedded | PROPOSED | notes |
|---|---|---|---|---|---|---|
| orient_axis_angle | `orient_axis_angle(axis, angle)`. **No slot-table row.** LSP shows `axis: Vector`; the reference shows `axis: Vector3<Dimensionless>`. | arg 0. | `orientation.rs:601`. `tensor_components_f64` accepts Tensor, Point or Vector only, so **`Value::Direction` gives Undef**. It ignores the dimension, normalises, and returns Undef for zero or non-finite. | 28 calls / 11 files (17 dimensioned, 9 unitless, 2 mixed per S4). rs 34, md 9. | `orient_axis_angle(axis: Direction, angle: Angle)` | The 17 dimensioned callers (printer.ri) stop compiling and need a one-shot rewrite. |
| orient_look_at | `orient_look_at(forward, up)` | args 0 and 1. | `orientation.rs:179`. Gram-Schmidt: normalises forward, cross(up, z) gives x, errors on parallel or zero. Same Direction-rejecting reader. | 3 calls / 1 file (printer.ri 1395-1397, Length-dimensioned). rs 2, md 8. | `(forward: Direction, up: Direction)` | Both args are directions and the magnitude is ignored. |
| orient_basis | `orient_basis(x_axis, y_axis, z_axis)` | args 0-2. | `orientation.rs:106`. Requires |v| = 1 within 1e-6, orthogonality within 1e-6 and det = +1, else Undef. | 0 `.ri` calls. rs 2, md 3 (frame_at calls it internally). | `(x: Direction, y: Direction, z: Direction)`. | The only consumer that **enforces** unit magnitude. A Direction guarantees it. |
| orient_to_axis_angle | `orient_to_axis_angle(q: Orientation<3>)`. The compiler types it `StructureRef("AxisAngle")`, which is deliberately declared nowhere. | **Output** `axis`. | `orientation.rs:465`. Returns `Value::Map{angle, axis: Vector[Real;3]}`. Identity gives axis [1,0,0]. | 2 calls / 2 files. rs 16, md 7. | return `AxisAngle{axis: Direction, angle: Angle}`, per #6004. | The `axis` field becomes a Direction. The Map to nominal-struct change is #6004's. Declaring `AxisAngle` is its post-state. |
| orient_exp / orient_log | rotation vector, |ω| = angle, ANGLE dimension. | n/a. | n/a. | n/a. | **exclude**. | Not a direction. |

## D. Datum, frame and plane constructors (already partly Direction)

| name | current | direction args | eval | corpus | PROPOSED | notes |
|---|---|---|---|---|---|---|
| frame_at | `frame_at(o: Point, x: Direction, z: Direction) -> Frame(3)` (units.rs:691). Not in the slot table. | args 1 and 2: **Direction only**. | `eval_frame_at` (stdlib/geometry.rs:1901). `decode_direction` accepts a Direction **only**, so a Vector gives Undef. It normalises z, derives y = norm(z×x) and x = y×z (Gram-Schmidt). It errors if parallel or zero. | 6 calls / 4 files, all projections. rs 13, md 1. | unchanged. | The only existing Direction-typed builtin. It is the template. |
| axis_through | `axis_through(a: Point, b: Point) -> Axis` | none (points). | `geometry.rs:1806`. Produces direction = normalised(b−a) as `Value::Vector<Real>`, **not** a Direction. | 2 calls / 2 files. | args unchanged; ensure the minted `Axis.direction` is a Direction. | Excluded as an argument site. The producer matters for `.dir`. |
| plane_through | `plane_through(a,b,c: Point) -> Plane` | none. | `geometry.rs:1834`. Normal = normalised cross, minted as a Vector. | 2 calls / 2 files. | args unchanged; the minted normal becomes a Direction. | Same producer note. |
| axis_x / axis_y / axis_z | `axis_x(o: Point3<Length>) -> Axis` | none. | `make_axis` mints a `Vector[Real]` direction. | axis_x 2 calls and axis_z 2 calls (axis_y 0). | args unchanged; direction becomes a Direction. | LSP rows for these are wrong; see Surprises. |
| plane_xy / plane_xz / plane_yz | `plane_xy(δ: Length) -> Plane` | none. | `make_plane` mints the normal as a Vector. | 17 calls / 8 files in total. | args unchanged. | Same producer note. |
| offset (plane, arity 2) | `offset(p: Plane, δ: Length) -> Plane`. The arity-3 form is a relation. | none. | `eval_offset_plane`. Reads the normal via `decompose_vec3` then `decode_direction`, so **Direction or Vector are both accepted** here. Normalises it. | arity 2: 3 calls / 3 files. | unchanged. | Dispatched by arity: `"offset" if args.len()==2` in the compiler. |
| midplane | `midplane(a: Plane, b: Plane) -> Plane` | none. | Uses `decode_plane` in stdlib/geometry.rs, which accepts a Vector **or** a Direction. | 2 calls / 2 files. | unchanged. | |
| draft | `draft(target, [faces,] angle, plane)`. Arity 3 or 4. | The Plane's normal is the pull direction. | no separate direction arg. | 2 calls / 2 files. | unchanged. | Mention only. |

## E. Joint and flexure constructors (stdlib eval, same arity)

| name | current | direction arg | eval | corpus | PROPOSED | notes |
|---|---|---|---|---|---|---|
| prismatic | `prismatic(axis, range[, pivot])`. Documented as `axis: Vector3<Dimensionless>`. | arg 0, `Prismatic.axis`. | `joints.rs:16` → `validate_axis` → `validate_dimensionless_unit_axis_vec3` (helpers.rs:311). Needs a Tensor, Point or Vector, so **a Direction gives Undef**. Needs DIMENSIONLESS, finite and non-zero, and returns the **raw** triple. The joint stores it raw; `unit_axis_from_map` normalises at use. | 46 calls with 2 args, 5 with 3 and 1 with 4 (52 total, about 25 files). rs 121, md 20. | `prismatic(axis: Direction, range[, pivot])` | The corpus passes `Y_HAT`-style unit lets. It is the largest Vector3 population. |
| revolute | `revolute(axis, range[, pivot])` | arg 0. | `joints.rs:41`, same validator. | 21 calls (8 with 2 args, 11 with 3, 2 with 4). rs 56, md 27. | `revolute(axis: Direction, range[, pivot])` | The reference says `axis: Axis` (line 2124) while the runtime is a Vector. |
| cylindrical | `cylindrical(axis, lin_range, ang_range)` | arg 0. | `joints.rs:150`. | 3 calls / 3 files. | `axis: Direction` | |
| planar (joint) | `planar(axis_x, axis_y, range_x, range_y, range_rot)` | args 0 and 1. | `joints.rs:75`. Same validator, plus a perpendicularity check (`|dot| < 1e-9`). | 2 calls / 2 files. A separate 2-arg `planar` also exists. | `axis_x: Direction, axis_y: Direction` | Direction-typing does not remove the orthogonality check. |
| prb_cantilever_beam, prb_fixed_fixed_beam | `(length,width,thickness,material,pivot,axis[,neutral[,declared_range]])` | **arg 5**. | `flexures/beam.rs:82` → the same validator; stores the axis raw in the joint Map. Rejects a Direction. | 6 calls (5 with 6 args, 1 with 8) / 5 files. | `axis: Direction` at arg 5. | |
| prb_notch_circular, prb_notch_elliptical, prb_notch_right_circular | `(notch_radius,web_thickness,width,material,pivot,axis[,…])` | **arg 5** | `notch.rs:128`. | prb_notch_circular 2 calls / 2 files. | same. | |
| prb_living_hinge, prb_cross_spring_pivot | `(length,width,thickness,material,pivot,axis[,…])` | **arg 5** | `hinge.rs:91`. | 0 calls found. | same. | |
| prb_prismatic_blade, prb_two_axis_pivot | `(length,width,thickness,material,pivot,axis[,neutral])` | **arg 5** | `prismatic.rs:103`. | 0 calls found. | same. | |
| prb_parallelogram_flexure, prb_double_parallelogram_flexure | `(length,width,thickness,blade_spacing,material,motion_axis,pivot[,declared_range])` | **arg 5** (`motion_axis`; the pivot is arg 6). | `compound.rs:94`. | 8 calls / 8 files (7 and 1). | `motion_axis: Direction` | Arg order differs: the axis sits before the pivot. |
| prb_let_joint | `(length,width,thickness,n_blades,material,pivot,axis[,…])` | **arg 6** | `hinge.rs:244`. | 0 calls found. | `axis: Direction` | The index differs from the others. |
| prb_cartwheel_flexure | `(blade_count,blade_length,blade_width,blade_thickness,material,pivot,axis[,…])` | **arg 6** | `compound.rs:274`. | 0 calls found. | `axis: Direction` | |

"arg5 direction" is therefore wrong for 3 of the 13 flexure ctors (let_joint and cartwheel take it at arg 6, parallelogram at arg 5 but with a different order). Flexure ctors have no compile-time slots at all (flexure_signatures.rs only classifies the result type).

## F. Excluded or confirmed not-a-direction
- `translate(target, dx,dy,dz)`: **confirmed a displacement**, not a direction. Slots are `length_arg(1..3)`. The eval arm `transform_translate` uses `required_length_args`. Exclude.
- `normal(surface, at)`: **output** only, typed `Vec3<dimensionless>` (units.rs:1563). It could return a Direction; this is not an arg site.
- `extrude`, `extrude_symmetric`: scalar distance, no direction arg.
- `plane_through` and `axis_through` take points. `orient_exp`, `orient_log` and Twist are rotation vectors. `scale(target, Vector3)` is a factor vector.
- stdlib structure fields (not callable fns, same migration surface):
  - `fdm.ri:107` build_direction (Vec3<Dimensionless>).
  - `fea_multi_case.ri:349` PointLoad.direction and `:529` Gravity.direction. These scale the force by |direction| (elastic_static.rs:4914/:4956).
  - `fea_multi_case.ri:427` PressureLoad.direction (String).
  - `modal_analysis.ri:526` reference_direction, and `:637,:704,:773,:843` force directions.
  - `modal_analysis_fns.ri:168,:209`.
  - `kinematic.ri:169,:191,:204,:213,:214` joint axes.
  - `ports.ri:63-65` Frame3 x/y/z_axis, which hold zero-vector placeholders in 63 corpus literals.
  - `ports_mechanical.ri:140,:154` port axes.
  - `constitutive.ri:58-60` MaterialFrame axes; `rotate_voigt` is k^4-sensitive.
  - `joints.ri:59,:80,:95` datum joints take `Axis` and are a different construct.

## Dispatch mechanism
- **Compile time: dispatch by arg count, not by type.**
  - `mirror` branches on `n == 7` or `n == 2` (G:2045). `circular_pattern` branches on `n == 9` or `n == 4`. `rotate` matches `compiled_args.len()` on 5 or 2 (GT:37). `draft` uses 3 or 4. `offset` is arity-gated at 2 versus 3.
  - Each arm emits differently **named** op args, and eval then tests which named arg is present: `"plane"` (`pattern_mirror`), `"axis"` (`pattern_circular`), `"orientation"` (`transform_rotate`), `"third"` (`modify_offset_curve`, which disambiguates on `Value` variant).
  - Builtins without an overload (`linear_pattern`, `linear_pattern_2d`, `half_space`, `revolve`, `revolve_full`, `extrude_infinite`, `arc`, `rotate_around`) call `check_arg_count_exact`. Any other count is a labelled error, so a new arity can sit beside the old one only by editing that check into a multi-arity arm.
- **Answer to (1): a Direction-taking form CAN be added beside the scalar form, provided its arity differs.**
  - Arities are 3 (rotate), 4 (linear_pattern), 7 (linear_pattern_2d), 3 (mirror), 2 (half_space), 3 or 4 (revolve), 2 (revolve_full), 5 (arc) and 3 (extrude_infinite), so no scalar arity collides.
  - So scalar-form removal is a deprecation policy choice, not a dispatch necessity. Removing it **is** required for same-arity sites, where the new form replaces or widens the old one in place: the 4 selectors, `orient_axis_angle`, `orient_look_at`, `orient_basis`, the joint ctors, the flexure ctors, and the 3-arg `offset_curve`. These are type-checked at eval time, so they widen rather than overload.
  - Each new arity needs three synchronised edits. First, a match arm in the lowering. Second, a **new arity-guarded arm** in `builtin_arg_slots` (e.g. `"revolve" if arg_count == 8`). The builtin_signatures HAZARD block states why: an arity-agnostic index would fire on the wrong parameter. Third, a row in the pinned ledger `LOWERING_ACCEPTED_ARITIES` (builtin_signatures.rs:~4160-4230), with a new arity such as `("rotate", Exactly(&[2,3,5]))` and `("mirror", Exactly(&[2,3,7]))`. Skipping the ledger edit turns `lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table` red; the comment names a gained arity as "FINDING 2's hazard arriving". Names that are arity-agnostic today (`revolve`, `revolve_full`, `rotate_around`, `half_space`, `arc`) must gain guards in the same change.
  - The 4 selectors are in `ARITY_UNOBSERVABLE_SLOT_KEYS`, which the ledger test re-measures, so a widen-in-place change needs no ledger edit there.
- **Type-based overloading is not available** at compile time (`compiled_args.len()` only). Two same-arity forms (for example `mirror(g, Plane)` and `mirror(g, Direction)`) would have to be told apart by `result_type`, as `scale` does for Vector3 versus Real (GT:`is_vector3`), or at eval by `Value` variant.

## Lockstep surfaces
**LSP (crates/reify-lsp/src/completion.rs, `BUILTIN_FUNCTIONS` at :493): only 4 direction-bearing rows exist.**
- `half_space` (:544, signature `half_space(px: Length, py: Length, pz: Length, nx: Real, ny: Real, nz: Real)`, plus a `DIMENSIONLESS_NORMAL_NOTE` and a `LENGTH_GATED_EXAMPLES` entry `"half_space(0mm, 0mm, 0mm, 0, 0, 1)"`). Both are pinned behaviourally by `gated_length_builtins_advertise_their_dimension_requirement` and `length_gated_tables_stay_in_sync`, so changing half_space edits three places in one file and the example must still evaluate clean.
- `orient_basis` (:946), `orient_axis_angle` (:952), `orient_look_at` (:958). They use untyped `Vector` args.
- There are **no** rows for rotate, rotate_around, mirror, revolve, revolve_full, linear_pattern(_2d), circular_pattern, extrude_infinite, arc, the selectors, frame_at, the joints or the flexures.
- Rows for `plane_xy/xz/yz` (:977-989, signature `plane_xy() -> Frame`) and `axis_x/y/z` (:995-1007, `axis_x() -> Vector`) are wrong for direction-bearing datums today. Fixing them is outside the migration but in the same file.

**MCP chunks (crates/reify-mcp/src/tools/chunks/*.md)**, call-form lines:
- geometry.md: 66 (arc), 83 (half_space sig), 95 (half_space example), 144 (revolve), 165 (rotate example), 234 (rotate_around example), 210-211 (revolve and rotate_around "axis direction" table rows), 232-233 (comment), 258-273 (mirror and linear_pattern prose).
- stdlib.md: 34 (offset_curve), 35 (extrude_infinite, revolve), 36 (translate, rotate, rotate_around, mirror), 37 (linear_pattern, _2d, circular_pattern), 39 (arc). Lines 35-40 are one-line catalogue entries that name arg lists.
- topology.md: 68, 69, 71, 72 (the four selectors, documented as `direction`), 100 (faces_by_normal example), 22 (prose about `rotate(geo, axis, angle)` documentation drift).
- units.md: 78-81 (mirror example), 118 and 121 ("trailing axis stays bare"), 139, 154, 186, 194, 204 (prose about axis and normal staying bare). The "bare axis/normal" rule is stated in prose here and will need rewriting.
- No chunk mentions orient_axis_angle, orient_look_at, orient_basis, frame_at, axis_through, plane_through, midplane, the joints or the flexures.
- Totals (call-form lines): geometry.md ~10, stdlib.md 6, topology.md 6, units.md ~9. The chunks are gated by crates/reify-compiler/tests/harness_doc_chunks/{geometry_chunk_smoke, stdlib_chunk_geometry_ops_smoke, fence_gate, oracle_xref_smoke}.rs (named in the SYNC comments in geometry.md:390 and stdlib.md:29). Fenced examples are compiled, so any example call must be migrated in the same change.

**docs/reify-stdlib-reference.md lines:**
- Orientation: 278 (orient_axis_angle), 281 (orient_basis), 282 (orient_look_at), 325 and 386 (orient_to_axis_angle).
- Primitives and sweeps: 522-528 (half_space, with a "current compiler form" comment), 531-537 (extrude_infinite), 601-603 (arc), 697-699 (offset_curve; 699 is the direction overload), 700 (draft), 712-713 (revolve and revolve_full, documented in Axis value form).
- Transforms and patterns: 742-745 (translate, rotate, rotate_around; the vector forms are marked "not implemented"), 755 (that explanatory note), 762-765 (mirror, linear_pattern, circular_pattern, linear_pattern_2d; the Vector3 and Axis forms are spec only), 773.
- Selectors and queries: 819 (`normal`, output), 833-834 (faces_by_normal and edges_parallel_to documented with `Vector3<Dimensionless>`).
- Datums: 938-943 (plane_xy..., axis_x...).
- Joints: 2112 (placeholder note), 2123-2124 (prismatic and revolute), plus the examples at 2276, 2277 and 2302.
- Not documented there at all: frame_at, axis_through, plane_through, midplane, faces_perpendicular_to, edges_perpendicular_to, cylindrical, planar and every `prb_*` ctor.

## Counting rules
- **Scripted `.ri` counts**: `python3 -I /tmp/dir-study2/count.py`, over `git ls-files` with suffix `.ri`, all 917 files. Comments are stripped with `//[^\n]*`. A call is `(?<![\w.])NAME\s*\(` with `fn NAME` excluded. Arity is counted by a paren-balanced scan with strings skipped, and commas inside nested `()[]{}` are not counted. The counts include `.ri` under crates/*/tests/fixtures/, tests/prd-gate/fixtures/ and examples/, not only examples/ and stdlib/. That explains differences from S4's partition totals (S4's prism/faces_by_normal totals were by callee, ours are by arity).
- **Embedded counts**: `git grep -h -P "(?<![\w.:])NAME\s*\(" -- '*.rs'` (lines, excluding `fn NAME`), and the same for `*.md`. They are raw line counts with no string-literal discrimination.
  - Rust helper fns with the same names (`half_space`, `mirror`, `rotate`, `offset`, `planar`, `prismatic`) and prose inflate the numbers. Treat them as upper bounds.
  - Names with many hits (rotate 112, faces_by_normal 135, prismatic 121, offset 93) are dominated by tests and Rust-side helpers.
  - TS/TSX/JSON: at most 5 lines per name (mirror 3, rotate 3, prismatic 4, revolute 3, cylindrical 4, planar 2, offset 5).
- **Not counted**: named-arg `.ri` calls are counted by position (the scan ignores `name:`), and macro-generated or string-concatenated calls are invisible. `offset/2` and `offset/3` are separated only by arity in the script. `arc/3` is the 2D sketch arc, kept apart from `arc/9`.

## Surprises
1. **The scalar `.ri` population is small.** The scripted counts for 3-scalar sites are far below the Rust/md totals: rotate/5 is 29 calls, but the other builtins are 1-16. The migration cost is dominated by Rust tests, MCP chunk fences and prd-gate fixtures, not user `.ri`.
2. **No 4-arg `linear_pattern` value form exists at HEAD**, nor a 7-arg `linear_pattern_2d`. The brief and the stdlib reference (lines 763, 765) treat them as extant. G:1965 and G:2088 hard-require 6 and 11. The `builtin_signatures.rs` comments say "task 5351 lands" the value form; it has not.
3. **Direction values are rejected nearly everywhere a direction-like arg is consumed.** Rejecting readers: `tensor_components_f64` (helpers.rs:364; used by orient_axis_angle, orient_basis, orient_look_at, joint and flexure axis validation), `decode_plane` and `decode_axis` (GO), `point3_components` (offset_curve), `resolve_vec3_arg` (selectors) and `datum_vec3`. Only `frame_at`, stdlib `decode_plane` (the `midplane` and `offset` path) and the relate/feature-datum readers accept it. Hence `orient_axis_angle(axis.dir, a)` and `prismatic(axis.dir, r)` return Undef today.
4. **Stdlib-minted Axis and Plane carry `Value::Vector<Real>`, not a Direction.** This holds for `axis_x/y/z`, `plane_xy/xz/yz`, `axis_through` and `plane_through`. Only datum projections and kernel-minted datums produce `Value::Direction`. The migration therefore also touches these producers (stdlib/geometry.rs `make_plane`, `make_axis`, `eval_axis_through`, `eval_plane_through`).
5. **Magnitude/zero thresholds do not agree** (S3): eval 1e-12 in `revolve` and `extrude_infinite`, `unit_vector3` at 1e-12 in `decode_plane`/`decode_axis`, a kernel exact-zero test for `half_space`, none for the circular scalar form and `rotate`/`rotate_around` eval. A Direction type removes the need for per-site guards only if its constructor `dir(0,0,0)` rejects zero. That is a PRD decision.
6. **Revolve/extrude_infinite zero-axis is a Warning plus dropped op; `decode_*` zero is an Error.** `modify_offset_curve` is also Error. The behaviour a Direction migration preserves differs per site.
7. **"both" in `extrude_infinite` has no Direction equivalent.** Folding "positive"/"negative" into the sign works; "both" cannot.
8. **Flexure arg index is not uniformly 5** (see section E). The flexure ctors have no compile-time slot rows, so a Direction check there would be a new mechanism or an eval-time check.
9. **Existing joint/flexure validation accepts non-unit vectors** (it returns the raw triple, normalised later at use), so `prismatic(vec3(0,0,2), …)` is legal today. Migration to Direction is a behaviour change for any caller passing a non-unit literal (S4: no such `.ri` sites found for unit-ness, but 17 dimensioned `orient_axis_angle` axes exist).
10. **`orient_to_axis_angle` already returns a Value::Map, and its compiler type `AxisAngle` is intentionally undeclared**; the Direction-valued `axis` field lands with #6004's declaration.
11. **LSP signatures for `plane_xy`/`axis_x` are wrong** (`plane_xy() -> Frame`, `axis_x() -> Vector`) versus the real `plane_xy(δ: Length) -> Plane` and `axis_x(o: Point3) -> Axis`.
12. **Three of the stdlib-reference datum signatures (`revolve(profile, axis: Axis, angle)`, `circular_pattern`, `mirror(plane)`) and Axis-typed `revolute` are spec/placeholder text**; the doc marks the vector-argument forms of translate/rotate/rotate_around "Not implemented".

## Unverified
- No code was executed. In particular, the claim that `Value::Direction` makes `orient_axis_angle`, `prismatic` etc. return Undef is by reading `tensor_components_f64`, not by running a case.
- I did not read every caller of `resolve_vec3_arg` beyond the five listed (GO:7204, 7218, 7250, 7264, 8897) or check `angle(a,b)` as a direction consumer.
- Embedded rs/md counts were not filtered to string literals (see Counting rules). I did not check `.md` files outside the chunks and the stdlib reference for embedded `.ri`.
- The `prb_*` call counts come from a regex over names I listed; `prb_notch_elliptical` and `prb_notch_right_circular` have 0 hits, and several zero-count flexures are inferred zero.
- `sdf_half_space(plane_xy(1mm))` appears in a prd-gate fixture and PRD (implicit-solids); I found no implementation in `crates/`, so I did not include it as a builtin.
- I did not run the harness_doc_chunks tests to confirm which chunk fences are compiled versus only syntax-checked.
- Relation verbs (`parallel`, `perpendicular`, `angle`, `distance`) accept datum Axis/Plane/Direction (S3) but I did not tabulate them since their operands are datums, not direction args.
- Joint ctor arities 3 and 4 in the corpus (e.g. `prismatic/4`) were counted but not inspected for what the extra args are.

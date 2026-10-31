# PRD — Direction typing: one direction type, explicit conversion, enforced unit invariant

**Milestone:** v0_6 · **Status:** active · **Approach:** B + H (contract + two-way boundary tests) · **Authored:** 2026-10-30 (team study 2026-10-29; rulings Leo 2026-10-29/30; fable critic pass folded 2026-10-30)

**Code anchors** verified against main `f2c2220302` (2026-10-30). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Supersedes** the dual-spelling ruling recorded in task #6090 (Leo, 2026-08-07: "keep BOTH spellings … Direction widens IMPLICITLY to Vector3<Dimensionless>") and the field-typing half of task #5848 (steward, 2026-08-01: "a direction/axis field is typed `Vec3<Dimensionless>` … NOT a new `Direction` type"). Both reversals are Leo's rulings of 2026-10-29, with the reasons in §3. The `Direction` type itself is **not** new: it was designed and landed by the geometric-relations PRD (`docs/prds/v0_6/geometric-relations.md` β, task #4382; design of record `docs/design/geometric-relations.md` §2: "a new first-class type (dimensionless, unit-normalized)"). This PRD widens its role from "projection codomain" to "the direction type" and gives it the invariant, constructors, conversion and acceptance rules that design never specified. Design §2's "no Point→Direction projection" stays: `dir(v)` converts a *vector*, never a point.

---

## 1. Goal & user-observable surface

A `.ri` author has exactly one way to write a direction, and every place that consumes one rejects anything else, loudly, at `reify check` or at eval with a coded diagnostic:

```reify
let up        = dir(0, 0, 1)                    // normalised by explicit contract; dir(0,0,0) is E_DegenerateDirection
let spin      = Revolute(axis: dir(0, 1, 0))     // Revolute.axis : Direction
let rotated   = rotate(body, dir(1, 0, 0), 90deg)
let m         = mirror(body, point3(0mm, 0mm, 0mm), dir(0, 0, 1))
let f         = frame_at(origin, a.dir, pl.normal)   // Frame datum; f.x / f.y / f.z are Directions
let load      = PointLoad(point: "tip", force: 100.0, direction: dir(0, 0, -1))
let disp      = 5mm * up                           // Vector3<Length>: a scaled direction is a displacement
let n         = dir(cross(up, dir(1, 0, 0)))       // a unit normal from two directions
let v         = vector(up)                         // the conversion to Vector3<Dimensionless>
```

and these are errors, each naming the expected type and (for the vector case) the fix:

```reify
Revolute(axis: vec3(0, 1, 0))      // error: argument 'axis' has type Vector3<Int> but param 'axis' requires Direction — write dir(x, y, z)
Revolute(axis: vec3(0, 0, 0))      // same error (the vector form is simply not accepted)
dir(0, 0, 0)                       // E_DegenerateDirection: no unit Direction exists for a zero vector
dir(1mm, 0, 0)                     // DimensionedArgRejected: a direction is dimensionless
rotate(body, 0, 0, 1, 90deg)       // error: rotate takes (Geometry, Direction, Angle) — write rotate(body, dir(0, 0, 1), 90deg)
orient_axis_angle(vec3(1m, 0mm, 0mm), 90deg)   // error: argument 'axis' requires Direction
frame_at(o, vec3(1, 0, 0), vec3(0, 0, 1))      // error at check (today: clean check, undef at eval)
up + up                            // error: `+` is undefined for Direction — write vector(up) + vector(up)
A(d: "hello")                      // error where today it passes silently (Direction/Axis/Plane/Frame slots are checked)
MassProperties(..., origin: ...)   // error: unknown field — the field is deleted
```

`Vector3<Dimensionless>` remains the type of a dimensionless vector that is **not** a direction — a mode shape, a Jacobian column, a curl field, a ratio triple — and nothing coerces between the two at any acceptance site.

Consumers (G1): every `.ri` author of joints, loads, frames, datums, selectors and transforms (stdlib, `examples/`, `prj/`, `designs/`, `gui/test`, `tree-sitter-reify/test`); the V2 quantity-slot tightening #9568 (its migration target is this PRD's `dir(...)`); the one-verdict acceptance relation #9571 (its Direction/Axis/Plane/Frame rows are this PRD's δ); FEA (`extract_loads`, material frames), modal, dynamics/joints, flexures, relate-solve and the OCCT kernel boundary, all of which read directions through one decoder after this PRD; LSP completion; the MCP doc chunks.

## 2. Background — what the 2026-10-29 study measured

Full record: `docs/notes/direction-typing-study-2026-10-29.md` (dated snapshot, committed beside this PRD; its Appendix E is the builtin signature table θ/ι implement from). The facts the design turns on, all verified on a fresh binary at main `ba2ae8e34e`:

- **Four spellings of "a direction" are live.** (a) `Direction`: produced only by datum projections (`axis.dir`, `plane.normal`, `frame.x/y/z`) and the kernel; six `let` annotations in two examples; **zero** stdlib fields. (b) `Vector3<Dimensionless>`: 23 stdlib direction fields (17 non-frame + the 6 `Frame3`/`MaterialFrame` axes); ~235 unitless `vec3(..)` direction literals in the corpus (172 direction-role + 63 all-zero `Frame3` placeholders). (c) unitless `vec3` at a `Vector3<Length>` slot: zero literal sites left (task #5848 retired the idiom), but the 5766/6159 arg-side tolerance still admits it silently at ctor/default. (d) three bare scalars `(nx, ny, nz)` in `rotate`, `rotate_around`, `mirror`, `half_space`, `revolve(_full)`, `linear_pattern(_2d)`, `circular_pattern`, `extrude_infinite`, `arc`: ~89 triples over ~64 `.ri` calls, deliberately unslotted.
- **Nothing enforces unit length anywhere.** `Value::Direction { x, y, z }` is "assumed unit-normalized"; two of six Rust constructors normalise, the rest pass through. Consumers disagree: joints normalise at use; the material frame reaches `rotate_voigt` raw, so a non-unit axis rescales stiffness by k⁴ (pinned by a FENCE test, `material_frame_is_not_normalised_so_a_non_unit_axis_moves_d_global`, not fixed); `PointLoad.direction` / `Gravity.direction` scale the force by |direction| while their docs demand unit input; two FEA/modal readers silently default to ±Z on anything they do not recognise; `frame_at` accepts anything statically and yields undef at rc 0; relations accept only `Direction`/`Axis`.
- **Direction slots are not checked at all.** `A(d: "hello")`, `A(d: 5)`, `A(d: vec3(0,0,1))` pass `reify check` at ctor and default sites (unexamined family, #7958) and the laundered value then passes exact-match user fns and relations. Builtin slots have no `Direction` kind (`ExpectedArg` is `Scalar | Int`), so no builtin can reject a non-direction statically.
- **Arithmetic on `Direction` type-checks and evaluates to undef**: `d + d`, `-d`, `dot/cross/normalize/magnitude(d)` (and `normalize(d)` is typed `Vector0<Real>`).
- **Zero-vector thresholds disagree five ways** across eval (1e-12 on |v|), Rust kernel arms (`AXIS_MAG_SQ_MIN` 1e-12 on |v|²), C++ (1e-30), linear_pattern (1e-10) and `gp_Dir` itself (2e-308); `circular_pattern`'s scalar form, `split` and the moment-of-inertia query have no reify-side guard.
- **Value shape is split**: the kernel mints `Plane.normal` / `Axis.direction` as `Value::Direction`; `.ri` constructors (`make_plane`, `make_axis`, `axis_through`, `plane_through`) and the solver's `transform_datum` mint `Value::Vector`; reify-eval's `decode_plane` / `decode_axis` and reify-expr's `datum_vec3` reject the kernel form; the compiler lockstep test `has_codomain_kind` pins the Vector form.
- **Branch `task/6090`** (22 commits, 24 files, +1183/−29, one merge conflict against main in `crates/reify-expr/src/lib.rs`, 1217 files of drift, never built against current main) implements most of the dual-spelling ruling: `dir(v)` (arity 1 only) with `DegenerateDirection`, `DIRECTION_DEGENERACY_FLOOR = 1e-12`, `unit_direction`, `widen_direction`, Direction leaf-family promotion, `±` rejection, negation, dot/cross/magnitude widening, `normalize(Direction)` identity. Everything except the implicit language-level widening is ported here (by cherry-pick; the first leaf touching `reify-expr/src/lib.rs` owns the conflict).
- **No compelling reason against uniformity was found.** Rotation vectors (`orient_exp/log`, `Twist`, |ω| = angle) are not directions and stay `Vector3<Angle>`. `Direction` is signed (`parallel` ≠ `antiparallel` in relate-solve); sign-insensitive consumers (`edges_parallel_to`, `perpendicular`) consume a signed type fine; an undirected line is the `Axis` datum. No 2D direction exists in the corpus. OCCT's `gp_Dir` is the same concept and self-normalises; the IR carries every axis as a bare `[f64;3]`, so the kernel boundary is type-agnostic and only eval's decoders change.

## 3. Resolved design decisions

| # | Decision | Ruled |
|---|---|---|
| D1 | **`Direction` is the single direction type.** Every field, param, argument and return that *means* a direction (unit, dimensionless, signed, 3-vector) is `Direction`. `Vector3<Dimensionless>` is reserved for dimensionless non-directions. Reverses #6090's "keep both" and #5848's "not a new Direction type". Why the facts changed: "unimplementable" expired (branch task/6090); the migration is larger than #6090 stated, not smaller (23 fields, ~235 literals); and the study found the k⁴ rescale, the force-scaling loads, the accepted zero axis, the laundering hole and the silent-default readers — all consequences of the unit property having no type home (code-quality heuristics 3 *orthogonal dimensions*: dimensionless ≠ unit; 10 *invariants enforced uniformly*; 11 *SPOT*). #5848's two grounds (the grammar gap had closed; Invariant V, dimensionless == `Value::Real`) are both compatible with `Direction`. | Leo 2026-10-29 |
| D2 | **No coercion between `Direction` and `Vector3<Dimensionless>` at any acceptance site.** `dir(v)` is the only way to obtain a `Direction` from a vector (normalising by explicit contract); `vector(d)` is the named conversion back. The scalar product `s * d` (D3) is an *operation* whose codomain is a vector, not a coercion. `type_compatible`, `implicitly_converts_to`, the overload ladder and the `ArgVerdict` relation carry no Direction arm. Consequence: #8586 (admit a widening in overload resolution) is moot. | Leo 2026-10-29 |
| D3 | **Geometric operations on a `Direction` need no conversion; everything else is an error.** Allowed: `-d` → `Direction`; `s * d` and `d * s` → `Vector3<Q(s)>` (`2mm * d : Vector3<Length>`, `2.0 * d : Vector3<Dimensionless>`); `dot(d1, d2)` → `Real`; `cross(d1, d2)` → `Vector3<Dimensionless>` (magnitude sin θ; a unit normal is `dir(cross(d1, d2))`); `angle(d1, d2)` → `Angle`; components `d.x / d.y / d.z` → `Real`; `normalize(d)` → identity. Errors (`ArithOperandKind` / `ArgTypeMismatch`, each hinting `vector(d)`): `d ± anything`, `d * d`, `d / s`, `magnitude(d)`, and the mixed forms `dot(d, v)`, `cross(d, v)`, `dot(v, d)`, `cross(v, d)`. | Leo 2026-10-30 |
| D4 | **Unit length is an enforced invariant with one home: construction.** `Value::Direction` becomes a newtype over a private `UnitVec3` whose only constructor normalises and rejects degenerate input; nothing can hold a non-unit `Direction`, so no decoder re-checks magnitude. Redundant enforcement (heuristic 10) is construction (reify) plus the kernel-side floors, which stay because the IR still carries raw `[f64;3]` axes from non-direction producers. One named constant in reify-core: `DIRECTION_DEGENERACY_FLOOR`. | Leo 2026-10-29 (mechanism: this PRD; critic pass removed the vacuous decode-side tolerance) |
| D5 | **Frames: retire everything but the `Frame` datum.** The stdlib structs `Frame3` (`ports.ri`) and `MaterialFrame` (`constitutive.ri`) are deleted; `LocatedPort.frame` and `AnisotropicMaterial`'s frame become `Frame`; `frame_at(origin, x: Direction, z: Direction)` is the constructor; one public quaternion→basis helper replaces the three private copies (`quat_basis_axis`, `quat_to_rotation_matrix`, the orientation.rs rotation). Orthonormality is then by construction, which closes the k⁴ rescale. | Leo 2026-10-30 |
| D6 | **Delete `MassProperties.origin : Frame3`.** The RBD PRD defined it only as "body frame the above are expressed in"; no production consumer reads it; both Rust minters write all-zero axes with a comment calling the zero "load-bearing" for byte-agreement only; 21 example/fixture literals write `vec3(0,0,0)` axes to satisfy the type. The dynamics solver already treats `com` and `inertia` as expressed in the body frame; that convention is documented on the struct instead. Reintroducing a real body-frame pose later is a widening, not a correction. | Leo 2026-10-30 |
| D7 | **Bare-scalar direction builtins migrate in this PRD and the scalar forms are retired.** `rotate`, `rotate_around`, `mirror`, `half_space`, `revolve`, `revolve_full`, `linear_pattern`, `linear_pattern_2d`, `circular_pattern`, `extrude_infinite`, `arc` gain Direction-taking signatures; a leftover scalar call is a compile error naming the new form. One spelling, loud (heuristic 12 *structured data instead of meaningful scalars*). | Leo 2026-10-30 |
| D8 | **Datum-slot conformance is promoted for all four families.** `Direction`, `Axis`, `Plane`, `Frame` slots are checked by exact identity at ctor and param-default sites in one leaf (absorbs #7958); user fn-call slots already reject by exact match. A `Vector3` argument at a `Direction` slot is rejected with the hint `write dir(x, y, z)`. Builtin slots get a `Direction` kind of their own (σ) so `frame_at`, the selectors and the orientation builtins reject at check, not at eval. | Leo 2026-10-30 (σ: critic pass) |
| D9 | **`PressureLoad.direction` becomes a data-carrying enum** `PressureDirection { Normal, Fixed { dir: Direction } }` (default `PressureDirection.Normal` = outward face normal). G3 found that variant payloads must be compile-time literals today (`compile_variant_construct`, "non-constant payload value … is not yet supported"; no task existed), so this PRD carries the prerequisite leaf κ₀ (runtime variant construction, paralleling `StructureInstanceCtor`). | Leo 2026-10-30 |
| D10 | **`extrude_infinite`'s sign string folds into the direction; "both" becomes a sibling builtin.** `extrude_infinite(profile, dir: Direction)` extrudes along `dir`; `extrude_infinite_symmetric(profile, dir: Direction)` replaces `"both"`, mirroring the existing `extrude` / `extrude_symmetric` pair. The String arg is retired. | this PRD (least surprise: existing naming) |
| D11 | **`dir` has two arities**: `dir(x, y, z)` over dimensionless scalars/ints (the literal form; also the `Display` round-trip spelling and the serialiser's output) and `dir(v: Vector3<Dimensionless>)`. A dimensioned argument is `DimensionedArgRejected`; a zero argument is `E_DegenerateDirection` (runtime `Severity::Error`, never bare undef). | this PRD |
| D12 | **`angle(a, b)` is Direction-only** on both the value path and the build path (`geometry_ops` reads it through the one decoder); a vector operand is written `angle(dir(a), dir(b))`. | this PRD (uniformity; critic M-7) |
| D13 | **Corpus partition rule: a corpus site is migrated by the leaf that retypes its callee.** `prismatic(vec3(..))` sites belong to the joints slice, `orient_axis_angle` sites to the orientation slice, chunk fences to the slice that changes the documented signature. No two leaves edit the same call site. | this PRD (critic BL-1) |
| D14 | **Landing shape**: port branch `task/6090`'s mechanism (reify-stdlib `direction.rs`, `DegenerateDirection`, conformance promotion, `±` rejection, negation, the linalg arms) minus `direction_widens_to` and the ctor/default widening arm; #6090 is cancelled in favour of α/β/γ/δ at decompose time (it is deferred on exactly that condition). | this PRD |

## 4. Pre-conditions for activating

- **ι #8739** (compiler-declaration-passes: the `conformance_accepts` / `ArgVerdict` relation in `type_acceptance.rs`; in progress, escalated once as esc-8739-2 and narrowed to behaviour-preserving) — δ lands its datum-family rows in that relation, so δ depends on #8739, and through δ so do ζ-a..d, κ and μ. Phase 1's α, β, γ, ε, σ and the η, θ, ι slices are independent of it.
- No novel grammar. `dir(...)`, `vector(...)`, `frame_at(...)` and the named-field enum form all parse today (grammar gate: 13 fixtures, 0 ERROR nodes, `tests/prd-gate/fixtures/direction_typing_*.ri`).
- Substrate verified present: `Frame`-typed struct params and `frame_at` construction (`type_resolution.rs` `"Frame" => Type::Frame(3)`; `eval_frame_at` Gram-Schmidt via `orient_basis`); DCE named-field declaration and the dotted bare-variant default (`PressureDirection.Normal` — the `::` form resolves as a trait lookup and fails); `Direction` survives `let`, param default, struct field and `List` at eval; `extrude_symmetric` exists as the naming precedent; `LOWERING_ACCEPTED_ARITIES` and its coupling tests exist in `builtin_signatures.rs`; chunk fences tagged ```reify are compiled as complete modules by `harness_doc_chunks/fence_gate.rs`.
- Substrate verified **absent** and queued as leaves: `dir`/`vector` builtins (β, γ); a `Direction` kind in `ExpectedArg` (σ); a public quaternion→basis helper (η); runtime-computed enum payloads (κ₀); a `linear_pattern` value form (ι-c — the 4-arg form documented in the stdlib reference does not exist; #5351 is absorbed).

## 5. Contract (H component)

Implementers read this section as the specification; the boundary tests in §6 are its executable half.

### C1 — The type

`Type::Direction` (existing, `crates/reify-core/src/ty.rs`) denotes a finite, dimensionless, **unit**, **signed** 3-vector. It is distinct from `Vector3<Q>` for every `Q`, from `Orientation`, `Axis`, `Plane`, `Frame` (locked by the existing distinctness tests). 2D has no direction type; this PRD does not add one.

### C2 — The value and its only constructor

```rust
// crates/reify-core/src/direction.rs (new)
pub const DIRECTION_DEGENERACY_FLOOR: f64 = 1e-12;   // |v| at or below this cannot be normalised (from branch task/6090)

// crates/reify-ir/src/direction.rs (new)
pub struct UnitVec3 { /* private; |v| == 1 by construction */ }
pub enum DirectionFault { Degenerate { magnitude: f64 }, NonFinite }
impl UnitVec3 {
    pub fn from_vector(v: [f64; 3]) -> Result<UnitVec3, DirectionFault>;   // normalises; the ONLY way to make one
    pub fn negated(&self) -> UnitVec3;                                      // exact; needs no re-normalisation
    pub fn components(&self) -> [f64; 3];
}
// crates/reify-ir/src/value.rs
Value::Direction(UnitVec3)   // replaces Value::Direction { x, y, z }; no public field construction
```

Every current construction site (six non-test: reify-expr `vec3_to_direction`, `frame_axis_direction`; reify-constraints `transform_datum`; reify-eval `direction_value`; reify-kernel-occt analytic surface/curve datums) goes through `from_vector`. Kernel-minted and solver-rotated vectors are unit by upstream construction; re-normalising them costs one divide and makes the invariant unconditional rather than trusted. The hand-written `PartialEq` / `Ord` / content-hash arms and `Display` in `value.rs` change shape (not semantics); `Display` prints `dir(x, y, z)` so output round-trips. `NonFinite` is reachable from Rust only — the `.ri` surface has no NaN literal and `0.0 / 0.0` is already undef — so α pins it in a Rust test and `.ri` sees only `Degenerate`.

### C3 — The only decoder

```rust
// crates/reify-ir/src/direction.rs
pub enum DecodeDirectionError { NotADirection { got: &'static str } }
pub fn decode_direction(v: &Value) -> Result<UnitVec3, DecodeDirectionError>;
```

A variant match: accepts **only** `Value::Direction`. The message names the kind received and ends with the fix hint `write dir(x, y, z)`. Every consumer that reads a direction uses it — joints (`validate_axis` / `unit_axis_from_map`), flexures, supports, FEA loads (`read_direction_or_neg_z` is deleted; its pinned "absent ≠ wrong" default becomes a loud `FeaValueShapeError::NotADirection`), material frames (via C8), modal (`extract_reference_direction`, transient `read_vec3`), as-printed (`field_vec3`), selectors and `angle` (`resolve_vec3_arg`), `offset_curve`, `frame_at` (stdlib `decode_direction` is replaced by this one), orientation (`orient_axis_angle`, `orient_look_at`, `orient_basis`), relate-solve (`dir_of`), stdlib `decode_plane` (narrowed from Vector-or-Direction) and the eval decoders for Axis/Plane. No consumer may accept a `Value::Vector` where a direction is meant, and no consumer may default on a mismatch.

### C4 — Surface constructors

| Form | Static type | Eval |
|---|---|---|
| `dir(x, y, z)` — `x,y,z : Int \| Real \| dimensionless Scalar` | `Direction` | `from_vector`; `E_DegenerateDirection` on `Err`; `DimensionedArgRejected` if any component carries a dimension |
| `dir(v)` — `v : Vector3<Dimensionless>` (or a `Vector3<Int>` literal) | `Direction` | as above; a `Direction` argument is identity |
| `axis.dir`, `plane.normal`, `frame.x/.y/.z`, `self.x/.y/.z`, `<feature>.dir/.normal` | `Direction` | existing projections, now via C2 |
| `normal(surface, at)` | `Direction` (was `Vec3<dimensionless>`) | kernel face normal via C2 |
| `axis_through(a, b).dir`, `plane_through(a, b, c).normal`, `axis_x(o).dir`, `plane_xy(δ).normal` | `Direction` | the stdlib constructors mint `Value::Direction` inside Axis/Plane (C9) |

`dir` joins `DATUM_CONSTRUCTOR_NAMES` (typed via `datum_constructor_result_type`); it is a call name and does not collide with the `.dir` projection member (call and member namespaces are separate, as `normal` already demonstrates; the pairwise-disjointness test over builtin name families must stay green). Shadowing by a user `let dir = …` follows the existing rule for every builtin name.

### C5 — Named conversion

`vector(d: Direction) -> Vector3<Dimensionless>` (a math-family builtin beside `normalize`). D2 governs: no acceptance site coerces in either direction.

### C6 — Operator table (D3)

| Expression | Static | Eval |
|---|---|---|
| `-d` | `Direction` | `negated()` |
| `s * d`, `d * s` (`s : Scalar<Q> \| Int \| Real`) | `Vector3<Q>` (`Vector3<Dimensionless>` for a bare number) | components × s |
| `dot(d1, d2)` | `Real` | ∈ [−1, 1] |
| `cross(d1, d2)` | `Vector3<Dimensionless>` | not normalised |
| `angle(d1, d2)` | `Angle` | acos(dot) |
| `d.x`, `d.y`, `d.z` | `Real` | component |
| `normalize(d)` | `Direction` | identity |
| `d + x`, `d - x`, `x ± d`, `d * d`, `d / s`, `magnitude(d)`, `dot(d, v)`, `cross(d, v)`, `dot(v, d)`, `cross(v, d)`, `angle(v, …)` | **error** `ArithOperandKind` / `ArgTypeMismatch`, message ends "write vector(d)" (or "write dir(v)" for the `angle` vector operand) | — |
| `d == d`, `d < d` | **error** (existing: not an equatable/orderable kind) | — |

### C7 — Acceptance (D8)

| Site | Mechanism | Diagnostic |
|---|---|---|
| struct ctor, param default | δ: exact-identity rows for `Direction`/`Axis`/`Plane`/`Frame` in the `ArgVerdict` relation (`type_acceptance.rs`, from #8739), reached through the general concrete-leaf arm | `ArgTypeMismatch` naming the expected type; a `Vector3<_>` at a `Direction` slot adds "write dir(x, y, z)" |
| user fn-call | existing exact match (`resolve_function_overload`); the `dir(...)` hint on `NoMatchingOverload` is #9571's (it owns the fn-call site's consultation of `ArgVerdict`) | `NoMatchingOverload` |
| builtin slot (`frame_at`, selectors, orientation, `offset_curve`, the ι forms) | σ: `ExpectedArg::Direction` in `builtin_arg_slots` plus its check arm | `ArgTypeMismatch` naming `Direction` |
| relation operand | existing lift (`Axis → .dir`, `Plane → .normal`); a `Vector3` stays `DatumProjectionUnavailable` | existing |
| `let` annotation | out of scope (§9; #9571) | — |

### C8 — Frames (D5)

`Type::Frame(3)` / `Value::Frame { origin: Point3<Length>, basis: Orientation }` is the only frame. Constructor: `frame_at(origin, x: Direction, z: Direction)` (existing; Gram-Schmidt, errors on parallel/degenerate). One public helper, the single home for quaternion→basis:

```rust
// crates/reify-ir/src/direction.rs (beside decode_direction)
pub fn frame_basis(frame: &Value) -> Result<[[f64; 3]; 3], DecodeDirectionError>;   // rows are the frame's x, y, z — orthonormal by construction
```

replaces reify-expr `quat_basis_axis`, reify-stdlib `quat_to_rotation_matrix` and the orientation.rs rotation for this purpose (heuristic 11). The FEA material reader builds `rotate_voigt`'s rows from it; `LocatedPort.frame : Frame`; `AnisotropicMaterial(frame: Frame)`; as-printed minters emit `Value::Frame`. The FENCE test `material_frame_is_not_normalised_so_a_non_unit_axis_moves_d_global` is replaced by its inverse: a frame reaching `rotate_voigt` is orthonormal, pinned through `frame_basis`; the existing identity-frame bitwise contract (`to_bits`) is kept.

### C9 — Value-shape SPOT for datums

`Value::Axis.direction` and `Value::Plane.normal` are **always** `Value::Direction` — minted so by the kernel (already), by the stdlib constructors (`make_axis`, `make_plane`, `eval_axis_through`, `eval_plane_through`, `frame_plane`) and by the solver's `transform_datum` (which re-mints through C2). reify-eval `decode_plane` / `decode_axis` and reify-expr `datum_vec3` read them through C3; stdlib `decode_plane` narrows to Direction-only; the pinned tests `decode_axis_direction_variant_still_fails_the_pre_delta_way` (reify-eval) and `has_codomain_kind`'s `Plane → Value::Vector` row (compiler `datum_projection.rs`) are inverted. Consequence: a kernel-realised axis or plane feeds `circular_pattern(g, axis, …)` and `mirror(g, plane)` for the first time.

### C10 — Thresholds and the kernel boundary

A `Direction` reaching the IR is unit, so the **eval-side** zero paths that only a scalar/vector axis could reach (`sweep_revolve`'s and `sweep_extrude_infinite`'s Warning-and-drop, `unit_vector3` at the Axis/Plane decoders) become unreachable from `.ri` and are deleted with their tests retargeted to `dir(...)` degeneracy. The **kernel Rust-arm** guards (`AXIS_MAG_SQ_MIN`, `validate_revolve_params`, the half-space exact-zero test) **stay**: `GeometryOp::{Rotate, Mirror, Revolve, HalfSpace, …}` are still constructed from raw `[f64;3]` by non-direction producers (sweep classification, feature datums, engine-build tests), and reify-kernel-occt is a library. The C++ floors stay likewise. Heuristic 10's "at use" check is therefore the kernel arm, not a second reify-side decoder.

### C11 — Diagnostics

| Code | Severity | Origin |
|---|---|---|
| `E_DegenerateDirection` (`DiagnosticCode::DegenerateDirection`, ported from branch task/6090) | runtime Error | `dir(...)` on a zero vector |
| `DimensionedArgRejected` (existing) | Error | `dir(1mm, 0, 0)` |
| `ArithOperandKind` (existing) | Error | C6 rejected operators |
| `ArgTypeMismatch` (existing), `NoMatchingOverload` (existing) | Error | C7, with the `dir(...)` / `vector(d)` hints |
| `FeaValueShapeError::NotADirection` (new variant) | Error | a load/frame field that is not a `Direction` (replaces the ±Z silent defaults) |

Every diagnostic carries a code (INV-SF-6); a degenerate direction is never bare undef (INV-SF-1); the removed silent defaults are INV-SF-5 (`placeholders-owned-and-loud`) and `no-silent-fail-soft`.

## 6. Boundary-test sketch (H component)

The integration gate μ lands these as `tests/prd-gate/fixtures/direction_*.ri` plus Rust boundary tests; each row names its postcondition as the observable. Preconditions name the leaves whose landing makes the row true.

| # | Facing | Scenario | Pre | Postconditions |
|---|---|---|---|---|
| B1 | producer | `dir(0, 1, 0)`, `dir(0.0, 2.0, 0.0)`, `dir(vec3(0, 0, 3))` | β | `reify eval` prints `dir(0, 1, 0)`, `dir(0, 1, 0)`, `dir(0, 0, 1)`; static type `Direction` |
| B2 | producer | `dir(0, 0, 0)`, `dir(1e-13, 0, 0)` | β | `E_DegenerateDirection`, exit ≠ 0, cell undef with cause (`NonFinite` is pinned in α's Rust test; unreachable from `.ri`) |
| B3 | producer | `dir(1mm, 0, 0)`, `dir(vec3(1mm, 0mm, 0mm))` | β | `DimensionedArgRejected` |
| B4 | producer | `axis_through(a, b).dir`, `plane_through(a, b, c).normal`, `axis_x(o).dir`, a kernel face `normal(s, at)` | ε (stdlib three), θ-b (`normal`) | each prints `dir(...)`; `decode_direction` accepts each |
| B5 | producer | `frame_at(o, dir(1,0,0), dir(0,0,1)).y` | α, β | `dir(0, 1, 0)`; `frame_basis` rows orthonormal to 1e-12 |
| B6 | consumer | `Revolute(axis: vec3(0,1,0))`, `Revolute(axis: vec3(0,0,0))`, `Revolute(axis: "up")`, `Revolute(axis: 5)` | δ, ζ-a | ctor `ArgTypeMismatch` naming `Direction`; the first two end "write dir(x, y, z)"; nothing reaches eval |
| B7 | consumer | `fn f(d: Direction)` called with `vec3(0.0,1.0,0.0)`, `a.dir`, `dir(0,1,0)` | β | `NoMatchingOverload` for the vector (today); accepted for both Directions |
| B8 | consumer | `param d : Direction = vec3(0,0,1)` | δ | default-site `ArgTypeMismatch` with the hint |
| B9 | consumer | `PointLoad(direction: dir(0,0,-1))`; `PointLoad(direction: vec3(0,0,-2))` | ζ-b | first: tip force `[0, 0, -force]`; second: check error — the old silent ×2 is unreachable |
| B10 | consumer | `MassProperties(mass:…, com:…, inertia:…, origin: …)` | η | error: unknown field `origin`; the 21 migrated literals check clean; `mass_properties(...)` prints no `origin` |
| B11 | consumer | `anisotropic_material(law, frame_at(o, dir(1,0,0), dir(0,0,1)))` (the identity frame) | η | `D_global` bitwise (`to_bits`) equal to `D_local`, as the existing identity contract pins; for any other `frame_at` input the rows are orthonormal to 1e-12 |
| B12 | consumer | `rotate(g, dir(0,0,1), 90deg)`, `rotate_around(g, p, dir(0,0,1), 90deg)`, `mirror(g, p, dir(0,0,1))` | ι-a | geometry byte-identical to the retired scalar form on fixtures whose scalar axis was a signed basis vector (every corpus `.ri` site; `.rs` goldens are re-measured at ι) |
| B12′ | consumer | `revolve(prof, p, dir(0,0,1), 90deg)`, `revolve_full(prof, p, dir(0,0,1))`, `extrude_infinite(prof, dir(0,0,-1))`, `extrude_infinite_symmetric(prof, dir(0,0,1))` | ι-b | same criterion |
| B12″ | consumer | `linear_pattern(g, dir(1,0,0), 3, 5mm)`, `linear_pattern_2d(g, dir(1,0,0), 2, 5mm, dir(0,1,0), 2, 5mm)`, `half_space(p, dir(0,0,1))`, `arc(c, 5mm, 0deg, 90deg, dir(0,0,1))` | ι-c | same criterion |
| B13 | consumer | `rotate(g, 0, 0, 1, 90deg)`, `mirror(g, 0mm,0mm,0mm, 0,0,1)`, `extrude_infinite(prof, 0,0,1, "both")`, `linear_pattern(g, 1,0,0, 3, 5mm)` | ι-a/b/c | compile error naming the Direction form; `LOWERING_ACCEPTED_ARITIES` ledger and its coupling tests green |
| B14 | consumer | `orient_axis_angle(vec3(1m,0mm,0mm), 90deg)` (the printer.ri idiom); `orient_axis_angle(dir(1,0,0), 90deg)` | σ, θ-a | first: check error "argument 'axis' requires Direction"; second: the quaternion main prints today (`[0.7071067811865476, 0.7071067811865475, 0, 0]q`) |
| B15 | consumer | `faces_by_normal(s, dir(0,0,1), 1deg)`, `edges_parallel_to(s, dir(1,0,0), 1deg)`; `faces_by_normal(s, vec3(0.0,0.0,1.0), 1deg)` | σ, θ-b | first two select as today; third is a check error |
| B16 | consumer | `relate { parallel(a.dir, dir(0,0,1)) }`, `parallel(vector(a.dir), …)` | γ | first solves; second `DatumProjectionUnavailable` |
| B17 | consumer | `circular_pattern(g, axis_through(a, b), 4, 90deg)`, `mirror(g, single(faces(g)).plane)` (stdlib- and kernel-minted datums) | ε | evaluate (today: "not a valid 3-component numeric Point/Vector") |
| B18 | both | `2mm * d`, `d * 2.0`, `dot(d, d)`, `cross(d, e)`, `-d`, `normalize(d)`, `dir(cross(d, e))` | γ | typed `Vector3<Length>`, `Vector3<Dimensionless>`, `Real`, `Vector3<Dimensionless>`, `Direction`, `Direction`, `Direction`; eval values correct |
| B19 | both | `d + d`, `d * d`, `magnitude(d)`, `dot(d, vec3(0.0,0.0,1.0))`, `d / 2` | γ | each a check error ending "write vector(d)" |
| B20 | both | `vector(d)` then `vector(d) + vector(e)` | γ | `Vector3<Dimensionless>`; eval sum |
| B21 | consumer | `PressureLoad(direction: PressureDirection.Normal)`, `PressureLoad(direction: Fixed { dir: frame.z })`, `PressureLoad(direction: "normal")` | κ₀, κ | first two evaluate (the second with a runtime payload); third is a check error |
| B22 | consumer | `A(f: "hello")` with `param f : Frame`; `A(a: 5)` with `param a : Axis`; `A(p: vec3(0,0,1))` with `param p : Plane` | δ | each a check error naming the datum type |
| B23 | consumer | `frame_at(o, vec3(1,0,0), vec3(0,0,1))` | σ | check error naming `Direction` at slots 1 and 2 (today: clean check, undef at eval) |
| B24 | docs | every signature in the updated chunks / stdlib reference compiles in a smoke `.ri` | λ | `harness_doc_chunks` green; discoverability: an author searching the chunks for "axis", "normal", "direction", "point a load" finds `dir(...)` |
| B25 | consumer | `angle(dir(1,0,0), dir(0,1,0))`; `angle(vec3(1.0,0,0), vec3(0,1.0,0))` | γ, θ-b | first `90 deg`; second a check error ending "write dir(v)" |

## 7. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometric-relations.md` (β #4382, design §2) | this PRD extends | `Type::Direction`, projections, `ExpectedDatum::Direction` | this PRD owns the type's contract from now on (C1–C7); the design doc §2 gets a one-line pointer here (λ) | wired |
| `compiler-declaration-passes.md` ι #8739 → #9571 (one `ArgVerdict` relation) | this PRD consumes #8739; #9571 consumes δ | datum-family rows in `type_acceptance.rs`; the fn-call `dir(...)` hint | δ lands the rows in the relation #8739 extracts; #9571 unifies the sites, adds the fn-call hint and the `let` row, and takes a dependency on δ | queued (δ depends_on #8739; `add_dependency` #9571 ← δ at decompose) |
| #9568 (V2 quantity-slot tightening) | consumes | `dir(...)` as the migration target | #9568 (released 2026-10-29 with target `Direction`) | `add_dependency` #9568 ← β at decompose |
| #6090 (keep-both ruling; branch task/6090) | superseded | branch ported by α/β/γ/δ | this PRD; #6090 cancelled in favour of those leaves at decompose (its deferral condition) | queued |
| #8586 (overload admits the widening) | moot under D2 | — | cancel at decompose with Leo's say-so | queued |
| #8585 (eval readers accept `Value::Direction`), #8587 (negation), #7958 (datum slots unexamined), #5351 (pattern direction as a value), #9256 (decoded-value directions and selector vec3s dimension-checked) | absorbed | ζ-b/c/d, γ, δ, ι-c, θ-b/ε respectively | this PRD | cancel-in-favour at decompose |
| `builtin-signature-registry.md` decision 7 / #6004 (`AxisAngle { angle, axis: Vector3<Dimensionless> }`) | this PRD amends | `AxisAngle.axis : Direction` | θ-a changes the eval `Value::Map` `axis` to a `Direction` with no dependency on #6004; #6004's text is amended so the nominal struct it declares says `Direction` | companion correction |
| `data-carrying-enums.md` (shipped) | this PRD extends | runtime variant construction node | κ₀ (this PRD) — a compiler feature that also unblocks dynamics' `JointForceValue` tagged union | queued |
| `rigid-body-dynamics.md` (v0_3) §4.1 | this PRD corrects | `MassProperties.origin` | η deletes the field; the PRD gets a companion note (λ) | companion correction |
| `typed-fea-authoring-surface.md` | this PRD retypes | `PointLoad.direction`, `Gravity.direction`, `PressureLoad.direction` | ζ-b, κ | companion correction |
| `dimensioned-construction-strictness.md` §12 / `ty.rs` quantity-slot prose | adjacent | the arg-side tolerance text cites the direction idiom | #9568 rewrites the prose; this PRD only removes the idiom and adds the "unit ≠ dimensionless" paragraph (λ) | wired |
| OCCT kernel (`kernel-seam-contracts.md`) | consumes | `[f64;3]` axes in the IR; the Rust-arm guards | unchanged (C10) | wired |

None of the three known contested-ownership pairs (persistent-naming ↔ multi-kernel, imported-field-source ↔ multi-kernel, topology-selectors ↔ persistent-naming) is touched.

## 8. Decomposition plan

Approach B + H. Labels are Greek; task IDs are assigned at decompose time. Every leaf declares `task_kind` and its observable signal; intermediates name what they unlock. A slice = retype + readers + its own corpus sites + its own chunk fences (D13), so the workspace is green per commit. Counts are from the study note (Appendix C/E); `.rs`/`.md` figures are line-count upper bounds.

**Phase 1 — foundation (the contract)**

- **α — Direction value contract.** reify-core `direction.rs` (the floor constant), reify-ir `direction.rs` (`UnitVec3`, `DirectionFault`, `decode_direction`, `frame_basis`), `Value::Direction(UnitVec3)`, the six construction sites migrated, every pattern site re-shaped (14 non-test files across reify-ir, reify-expr, reify-constraints, reify-eval, reify-kernel-occt, reify-stdlib, reify-compiler incl. `has_codomain_kind`), `Display` prints `dir(x, y, z)`, `DegenerateDirection` diagnostic ported; `NonFinite` pinned in Rust. **Intermediate** → unlocks β, γ, ε, η. Prereqs: none.
- **β — `dir(x, y, z)` / `dir(v)` builtin** (D11): `DATUM_CONSTRUCTOR_NAMES` row + `datum_constructor_result_type`, `builtin_arg_slots` rows for its own arguments, stdlib eval (port `direction.rs::eval_direction`, add the arity-3 form), `DimensionedArgRejected`, LSP completion row, `ri_literal` `dir(x, y, z)` serialiser arm, the one stdlib chunk line. Modules: reify-compiler, reify-stdlib, reify-lsp, reify-ir, reify-mcp. **Leaf.** Signal: B1–B3, B7. Prereqs: α.
- **γ — Named conversion and the operator table** (D2, D3, D12): `vector(d)` math builtin; C6 static arms in `infer_binop_type` / `math_fn_result_type` / the `expr.rs` guards; eval arms in reify-expr and reify-stdlib `linalg.rs` (port the branch arms, restricted to C6; `widen_direction` is deleted); unary negation; `normalize` identity; `angle` over Directions on the value path. Modules: reify-compiler, reify-expr, reify-stdlib. **Leaf.** Signal: B16, B18–B20, B25 (value half). Absorbs #8587. Prereqs: α.
- **σ — `Direction` slot kind for builtins** (D8): `ExpectedArg::Direction` in `builtin_signatures.rs`, its check arm, the `frame_at` slot rows (today arity/type-blind), coverage-test entries. Modules: reify-compiler. **Leaf.** Signal: B23. Prereqs: β. Unlocks θ-a, θ-b, ι-a/b/c.
- **δ — Datum-slot conformance for Direction/Axis/Plane/Frame at ctor and default** (D8): exact-identity rows in the `ArgVerdict` relation (`type_acceptance.rs`, post-#8739) reached from the general concrete-leaf arm; the `dir(x, y, z)` hint; the conformance "Still unexamined" list shrinks by four. Modules: reify-compiler. **Leaf.** Signal: B6 (type half), B8, B22. Absorbs #7958. Prereqs: #8739 (out of batch), β (the hint must name an existing builtin).
- **ε — Value-shape SPOT for datums** (C9): stdlib `make_axis` / `make_plane` / `eval_axis_through` / `eval_plane_through` / `frame_plane` and solver `transform_datum` mint `Value::Direction`; eval `decode_plane` / `decode_axis` / `datum_vec3` and stdlib `decode_plane` read via `decode_direction`; both pins inverted. Modules: reify-stdlib, reify-constraints, reify-eval, reify-expr, reify-compiler (test). **Leaf.** Signal: B4 (stdlib three), B17. Prereqs: α.

**Phase 2 — vertical slices** (each: retype → one reader → its corpus and fences → green)

- **ζ-a — Joints and flexures.** `kinematic.ri` ×5 and `ports_mechanical.ri` ×2 fields → `Direction`; joint fns `prismatic` / `revolute` / `cylindrical` / `planar` and the 13 `prb_*` flexure ctors (axis arg index per Appendix E §E: 5 for most, 6 for `prb_let_joint` / `prb_cartwheel_flexure`, `motion_axis` at 5 for the parallelogram pair) take `Direction` through σ's slot kind; `validate_axis` / `unit_axis_from_map` / supports / flexure validators read via `decode_direction` (the raw-then-normalise-at-use path and `joint_axis`'s raw return go away); corpus: 52 `prismatic(` + 21 `revolute(` + 3 `cylindrical(` + 2 `planar(` `.ri` calls across examples/tests/prj/gui/tree-sitter, the `prb_*` sites, and the embedded `.rs` strings (`prismatic(` ≤121 lines incl. 25 in `gui/src-tauri/src/tests/engine_tests.rs`, `revolute(` ≤56). Modules: reify-compiler stdlib, reify-stdlib, corpus. **Leaf.** Signal: B6 (value half): `prismatic(vec3(0,1,0), r)` is a check error; `prismatic(dir(0,1,0), r)` builds the same mechanism and `joint_axis` prints `dir(0, 1, 0)`. Prereqs: β, γ, δ, σ.
- **ζ-b — FEA loads.** `PointLoad.direction`, `Gravity.direction` → `Direction`; `extract_loads` via `decode_direction`; `read_direction_or_neg_z` deleted and its pin `extract_loads_malformed_direction_defaults_to_neg_z` inverted to `FeaValueShapeError::NotADirection`; the ~5 corpus sites + `fea_*` fixtures + chunk lines. Modules: reify-compiler stdlib, reify-eval, corpus. **Leaf.** Signal: B9. Prereqs: β, δ.
- **ζ-c — Modal.** `modal_analysis.ri` ×5 and `modal_analysis_fns.ri` ×2 → `Direction`; `extract_reference_direction` and transient `read_vec3` via `decode_direction` (the +Z / zero defaults deleted, pins inverted); corpus `reference_direction` ≤75 `.rs` + 16 `.ri` sites + chunk lines. Modules: reify-compiler stdlib, reify-eval, corpus. **Leaf.** Signal: `ModalOptions(reference_direction: vec3(..))` is a check error; with `dir(..)` the modal fixtures produce today's modes. Prereqs: β, δ.
- **ζ-d — FDM.** `fdm.ri` `build_direction` + its provenance literal → `Direction`; `field_vec3` via `decode_direction` (the +Z degenerate fallback deleted); `build_direction` ≤67 `.rs` lines. Modules: reify-compiler stdlib, reify-eval, corpus. **Leaf.** Signal: as-printed zones identical with `dir(0,0,1)`; the vector form rejected. Prereqs: β, δ.
- **η — Frames and `MassProperties.origin`** (D5, D6): delete `Frame3` and `MaterialFrame`; `LocatedPort.frame : Frame`; `AnisotropicMaterial` takes `Frame`; FEA material reader via `frame_basis` (FENCE test inverted, identity contract kept); as-printed minters emit `Value::Frame`; delete `MassProperties.origin` (dynamics.ri; `default_frame3` in reify-stdlib dynamics/eval.rs and `assemble_mass_properties` in reify-eval dynamics_ops.rs; the pins `mass_properties_origin_is_frame3_not_real`, the Frame3 origin/axes asserts, the dynamics_ops `origin: Real(0.0)` fixture; the 21 literals in `examples/dynamics/*` and `tests/prd-gate/fixtures/s6_idyn_*`); body-frame convention documented on `MassProperties`; `anisotropic_bar.ri` and `ports_breadth.ri` migrated to `frame_at` + `dir`. Modules: reify-compiler stdlib, reify-stdlib, reify-eval, reify-ir, corpus. **Leaf.** Signal: B5, B10, B11. Prereqs: α, β (the Frame-slot rejection B22 is δ's signal, not η's).
- **θ-a — Orientation builtins.** `orient_axis_angle`, `orient_look_at`, `orient_basis` take `Direction` through σ; `orient_to_axis_angle`'s eval `Value::Map` `axis` becomes a `Direction` (no dependency on #6004); corpus: all 28 `orient_axis_angle` sites (`prj/printer_v01/printer.ri` 16, `dev_capstan.ri` 2, examples/tests/fixtures 10) + 3 `orient_look_at` + embedded (`orient_axis_angle(` ≤34 `.rs`, 9 `.md` fences) + LSP rows. Modules: reify-compiler, reify-stdlib, reify-lsp, reify-mcp, corpus. **Leaf.** Signal: B14. Prereqs: β, γ, σ.
- **θ-b — Selectors and queries.** `faces_by_normal` / `edges_parallel_to` / `faces_perpendicular_to` / `edges_perpendicular_to` and build-path `angle(a, b)` read through `decode_direction` (one reader replaces `resolve_vec3_arg`); `normal(surface, at)` returns `Direction`; `offset_curve`'s direction arm; corpus: 30+25+2+2 `.ri` calls, `faces_by_normal(` ≤135 `.rs` + 62 `.md` fences, `edges_parallel_to(` ≤29+7; topology chunk lines. Modules: reify-compiler, reify-eval, reify-mcp, corpus. **Leaf.** Signal: B4 (`normal`), B15, B25 (build half). Absorbs #9256's selector half. Prereqs: β, γ, σ, ε.
- **ι-a — Transforms.** `rotate/3`, `rotate_around/4`, `mirror/3` with Direction slots; each = lowering arm (`geometry_transform.rs` / `geometry.rs`) + arity-guarded `builtin_arg_slots` rows + `LOWERING_ACCEPTED_ARITIES` row; scalar arities 5/8/7 removed with the arity error naming the Direction form; eval decoders via `decode_direction`; corpus (`rotate(` 29 `.ri`, ≤112 `.rs`, 39 `.md`; `mirror(` 3+5, ≤73, 57; `rotate_around(` 1, ≤20, 3) + geometry/stdlib/units chunk fences. Modules: reify-compiler, reify-eval, reify-mcp, corpus. **Leaf.** Signal: B12, B13 (these three). Prereqs: β, γ, ε, σ.
- **ι-b — Sweeps.** `revolve/4` (+ `revolve(profile, axis: Axis, angle)/3`), `revolve_full/3`, `extrude_infinite/2` + `extrude_infinite_symmetric/2` (D10; the String sense retired); eval-side zero Warning paths deleted (C10); same three-edit discipline; corpus (`revolve(` 2 `.ri`, ≤25 `.rs`; `extrude_infinite(` 2, ≤18) + fences. Modules: reify-compiler, reify-eval, reify-mcp, corpus. **Leaf.** Signal: B12′, B13 (`"both"`). Prereqs: β, γ, ε, σ.
- **ι-c — Patterns and primitives.** `linear_pattern/4`, `linear_pattern_2d/7`, `circular_pattern` (Axis form primary; 9-arg retired; Open Q4 decided here), `half_space/2` (the LSP triple-site row and its behavioural pins), `arc/5`; corpus (`linear_pattern_2d(` 16 `.ri`; `half_space(` 3, ≤62 `.rs`; `circular_pattern(` 5, ≤48; `arc/9` 1) + fences. Modules: reify-compiler, reify-eval, reify-lsp, reify-mcp, corpus. **Leaf.** Signal: B12″, B13 (`linear_pattern`). Absorbs #5351. Prereqs: β, γ, ε, σ.
- **κ₀ — Runtime variant construction** (D9 prerequisite): `compile_variant_construct` accepts non-literal payload field expressions by emitting a runtime constructor node paralleling `StructureInstanceCtor`; eval builds `Value::Enum { payload }`; the "non-constant payload value … is not yet supported" diagnostic is retired; `dynamics.ri`'s TAGGED-UNION NOTE blocker 1 is struck (blocker 2, payload reads outside `match`, stays). Modules: reify-compiler, reify-expr. **Leaf.** Signal: `enum E { A { v: Length }, B }` constructed as `A { v: some_param }` then `match ev { A { v } => v, B => 0mm }` prints `5 mm` where today `reify check` errors (payload `Display` is unchanged: `E::A`). Prereqs: none.
- **κ — `PressureLoad.direction` enum** (D9): `enum PressureDirection { Normal, Fixed { dir: Direction } }` in `fea_multi_case.ri`, default `PressureDirection.Normal`; the pressure-spec reader matches the payload; the two corpus sites and the chunk line migrated; `"normal"` String retired. Modules: reify-compiler stdlib, reify-eval, corpus. **Leaf.** Signal: B21. Prereqs: κ₀, β, δ.

**Phase 3 — docs-truth and gate**

- **λ — Docs-truth** (overlay rule; prose only — every slice already migrates its own compiled fences and signature lines same-diff): MCP chunk prose (`units.md`'s "trailing axis stays bare" rule rewritten; `topology.md`'s documentation-drift note retired); `docs/reify-stdlib-reference.md` (~30 signature lines, the "not implemented" vector-form notes retired, the undocumented `frame_at` / `axis_through` / `plane_through` / `midplane` / flexure ctors added); `docs/reify-language-spec.md` §4.9 known-limitation row removed and a `Direction` semantic paragraph added (unit, dimensionless, signed; `dir`, `vector`); `crates/reify-core/src/ty.rs` module doc gains the "unit ≠ dimensionless" rule beside the quantity-slot convention; `examples/best_practices/directions.ri` + `INDEX.md` line; `.claude/skills/reify-design/SKILL.md` index line; LSP rows for `plane_xy` / `axis_x` corrected; `docs/design/geometric-relations.md` §2 pointer; RBD PRD §4.1 note. **Leaf.** Signal: B24. Prereqs: β, γ, ε, ζ-a..d, η, θ-a/b, ι-a/b/c, κ.
- **μ — Integration gate (H)**: the §6 matrix as `tests/prd-gate/fixtures/direction_*.ri` plus Rust boundary tests facing producers (α/β/ε/σ) and consumers (ζ/η/θ/ι/κ); same-diff registrations per the overlay — `_RUST_COUPLED_RI_FIXTURES` for every fixture a Rust test reads (PG-DRIFT), nextest heavy/smoke partition entries where applicable, and no new wall-clock bound; `examples_smoke` and the ctor-conformance corpus survey green over the migrated corpus. **Leaf.** Signal: the matrix runs in the gate; every row's postcondition is a test. Prereqs: every Phase 1–2 leaf.
- **ν — PRD-close**: stamp `SHIPPED` with landed IDs, AS-AUTHORED freeze paragraph, LIVE/AS-AUTHORED map (LIVE: §5 contract, §7 table), matching header on the capability manifest. **Leaf.** Signal: the committed header. Prereqs: all.

Shared-file serialisation to expect: `builtin_signatures.rs` (ledger + slot table) is touched by σ and every ι-* leaf, and `units.rs` by β/θ-a; the edits are short rows, so lock serialisation is acceptable and not a defect.

**Companion corrections (filed at decompose, with Leo's say-so):** cancel #6090 in favour of α/β/γ/δ; cancel #8586 (moot); cancel-in-favour #8585 (ζ-b/c/d), #8587 (γ), #7958 (δ), #5351 (ι-c), #9256 (θ-b/ε); amend #6004's `AxisAngle.axis` to `Direction`; `add_dependency` #9568 ← β and #9571 ← δ.

G7 (reify INV-SF-1..6) walk: α/β satisfy `undef-has-provenance` (a degenerate direction is a coded Error, never bare undef) and `diagnostics-carry-codes`; ζ-b/c/d satisfy `placeholders-owned-and-loud` and the shared `no-silent-fail-soft` (the ±Z/zero defaults are removed, not re-homed); η satisfies `declared-intent-consumed-or-diagnosed` (a declared field nobody consumes is deleted); δ and σ close `error-severity-exits-nonzero` for datum slots. No waivers.

## 9. Out of scope

- The `let`-annotation hole (`let x : Vector3<Length> = "hello"` passes; the annotation does not retype; `let x : Direction = vec3(..)` likewise) and the Add/Sub operand-kind guard for vectors (`vd + vl` type-checks) — pre-existing, independent of directions; they belong to #9571's one-relation work.
- The `Vector3<Int>` literal-typing row (a user fn with a `Vector3<Dimensionless>` param rejects `vec3(0, 0, 1)`): #9568/#9571 territory; `dir(0, 0, 1)` sidesteps it for directions.
- GUI editing of a `Direction` parameter: β makes the serialiser emit `dir(x, y, z)`, but `PropertyEditor` is one text input per row and nothing parses a `dir(...)` typed into it; the frontend affordance is a GUI-surface task, not a direction-typing one.
- Retiring the IR's bare `[f64;3]` axes, the kernel Rust-arm guards and the C++ zero floors (C10 keeps them).
- A 2D direction type.
- `PointLoad.force : Real` remaining undimensioned, and whether a load should be one `Vector3<Force>` (`100N * dir(0,0,-1)` becomes spellable under C6) — an FEA-surface question.
- Payload reads outside `match` for data-carrying enums (dynamics.ri blocker 2).

## 10. Open questions (tactical; decide at the named leaf)

1. **Where `decode_direction` / `frame_basis` live.** reify-ir is the lowest crate every consumer already depends on; reify-core if a consumer needs it without `Value`. Decide at α.
2. **`magnitude(d)`**: error (as C6) or `Real` 1.0. Error keeps the table small; decide at γ.
3. **`circular_pattern` Direction form** beside the Axis form (`circular_pattern(g, origin, axis: Direction, count, angle)`): add or not. Decide at ι-c.
4. **`Frame` `Display` round-trip** (`frame_at(point3(..), dir(..), dir(..))`) — β does it for `Direction`; η may do it for `Frame`.
5. **Renormalise on `transform_datum`** every solver iteration (C2 does) vs. trust: measure cost at ε; expected negligible.
6. **`.rs` golden fixtures with a non-basis scalar axis** (B12's byte-identity premise beyond `.ri`): ι-a/b/c re-measure and, where a golden shifts by one normalisation's ulps, re-baseline with the reason recorded.

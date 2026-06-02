# PRD — `std.math.linalg` N-generality + compiler signatures

**Milestone:** v0.6 · **Authored:** 2026-06-02 · **Status:** Draft
**Closes:** gap-register cluster **P9 math-linalg-completion** (`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`, 5 gaps) + the `vecN` / `matrix()` construction substrate gap discovered during authoring.
**Source doc:** `docs/reify-stdlib-reference.md` §1 (`std.math`).

---

## 1. Consumer + user-observable surface

The whole cluster is a **stdlib + compiler type-inference** surface, not an in-engine kernel seam. It therefore does **not** plug into one of the 7 in-engine seams in `engine-integration-norm.md` §3 (those govern kernel/realization dispatch); its consumers are the user-facing compile/eval entry points:

- **`reify eval <file>`** — `determinant` / `inverse` / `eigenvalues` of an N>3 matrix return a **real value** instead of `undef`; `matrix(...)` / `vec(...)` / `diag(...)` / `identity(...)` evaluate to real `Tensor`/`Vector` values instead of `undef`; `complex_eigenvalues(...)` returns a `List<Complex<Q>>`.
- **`reify check <file>`** — every `std.math` call now carries its **documented dimensional return type** at compile time (e.g. `determinant(m4)` types as `Scalar<Q^4>`, `sqrt(area)` as `Scalar<Length>`), so a dimensionally-wrong *consumer* of a math result (`let x : Length = determinant(m4x4)`) is caught at compile time instead of silently typing as the first argument.
- **`examples/linalg.ri`** (CI) — the canonical example exercises a 4×4 `determinant`/`inverse`/symmetric-`eigenvalues` and a `complex_eigenvalues` case through `parse → compile_with_stdlib → eval`; runs green in CI.
- **`docs/reify-stdlib-reference.md`** — §1.1/§1.3 reconciled so the documented behavior matches the shipped API (the sqrt-"intrinsic" and pow-dimensioned-power claims).
- **Cross-PRD:** `affine-map-type.md` (task `#3961`) consumes the **shared `determinant` builtin name** (its `AffineMap → Real` overload) — see §6.

## 2. Sketch of approach

Five mechanisms, one substrate prerequisite:

1. **Construction substrate (prerequisite).** There is no matrix literal in `.ri` and only `vec2`/`vec3` exist (verified: `examples/linalg.ri` builds 3×3 via outer-product sums; `vec4`/`matrix` eval to `undef`). So an N>3 matrix **cannot be built from source today** — meaning the N-generality gaps would have *no possible user-observable `.ri` signal*, only synthetic Rust tests (the rejected fake-done signal, cluster C-07). Add the construction primitives first:
   - `vec(list)` — N-vector from a list literal `[a, b, …]` → `Vector<N>` (N = list length, known at compile time). `vec2`/`vec3` retained as sugar.
   - `matrix(rows)` — M×N matrix from a depth-2 nested list `[[…],[…]]` → `Tensor` rank-2. **Rank-2 only** (see §5 out-of-scope).
   - `diag(list)` → N×N diagonal `Tensor`; `identity(n)` → N×N identity `Tensor`.
   - All four are pure list→value reshaping (no LA dependency) plus their compiler signatures. **No grammar work** — nested list literals, `matrix(...)`, `vec4(...)`, `identity(...)` all parse today (verified against `target/debug/reify`; the tree-sitter CLI is stale per the project G3 note).

2. **N>3 `determinant` + `inverse`** — replace the hardcoded 1×1/2×2/3×3 closed forms in `crates/reify-stdlib/src/matrix.rs` (`_ => Value::Undef` at N≥4) with a general dense path via **nalgebra** (`DMatrix::determinant` / `try_inverse`). Dimension rules unchanged: `det → Q^N`, `inverse → Q^(-1)`. Singular → `Undef` (preserve existing 2×2/3×3 semantics).

3. **`eigenvalues` (real, any N) + `complex_eigenvalues` (general)** — in `matrix.rs`:
   - `eigenvalues(m) → List<Scalar<Q>>` for any N: real spectrum. nalgebra `symmetric_eigenvalues` for symmetric input; for general input, take nalgebra's `complex_eigenvalues` and project to real iff every imaginary part is ~0, else `Undef` (now **documented**, not silent). Fixes the degenerate-3×3 (repeated-root) case as a side effect.
   - `complex_eigenvalues(m) → List<Complex<Q>>` for any N: nalgebra `DMatrix::complex_eigenvalues` (real-input Schur). Closes the complex-discriminant-2×2 gap and non-symmetric N>3. Returns the v0.6 `Value::Complex` type. Naming follows the existing `complex_*` idiom (`complex_sqrt`/`complex_exp`/`complex_pow`/`complex_div`, tasks 3953/3954).

4. **Compiler signatures for all §1 `std.math` fns (B+H).** Today the compiler's `resolve_function_overload` `NoUserFunctions` arm (`crates/reify-compiler/src/expr.rs:1640-1657`) defaults a native call's result type to **the first argument's type**, so the documented dimensional return types are *not* compile-checked. Add a math-typed-fn family in that arm — **exactly mirroring the geometry-query family** (`is_geometry_query → geometry_query_result_type`, GHR-α / task 3603, `expr.rs:1603-1622`):
   - New `is_math_typed_fn(name)` predicate + `math_fn_result_type(name, &compiled_args)` in a new module `crates/reify-compiler/src/math_signatures.rs` (the frozen **contract**, §3).
   - It computes the dimensional return type from the *argument* types using the existing `DimensionVector` algebra (`reify-core/src/dimension.rs`: `mul`/`div`/`root`/`pow`). The substrate is sufficient — no value-model or type-system change.
   - The new family must be pinned **disjoint** from the geometry/dynamics name families (same convention as `units.rs::tests::{geometry,dynamics}_query_names_are_disjoint_from_other_families`).

5. **Doc reconcile** (`docs/reify-stdlib-reference.md`):
   - §1.1 `sqrt` — it is **not** a free-standing "compiler intrinsic"; its dimensional halving (`Q^(1/2)`) is now realized as a **compiler signature** (mechanism 4) + eval-time numeric halving (`dimension.root(2)`, `numeric.rs:30`). Reword.
   - §1.1 `pow` — the documented "integer literal exponents on dimensioned quantities works through repeated multiplication" belongs to the **`^` operator** (`reify-expr/src/lib.rs` `eval_pow`, delivered by tasks 3805/4106), **not** the `pow()` builtin (which is dimensionless-only, `numeric.rs:88`). Re-attribute.
   - §1.3 — `determinant`/`inverse` now any N; `eigenvalues` is real-spectrum (Undef on complex); add `complex_eigenvalues`; document `vec`/`matrix`/`diag`/`identity` constructors.

## 3. Contract — the math-signature table (B+H)

`math_fn_result_type(name, args)` is a **frozen table**. `Q`, `Q1`, `Q2` denote the `DimensionVector` of the corresponding argument's quantity slot; `N` is the matrix/vector dimension read from `Type::Tensor{n}` / `Type::Vector{n}`. Where an arg is `Type::Real` it is dimensionless `Q = DIMENSIONLESS`.

| fn | arg type(s) | result type | dimension rule |
|---|---|---|---|
| `sqrt` | `Scalar<Q>` / `Real` | `Scalar<Q^(1/2)>` | `Q.root(2)` |
| `abs` | `Scalar<Q>` / `Complex<Q>` | `Scalar<Q>` | identity |
| `min`/`max`/`clamp` | `Scalar<Q>` … | `Scalar<Q>` | identity (first arg) |
| `lerp` | `Scalar<Q>,Scalar<Q>,Real` | `Scalar<Q>` | identity |
| `sign` | `Scalar<Q>` | `Real` | dimensionless |
| `dot` | `Vector<N,Q1>,Vector<N,Q2>` | `Scalar<Q1·Q2>` | `Q1.mul(Q2)` |
| `cross` | `Vector<3,Q1>,Vector<3,Q2>` | `Vector<3,Q1·Q2>` | `Q1.mul(Q2)` |
| `normalize` | `Vector<N,Q>` | `Vector<N,Dimensionless>` | dimensionless |
| `magnitude` | `Vector<N,Q>` | `Scalar<Q>` | identity |
| `determinant` | `Tensor<2,N,Q>` (Matrix) | `Scalar<Q^N>` | `Q.pow(N)` |
| `determinant` | `AffineMap` | `Real` | — (cross-PRD #3961 arm, §6) |
| `inverse` | `Tensor<2,N,Q>` | `Tensor<2,N,Q^(-1)>` | `DIMENSIONLESS.div(Q)` |
| `transpose` | `Tensor<2,M×N,Q>` | `Tensor<2,N×M,Q>` | identity |
| `outer` | `Vector<N,Q1>,Vector<M,Q2>` | `Tensor<2,…,Q1·Q2>` | `Q1.mul(Q2)` |
| `trace` | `Tensor<2,N,Q>` | `Scalar<Q>` | identity |
| `eigenvalues` | `Tensor<2,N,Q>` | `List<Scalar<Q>>` | identity, listed |
| `complex_eigenvalues` | `Tensor<2,N,Q>` | `List<Complex<Q>>` | identity, listed |
| `vec` | `List<Scalar<Q>>` | `Vector<N,Q>` | N = list length |
| `matrix` | `List<List<Scalar<Q>>>` | `Tensor<2,M×N,Q>` | identity |
| `diag` | `List<Scalar<Q>>` | `Tensor<2,N,Q>` | identity |
| `identity` | `Int` | `Tensor<2,N,Dimensionless>` | dimensionless |
| `complex` | `Scalar<Q>,Scalar<Q>` | `Complex<Q>` | identity |
| `real`/`imag` | `Complex<Q>` | `Scalar<Q>` | identity |
| `conjugate` | `Complex<Q>` | `Complex<Q>` | identity |
| `complex_magnitude` | `Complex<Q>` | `Scalar<Q>` | identity |
| `phase`/`arg` | `Complex<Q>` | `Angle` | fixed |

(Trig fns §1.2 already have correct return types via their own paths; included only if a probe shows first-arg-default drift. `pow`/`log`/`log10`/`exp`/`remap`/`floor`/`ceil`/`round`/`mod` are dimensionless/`Int` and the first-arg default is already correct — no signature needed, but `pow → Real` is pinned to prevent the dimensioned-arg misread.)

**Two-way boundary test (the H component).** For every row, a test asserts **compile-time type ⟺ eval-time value dimension agree**, in both directions, for a representative input:
- *forward*: `reify check` types the cell as the table says;
- *backward*: `reify eval` produces a `Value` whose dimension equals the cell type's dimension (e.g. `sqrt(4.0m^2)` cell-type `Scalar<Length>` ⟺ eval `2.0 m`; `determinant` of a 4×4 `Scalar<Q^4>` ⟺ eval dimension `Q^4`).

This is the contract that prevents the compile/eval drift the gap describes — and it is why this PRD is **B+H** (§5 G5).

## 4. Pre-conditions for activating

- `target/debug/reify` builds (substrate probes above were run against it).
- nalgebra added to `crates/reify-stdlib/Cargo.toml` (introduced by task β, the first numeric consumer; faer **unchanged** — it remains the FEA sparse backend).
- No grammar prerequisite — all syntax parses today (G3 verified, §5).

## 5. Resolved design decisions

- **LA dependency = nalgebra** (Q1). reify-stdlib gains a dense-LA dep; nalgebra's `determinant`/`try_inverse`/`symmetric_eigenvalues`/`complex_eigenvalues` are direct and battle-tested, avoiding a hand-rolled QR/Hessenberg eigensolver (a G6 conditioning hazard). faer stays the FEA *sparse* solver — complementary, no migration. (User: "pull in whatever best supports the features; faer and nalgebra both acceptable.")
- **eigenvalues = real-`eigenvalues` + general-`complex_eigenvalues`** (Q2). Two functions with fixed compiler signatures rather than one runtime-polymorphic return. `eigenvalues → List<Scalar<Q>>` (real, any N, Undef-if-complex-documented); `complex_eigenvalues → List<Complex<Q>>` (general). Gives the engineering-95% case a `Scalar` result while making the complex capability reachable; closes the complex-2×2 gap.
- **Construction = `vec` + `matrix` + `diag` + `identity`, rank-2** (Q3). Thorough on dimension (any M×N, any N-vector) but **capped at rank-2** for `matrix`: arbitrary-rank tensor literals (depth ≥3) would be an **orphan producer** — no stdlib op consumes rank-3+ tensors (det/inverse/eigenvalues are rank-2-square only), tripping G1. Deferred until a consumer exists.
- **Signature enforcement is permissive** — the signature *propagates the correct dimensional type*; it does **not** add new bespoke hard errors. Downstream dimensional-mismatch checking (already present on binary `+`/`-`, assignment-to-typed-let) does the rejecting. This matches the project's "reject only statically-known mismatch" back-compat posture. The only new diagnostics are those that *already* fire when a now-correctly-typed result meets an incompatible consumer.
- **G3 grammar = N/A.** `matrix([[…]])`, `vec([…])`, `vec4(…)`, `identity(4)`, nested list literals, and all §1 fn-call forms parse against `target/debug/reify` (the tree-sitter CLI is stale and rejects even known-good list literals). The work is purely semantic.

## 6. Cross-PRD relationship + seam ownership

| Seam | This PRD owns | Other PRD owns | Resolution |
|---|---|---|---|
| **`determinant` builtin name** | Matrix/`Tensor<2,N,Q> → Scalar<Q^N>` eval arm + the **shared compiler signature** (`math_fn_result_type` includes the `AffineMap → Real` row proactively) | `affine-map-type.md` task **#3961** owns the `AffineMap → Real` **eval arm** (`det(a.linear)`) | No blocking edge. #3961 already says "if `determinant` exists, add an AffineMap arm." This PRD's signature table reserves the AffineMap row so #3961 slots its eval arm in regardless of land order. |
| **`complex_eigenvalues` → `Value::Complex`** | the eigensolve + signature | `complex-literals-and-stdmath.md` (landed: `Value::Complex`, tasks 3950–3955) | Consumer-of-landed-substrate. `Value::Complex` + dimensioned `complex(re,im)` already ship; this PRD only produces them. |

No new contested-ownership pair introduced (none of the three `phase-3-breadcrumb-map.md` §3 seams are touched).

## 7. Decomposition plan (one bullet = one leaf, with its user-observable signal)

Chain: **α → β → γ → δ → {ε, ζ}**. Serialized where leaves share a file (`matrix.rs` in β/γ; `expr.rs` in α/δ; `lib.rs` dispatch) to avoid narrow-file-lock contention; ε (docs) ∥ ζ (example) after δ.

- **α — construction primitives.** Add `vec`/`matrix`/`diag`/`identity` builtins (eval: list→`Vector`/`Tensor`) in `reify-stdlib` + their compiler signatures (table §3 rows) in `expr.rs`/`math_signatures.rs`. *Signal:* `reify eval` of a `.ri` with `matrix([[1,2],[3,4]])`, `vec([1,2,3,4])`, `diag([3,5,7])`, `identity(4)` prints the expected `Tensor`/`Vector` values (today all `undef`); `reify check` types them as rank-2 `Tensor` / `Vector<N>`. *Consumer:* β, γ, ζ.
- **β — N>3 `determinant` + `inverse`.** Add nalgebra to `reify-stdlib/Cargo.toml`; replace the `_ => Undef` N≥4 arms in `matrix.rs` with `DMatrix` det/inverse; preserve dim rules + singular→Undef. *Signal:* `reify eval` of `determinant(matrix([[…4×4…]]))` = the hand-computed value (today `undef`); `inverse(m4)` then `m4·inv ≈ I₄` within 1e-9. *Consumer:* ζ, AffineMap #3961, user. *Dep:* α.
- **γ — `eigenvalues` (real, any N) + `complex_eigenvalues` (general).** `matrix.rs`: nalgebra symmetric/real path for `eigenvalues` (Undef if any eigenvalue non-real), `complex_eigenvalues` via real-Schur → `List<Complex<Q>>`; signatures for both. *Signal:* `reify eval` of `eigenvalues(diag([3,5,7,9]))` = `[3,5,7,9]` (4×4, today `undef`); `complex_eigenvalues(matrix([[0,-1],[1,0]]))` = `[i, -i]` (2D rotation; closes the complex-2×2 gap). *Consumer:* ζ, user. *Dep:* β (nalgebra present + shared `matrix.rs`).
- **δ — compiler signatures for the pre-existing §1 fns (B+H).** `math_signatures.rs` frozen table (§3) + `is_math_typed_fn` arm in `expr.rs` (mirrors GHR-α geometry-query arm) + the two-way boundary-test suite + name-family disjointness test. *Signal:* `reify check` types `sqrt(4.0m^2)` as `Scalar<Length>`, `dot(vec([1m,2m]),vec([3m,4m]))` as `Scalar<Area>`, `determinant(m4)` as `Scalar<Q^4>`; a `let x : Length = determinant(m4)` now emits a dimension-mismatch diagnostic (today silently accepted). *Consumer:* `reify check` users, ε, spec-shape constraint typing. *Dep:* α, β, γ (so boundary tests cover the N-general rows on real eval values).
- **ε — doc reconcile.** `docs/reify-stdlib-reference.md` §1.1 (sqrt-signature wording; pow→`^` re-attribution), §1.3 (any-N det/inverse; real-`eigenvalues` + `complex_eigenvalues`; `vec`/`matrix`/`diag`/`identity`). *Signal:* §1.1/§1.3 text matches the shipped API; the imperative bullets in §1.1 prose name `^`/the signature mechanism, not "intrinsic"/"pow". *Consumer:* doc + GUI-assistant chunk readers. *Dep:* α, β, γ, δ.
- **ζ — integration example + CI gate (Type-C).** Extend `examples/linalg.ri` (and its `compile_with_stdlib + eval` test) with a 4×4 built via `matrix(...)`, its `determinant`/`inverse`/symmetric-`eigenvalues`, and one `complex_eigenvalues` rotation case. *Signal:* the example parses (exit 0, no ERROR) and evaluates with no `undef` in the asserted cells, in CI. *Consumer:* CI + users (canonical example). *Dep:* α, β, γ, δ.

## 8. Out of scope

- **Arbitrary-rank tensor literals** (depth ≥3 `matrix`/`tensor`) — orphan producer, deferred (§5).
- **Complex/real-exponent powers, complex trig/log** — already deferred by `complex-literals-and-stdmath.md` §8.
- **`pow()` becoming dimensioned** — the dimensioned integer-power lives in `^`; `pow()` stays dimensionless (doc reconcile only, ε).
- **Sparse / very-large matrices** — `matrix(...)` is hand-authored dense; FEA sparse stays on faer in its own crates.
- **Singular-matrix diagnostics** beyond the existing `Undef` (no new `E_*` code).

## 9. Open (tactical) questions

- Should `eigenvalues` on a general matrix with a *real* spectrum return those reals (project from `complex_eigenvalues`), or require symmetry? PRD picks **project-if-real** (more permissive); implementer confirms nalgebra's `complex_eigenvalues` imaginary-part tolerance (suggest `|im| ≤ 1e-9·max|λ|`).
- `matrix(rows)` with ragged rows (unequal lengths) → `Undef` vs diagnostic. Default `Undef` (matches existing `matrix_components_f64` shape-guards); revisit if a clearer signal is wanted.
- Whether `identity(n)`'s `n` must be a literal `Int` for the compile-time `Tensor<2,n>` type, or may be a determined param. Default: literal for the typed form; non-literal degrades to a generic rank-2 `Tensor` cell type.

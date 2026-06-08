# PRD: real-dimensionless-unification

**Milestone:** v0_6
**Status:** deferred → queue now (priority-override landing; see §Landing)
**Type:** contract resolving an accreted day-one inconsistency (spec said alias, M1 skeleton implemented two types)
**Date:** 2026-06-08
**Approach:** B + H (load-bearing seams: type-checker + grammar-resolution; blast radius ~10 crates)

## Goal

`Real` and `Dimensionless` (and `Scalar<Dimensionless>`) become genuinely **one type**, as the spec has always claimed (`reify-language-spec.md:226,330` "they are the same type"). After this PRD:

- A user can mix `Real` and `Dimensionless` spellings freely — `let c : Real = ratio` where `ratio : Dimensionless` compiles; `active_turns + dead_total` (one a plain number, one a `Length/Length` ratio) evaluates instead of silently becoming `undef`.
- `reify eval` on `lead * (active_turns + dead_total)` produces the correct `Length` (the bug that started this).
- `Vector3<Real>` is accepted (today it errors while `Vector3<Dimensionless>` works).
- Bare `Scalar` as a type is a **hard error** (`E_BARE_SCALAR`) — it no longer silently means `Length`.
- `param t : Length = 1.0` is a **hard error** (`E_FN_PARAM_DEFAULT_TYPE_MISMATCH` reused) instead of silently binding a dimensionless value under a `Length` annotation.

## Background

Investigation (memory `decisions_real_dimensionless_unification`) established the root cause and the decision:

- `Type::Real` and `Type::Scalar { dimension }` were born in the **same commit** (`80d46ea639`, M1 skeleton) — the spec's type-table row `Real | Alias for Scalar<Dimensionless>` was transcribed as a sibling enum variant and the alias-ness was never implemented. There is **no load-bearing reason** for the variant (perf: `DimensionVector` = `[Rational;10]` = 40 bytes `Copy`, and an enum's size is its largest variant so the unit `Real` saves nothing; the only force keeping it alive is Rust match ergonomics).
- The duality manifests as: `eval_add`/`eval_sub` lack a `(Real, Scalar{DIMENSIONLESS})` arm → silent `Value::Undef` (`eval_mul`/`eval_div` *do* bridge — hence the working distributed-form workaround); the type checker collapses `Div`/`Pow` dimensionless→`Type::Real` but `Mul` leaves `Type::Scalar{DIMENSIONLESS}`; `type_compatible` doesn't unify the two; bare `Scalar` resolves to `Length` while `Display` prints a dimensionless scalar as `"Scalar"` (a round-trip lie).

**Decision (Leo):** asymmetric canonicalization — one canonical form per layer, chosen for what each layer is good at:

| Layer | Canonical dimensionless form | The other form |
|---|---|---|
| **Type** | `Type::Scalar { dimension: DIMENSIONLESS }` | `Type::Real` — **deleted** |
| **Value** | `Value::Real(f64)` | `Value::Scalar { dimension: DIMENSIONLESS }` — **must never be constructed** |

These are deliberately **opposite** and that is correct: `Type::Real` is the redundant variant at the type layer (delete it); `Value::Real` is the cheap, ergonomic representation at the value layer (keep it, ban the fat `Scalar` form). The bridge: a dimensionless literal `3.14` compiles to a `Value::Real` value carrying static type `Scalar{DIMENSIONLESS}` (expr.rs:719, the canonical bridge site). **An implementer must not "symmetrize" these** — that is the single most likely way to get this wrong.

## Sketch of approach

Two chokepoints + a resolution cleanup + a corpus migration + the adjacent param-default hole + docs.

1. **Value-layer chokepoint already exists**: `Value::from_real_scalar(value, dim)` (reify-ir/value.rs:1045) returns `Value::Real` when `dim.is_dimensionless()`. The arithmetic operators just bypass it. Route them through it; delete the now-dead `is_dimensionless()` consumer guards.
2. **Type-layer chokepoint is resolution + a deleted variant**: remove `Type::Real`; every construction/match becomes `Type::Scalar { dimension: DIMENSIONLESS }` (helper `Type::dimensionless_scalar()` exists) unless a more specific dimension is genuinely correct; reconcile `Mul`/`Div`/`Pow` type inference (now trivially the same variant); `Display` prints dimensionless `Scalar` as `"Real"`.
3. **Grammar-resolution**: `Real` keyword → `Scalar{DIMENSIONLESS}`; accept `Real` in **dimension position** as a synonym for `Dimensionless` (fixes `Vector3<Real>`); **remove** bare `Scalar` → `E_BARE_SCALAR`.

No novel **grammar** is introduced — `Real`, `Dimensionless`, `Scalar<Q>`, `Vector3<Q>` all parse today (`Tensor<2,3,Real>` already parses). Bare-`Scalar` removal is a *resolution-level rejection* of an already-parsing form, not a grammar change. New diagnostic `E_BARE_SCALAR` appends to `DiagnosticCode` (enum at diagnostics.rs:156); `ε` reuses the existing `FnParamDefaultTypeMismatch` (diagnostics.rs:344). **G3: no grammar work — `grammar_confirmed = true` for all tasks.**

## Resolved design decisions

1. **Asymmetric canonical forms** (table above). Non-negotiable; the whole design hinges on it.
2. **Bare `Scalar` removed entirely as a hard error** — not deprecated. (Leo: "ambiguous as a type, must not mean Length"; no external users yet, so breaking is correct now.) Corpus migrated to `Scalar<Length>`/`Length` *before* the error lands (behavior-preserving, since bare `Scalar == Length` today), so the workspace is green throughout.
3. **`Real` accepted in dimension position** as a synonym for `Dimensionless` (rather than forbidding `Real` in `<Q>` slots). Keeps both spellings valid everywhere; style guide prefers `Dimensionless` for physical ratios, `Real` for plain numbers — connotation only, same type.
4. **`Display`: dimensionless → `"Real"`** (dimensioned `Scalar` still prints its canonical name e.g. `Length`). Diagnostics become self-consistent (what's printed is writable back with the same meaning).
5. **Struct-param default type-check = hard error**, mirroring fn-param strictness (`fn_param_default_compatible` exact-equality). Reuse `FnParamDefaultTypeMismatch`.
6. **Value::Real is NOT deleted** — it is the canonical value-layer dimensionless representation.

## Pre-conditions for activating

None external. All substrate exists (`from_real_scalar`, `dimensionless_scalar()`, `DiagnosticCode` enum, `FnParamDefaultTypeMismatch`, `is_representable_cell_type` already admits `Scalar`). Queue now.

## Landing (priority-override, lock-aware ordering)

The batch is priority-boosted to shrink the conflict window against concurrent v0_6 batches that edit the same files (expr.rs, type_resolution.rs, the ~900 `.rs` test fixtures). The DAG is ordered to keep the workspace green at every step and to **serialize** the test-file-heavy edits rather than run them concurrently (file-lock thrash avoidance):

`δ → {α, γ}`, `α → {β, ε, γ, η}`, `γ → {ζ, η}`, `β → η`.

`δ` (behavior-preserving corpus migration) goes first so the giant `α` and the `E_BARE_SCALAR`-adding `γ` both rebase onto an already-clean corpus, and so the workspace never has a window where the hard error has live violators.

## Cross-PRD relationship

No contested-ownership seam introduced (checked against overlay's known pairs). This is a foundational type-system change every other PRD's tasks depend on implicitly; the interaction is **file-lock coordination**, not ownership contest.

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| concurrent v0_6 batches (engine-build-dag, kinematic-*) | this produces | shared edits to `expr.rs` / `type_resolution.rs` / test fixtures | this-prd (priority-override) | queued |
| `reify check` / `reify eval` CLI | consumes | diagnostics + eval results | this-prd | queued (η) |
| stdlib `.ri` + `examples/` corpus | consumes | compiles under new resolution | this-prd | queued (δ, η) |
| GUI / LSP | consumes | inherit via shared `reify-compiler` | n/a (transparent) | — |

## Decomposition plan

Labels are PRD-local; task IDs assigned at decompose.

- **δ — Migrate corpus off bare `Scalar` (behavior-preserving).** Modules: `examples/*.ri`, `crates/reify-compiler/stdlib/*.ri`, inline `.ri` fixtures across `crates/**/*.rs` (~31 example sites + ~hundreds of fixture sites). Replace bare `: Scalar` annotations with `Scalar<Length>` / `Length` (bare `Scalar == Length` today, so semantics are unchanged). *Signal:* `cargo test --workspace` + `reify check examples/*.ri` stay green; `grep ': Scalar[^<a-zA-Z]'` over the corpus returns zero. *Prereqs:* none. (Intermediate; unlocks α, γ.) `grammar_confirmed=true`.

- **α — Delete `Type::Real`; canonicalize type layer to `Scalar{DIMENSIONLESS}`.** Modules: `reify-core` (ty.rs variant removal + `Display`), `reify-compiler` (~641 sites: expr.rs literal-typing:719, type_compat.rs `infer_binop_type` Mul/Div/Pow consistency + `type_compatible` Int-widening target, math_signatures.rs quantity-slot defaults, fallback defaults), `reify-expr`/`reify-eval`/`reify-stdlib`/`reify-runtime`/`reify-ir`/`reify-constraints`/`reify-kernel-openvdb`/`reify-test-support` (mechanical `Type::Real` → `dimensionless_scalar()` except where a specific dimension is correct). Audit `is_representable_cell_type` admits `Scalar{DIMENSIONLESS}` (it admits `Scalar` — confirm) and that no site maps a `Scalar{DL}` cell-type to an expected non-`Real` value variant. *Signal:* workspace compiles with the variant gone; a file mixing `param a:Real` + `param b:Dimensionless` with `a+b` compiles clean under `reify check`; a genuine dimension-mismatch diagnostic prints `"Real"` not `"Scalar"`. *Prereqs:* δ. (Intermediate.) `grammar_confirmed=true`.

- **β — Route arithmetic through the value-layer chokepoint; kill dead guards.** Modules: `reify-expr/lib.rs` (`eval_mul`/`eval_div`/`eval_pow`/`eval_add`/`eval_sub` → `from_real_scalar`; delete the `is_dimensionless()` arms in `eval_eq`/`eval_cmp`), `reify-eval` (kernel-reply wrappers in geometry_ops.rs + tolerance/modal producers that build dimensionless `Scalar` → route through chokepoint), `reify-stdlib`/`reify-eval` consumer guards (`fea.rs:190-193,466`, `elastic_static.rs:1303` → match `Value::Real` directly). Add leak-guard test: no arithmetic produces `Value::Scalar{DIMENSIONLESS}`. *Signal:* the `groove_len` repro (`lead * (active_turns + dead_total)`) evaluates to the correct `Length` via `reify eval`; leak-guard test asserts the invariant. *Prereqs:* α. (Intermediate; unlocks η.) `grammar_confirmed=true`.

- **γ — Grammar-resolution: unify spellings, remove bare `Scalar`.** Modules: `reify-compiler/type_resolution.rs` — `"Real"` → `dimensionless_scalar()` (line 573); accept `Real` in dimension position (`resolve_type_alias_expr_to_dimension` + `resolve_dimension_type`) as `Dimensionless` synonym; **remove** `"Scalar" => Type::length()` (line 562) → emit `E_BARE_SCALAR` ("write `Scalar<Q>` or a named dimension like `Length`"). New `DiagnosticCode::BareScalarType`. *Signal:* `reify check` — `Vector3<Real>` compiles; `param x : Scalar` emits `E_BARE_SCALAR`; `Real`↔`Dimensionless` interop compiles. *Prereqs:* α (variant gone), δ (corpus clean → zero violators of the new error). (Intermediate; unlocks ζ, η.) `grammar_confirmed=true`.

- **ε — Type-check struct-param defaults against annotation (hard error).** Modules: `reify-compiler` (structure-param compile path; reuse `fn_param_default_compatible` strict-equality + `FnParamDefaultTypeMismatch`). *Signal:* `param t : Length = 1.0` emits `E_FN_PARAM_DEFAULT_TYPE_MISMATCH` via `reify check` (today silently accepted → downstream `undef`). *Prereqs:* α (unified types so the comparison is on canonical forms). (Leaf; logically severable from the unification but thematically tied.) `grammar_confirmed=true`.

- **ζ — Doc + style reconcile.** Modules: `docs/reify-language-spec.md` (§3.1/§3.3.1 alias language made literally true; remove bare `Scalar` from the grammar/type sections; `sin/cos/tan -> Real`), `docs/reify-stdlib-reference.md` (§1.2 trig returns; the two-category style rule: `Real` for plain numbers, `Dimensionless` for physical ratios), `docs/architecture-audit/gap-register.md` (note the duality closed). *Signal:* spec/stdlib-reference no longer claim bare `Scalar` or contradict the alias; rendered docs updated. *Prereqs:* γ (final surface decided). (Leaf — doc-reconcile.) `grammar_confirmed=true`.

- **η — Integration-gate: committed example exercising the unified surface in CI.** Modules: `examples/dimensionless_unification.ri` + an eval test (CI). The example mixes `Real`/`Dimensionless` params, computes a `groove_len`-style `length * (ratio_a + ratio_b)`, and uses `Vector3<Real>`. The test asserts the computed `Length` value (exact: e.g. `lead=2mm`, ratios `3.0 + 1.5` → `groove_len = 9mm = 0.009 m`), asserts `Vector3<Real>` type-checks, and asserts both invariants (compile-time: `Type::Real` no longer exists; runtime: leak-guard from β). This is the B+H integration-gate leaf pointing at the boundary-test sketch. *Signal:* `examples/dimensionless_unification.ri` runs in CI and the eval test passes. *Prereqs:* α, β, γ. (Leaf.) `grammar_confirmed=true`.

## Out of scope

- **Tightening stdlib `: Real` placeholders to specific dimensions** (the #3090 audit's "tightenable-now" set, ~22 sites). This PRD makes `Real ≡ Scalar<Dimensionless>`; it does **not** tighten genuinely-dimensionless-or-not placeholders. Owned by the existing #3090 follow-up tasks.
- **Deleting `Value::Real`** — explicitly kept as the canonical value-layer form.
- **`Field<X,Y>` param-position parsing** (#3117) — unrelated.

## Open questions (tactical)

1. **ε diagnostic code:** reuse `FnParamDefaultTypeMismatch` vs mint `StructParamDefaultTypeMismatch`. Suggested: reuse (semantics identical). Decide in ε.
2. **`Real`-in-dimension-position scope:** accept only at top-level `<Q>` slots, or also inside dimensional-op expressions (`Scalar<Real * Length>`)? Suggested: accept everywhere `Dimensionless` is accepted (uniform). Decide in γ.
3. **δ fixture migration:** `Scalar<Length>` vs bare `Length` as the replacement spelling. Suggested: `Length` (shorter, idiomatic) where the value is a length; `Scalar<Dimensionless>`→`Real` is N/A (δ only touches bare `Scalar`). Decide in δ.

## Contract (B+H)

**Invariant T (type layer):** after type resolution, `Type::Real` does not exist as a representable type anywhere in the compiler. The canonical dimensionless type is `Type::Scalar { dimension: DimensionVector::DIMENSIONLESS }`. Enforced structurally — the variant is deleted, so `rustc` is the guard.

**Invariant V (value layer):** no code path constructs `Value::Scalar { dimension }` where `dimension.is_dimensionless()`. The canonical dimensionless value is `Value::Real(f64)`. Enforced by routing all producers through `Value::from_real_scalar` + a leak-guard test.

**Bridge:** a value of static type `Scalar{DIMENSIONLESS}` is held at runtime as `Value::Real`. `is_representable_cell_type(Scalar{DIMENSIONLESS})` is true; cell read/write paths accept `Value::Real` for a `Scalar{DIMENSIONLESS}`-typed cell.

**Resolution contract:** `Real` and `Dimensionless` resolve to the same `Type` in both type position and dimension position. Bare `Scalar` resolves to no type (emits `E_BARE_SCALAR`).

## Boundary-test sketch (B+H)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Mixed-spelling addition | `param a:Real`, `param b:Dimensionless`, `let c = a + b` | compiles; `c` evaluates to `Value::Real(a+b)`; type of `c` is `Scalar{DIMENSIONLESS}` |
| Ratio-in-length (the root bug) | `lead:Length`, `active_turns`/`dead_total` dimensionless (one via `L/L` division) | `lead*(active_turns+dead_total)` evaluates to correct `Length`, not `undef` |
| Mul-cancels-to-dimensionless | `a:Scalar<1/Length>`, `b:Length`, `let r = a*b` | `r` type `Scalar{DIMENSIONLESS}`; `r` value is `Value::Real` (not `Value::Scalar`) |
| `Vector3<Real>` accepted | `param v : Vector3<Real> = vec3(1,0,0)` | type-checks; same type as `Vector3<Dimensionless>` |
| Bare `Scalar` rejected | `param x : Scalar = 5mm` | `E_BARE_SCALAR` diagnostic |
| Struct-param default mismatch | `param t : Length = 1.0` | `E_FN_PARAM_DEFAULT_TYPE_MISMATCH` |
| Diagnostic prints `Real` | a genuine dimension mismatch involving a dimensionless operand | message text contains `"Real"`, never `"Scalar"` for the dimensionless side |
| Leak guard (invariant V) | run arithmetic op suite | no result is `Value::Scalar{dimension.is_dimensionless()}` |
| Variant-gone guard (invariant T) | compile workspace | `Type::Real` is not a constructible variant (compile-time) |

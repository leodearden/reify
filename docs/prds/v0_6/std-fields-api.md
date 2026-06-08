# std.fields §11 — Field constructor & operator surface

**Status:** authored 2026-06-02 · **Milestone:** v0_6 · **Approach:** B + H (contract + two-way boundary tests)
**Closes:** gap-register `P16 fields-api` (7 gaps, 3 high) + the two §10-11 doc-reconcile rows for `InterpolationMethod` and callable `compose`.
**Source survey:** `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` · raw evidence `.orchestrator-scratch/stdlib-gaps-extract.json` · doc `docs/reify-stdlib-reference.md` §11.

---

## §0 — Premise correction (read first)

The gap register frames §11 as "field constructors don't exist." The substrate audit (this session) found the opposite: the **field machinery is ~90% built**; the gap is purely the **callable language surface**. Concretely, already on main:

- `Value::Field { domain_type, codomain_type, source: FieldSourceKind, lambda }` with **12** source kinds (`reify-ir/src/value.rs:859-873`, `FieldSourceKind` at `:317`).
- `sample(field, at)` intercepting builtin dispatching on source kind (`reify-expr/src/lib.rs:196-343`).
- Differential operators `gradient`/`divergence`/`curl`/`laplacian` as intercepting builtins (`reify-expr/src/lib.rs:344-353`, impl `calculus.rs`), packaged by done task **4025**.
- Gridded interpolation machinery + `SampledField` + internal `InterpolationKind {Linear,NearestNeighbor,Cubic,Rbf*,Kriging*}` (`reify-expr/src/interp.rs`, `reify-ir/src/value.rs:926`; *Rbf/Kriging deferred post-v0.1), done tasks **2338/2341**. Built by `build_sampled_field` (`reify-eval/src/engine_eval.rs:971+`).
- `composed{ … }` field-source block (done task **2343**).
- `Field<D,C>` parses & resolves in param/return position (done task **3088**; the stale `#3117` memory note is obsolete — `parametric_field_resolution_tests.rs` is green).
- Reify-language `enum` declaration + qualified `EnumName.Variant` resolution (`reify-compiler/src/expr.rs:2692-2702`; exemplar `geometry_traits.ri` `EulerConvention`).
- Scalar `clamp`/`remap`/`min`/`max` builtins (`reify-stdlib/src/numeric.rs:46-219`).
- Lambda values with capture (`Value::Lambda{…,captures}`; free-var analysis `reify-compiler/src/expr.rs:2991-3000`; fn params registered in scope before body compile `functions.rs:155-161`).

**This PRD adds only the missing callable surface, on top of that substrate.** It does **not** re-implement field machinery.

---

## §1 — Consumer & user-observable surface (G1)

**Mechanism introduced:** the §11 callable field vocabulary — three constructors (`constant_field`, `fn_field`, `from_samples`), the spatial transforms (`clamp_field`, `remap_field`, `threshold`, `restrict`), the callable `compose(f,g)`, and a Reify-language `InterpolationMethod` enum.

**Named consumers (no orphans):**
1. **User surface / CI** — stdlib `.ri` examples under `examples/fields/` that construct a field and `sample()` it, run by `reify eval` / `reify check` in CI. This is the direct, in-batch G2 signal for every leaf (precedent: `examples/fields/composed_stiffness.ri`, `examples/m11_field_calculus.ri`).
2. **`std.analysis` stress fields** — `von_mises`/`principal_stresses`/`safety_factor` already consume `Field` values (`reify-expr/src/analysis.rs`); the constructors give users a way to *build* the fields those operators consume.
3. **Downstream `P10 structural-traits`** (future PRD) — `Flexible.stiffness_model : Field<Point3<Length>, Tensor<2,3,Pressure>>` and heterogeneous-material FEA (`solve_elastic_static` `material: Field<Point3, AnisotropicMaterial>`, done task 3780) want a user-facing field constructor; `constant_field`/`fn_field`/`from_samples` are it. Named as a downstream consumer, not a hard in-batch dep.

**Engine-seam sub-check (overlay G1).** The constructors/operators are **prelude-resolved eval-path intercepting builtins** (the `gradient`/`sample` path, 4025), *not* one of the 7 engine-integration seams — G1 engine sub-check N/A for them. The **one exception** is `restrict`, whose sample-time point-in-region test must reach the geometry/OCCT layer; that crosses into the eval/engine layer and is treated as a contracted seam in §5 + §6 (it is a NEW internal seam, surfaced per the overlay rule rather than silently introduced).

---

## §2 — Sketch of approach: minimal native core + composable `.ri`

The design splits the surface into a **native primitive core** (Rust, polymorphic via the compiler result-type table — needs no generics) and a **composable `.ri` layer** (self-hosted stdlib one-liners — needs user-fn generics).

**Tier 1 — native primitives (Rust; land independent of generics):**

| Symbol | Native because | Backed by |
|---|---|---|
| `fn_field(f) -> Field<D,C>` | wraps a user lambda; the *enabler* of all user-side composition | reuses `FieldSourceKind::Analytical` + `apply_lambda_with_point_unpacking` |
| `from_samples(points, values, method) -> Field<D,C>` | gridded interp machinery is Rust | reuses `build_sampled_field` + `FieldSourceKind::Sampled` |
| `restrict(field, region) -> Field<D,C>` | sample-time point-in-region needs OCCT (full-solid, per design decision D4) | new `FieldSourceKind::Restricted` + geometry-containment seam (§6) |
| `enum InterpolationMethod` | a Reify-language enum mapped to internal `InterpolationKind` | `reify-compiler/stdlib` + `expr.rs:2692` enum-variant lowering |

**Tier 2 — composable `.ri` stdlib fns (depend on the generics prerequisite, §3):**

```reify
// crates/reify-compiler/stdlib/fields.ri  (authored once generics land)
pub fn constant_field<D, C>(value: C) -> Field<D, C> { fn_field(|p| value) }
pub fn clamp_field<D, Q: Dimension>(f: Field<D, Scalar<Q>>, lo: Scalar<Q>, hi: Scalar<Q>)
    -> Field<D, Scalar<Q>> { fn_field(|p| clamp(sample(f, p), lo, hi)) }
pub fn remap_field<D, Q: Dimension>(f: Field<D, Scalar<Q>>, from_range: Range<Scalar<Q>>, to_range: Range<Scalar<Q>>)
    -> Field<D, Scalar<Q>> { fn_field(|p| remap(sample(f, p), from_range, to_range)) }
pub fn threshold<D, Q: Dimension>(f: Field<D, Scalar<Q>>, value: Scalar<Q>)
    -> Field<D, Bool> { fn_field(|p| sample(f, p) > value) }
pub fn compose<A, B, C>(f: Field<B, C>, g: Field<A, B>) -> Field<A, C> { fn_field(|p| sample(f, sample(g, p))) }
```

This is the architecture the user selected: self-hosted, user-extensible stdlib. A user can write *their own* combinator (`fn knockdown(base) { fn_field(|p| clamp(sample(base,p), 10MPa, 200MPa)) }`) the moment `fn_field` is native — at a **concrete** type even before generics land. The generic *stdlib* surface lands after generics.

The compiler-typing seam (`is_field_op` / `field_op_result_type`, §5.1) types the native primitives' returns as `Field<…>` (today they fall through to a wrong first-arg fallback — `sample` is itself mistyped). This is a prerequisite for chaining and is delivered by task α.

---

## §3 — Pre-conditions for activating (G3)

| Capability | State on main | Resolution |
|---|---|---|
| `fn_field`/`from_samples`/`restrict`/`compose` return-type typing | falls through to wrong first-arg fallback | **delivered by task α** (this batch) |
| `FieldSourceKind::Analytical` + lambda sample dispatch | EXISTS (`lib.rs:205`) | reuse |
| `build_sampled_field` gridded interp | EXISTS (`engine_eval.rs:971`); **gridded-only** | `from_samples` scoped to grids (D3) |
| scalar `clamp`/`remap` | EXIST (`numeric.rs:101/219`) | reuse in Tier-2 bodies |
| Reify `enum` decl + variant resolution | EXISTS | reuse |
| lambda literal as call arg | **parses** (verified — fixture `/tmp/prd-fields-fixtures/e1_lambda_arg.ri`, `grammar.js:1029`) | reuse |
| geometry point-in-solid containment | EXISTS in `reify-eval` geometry-query layer; **NOT reachable from `reify-expr` sample path** (no geom dep) | **task δ** delivers the dispatch seam (§6) |
| **generic user `fn` declarations** (`fn f<D,Q: Dimension>`) | **ABSENT** — `FnDef.type_params` parsed but never read (`functions.rs:16`); `resolve_type_name("T")` fails; no stdlib generic fn exists | **HARD PREREQUISITE** — tracking task **G** (a separate generics PRD, authored next session). Tier-2 tasks ε/ζ `depends_on` G. *Note for that PRD:* `Type::TypeParam` already exists (`reify-core/src/ty.rs:59`); `TraitDef.type_params` (traits.rs:94) + `resolve_auto_type_params` are the reuse points — this is wiring, not a from-scratch type system. |

Tier 1 (α/β/γ/δ) has **no generics dependency** and ships now. Tier 2 (ε/ζ) and the full-surface integration gate (η) block on **G**.

---

## §4 — Resolved design decisions

- **D1 — Architecture: minimal native core + composable `.ri`** (user-selected). `fn_field`/`from_samples`/`restrict`/`InterpolationMethod` native; `constant_field`/`clamp_field`/`remap_field`/`threshold`/`compose` are composable generic `.ri` fns.
- **D2 — Block on generics** (user-selected). The composable ops are NOT shipped as monomorphic stopgaps (a `Field<Real,Real>`-only `clamp_field` is useless to the real consumer, whose fields are `Field<Point3<Length>, Scalar<Pressure>>`). They wait for the generics prerequisite G.
- **D3 — `from_samples` is gridded-only** (G6 floor). The interp substrate (`interp.rs`) operates on regular axis-aligned grids, not scattered points. `from_samples(points, values, method)` validates that `points` form a regular grid and builds a `SampledField`; non-grid input emits **`E_FIELD_SAMPLES_NOT_GRID`**. Supported methods: `Linear`, `NearestNeighbor`, `Cubic`. (Honest bound — not "arbitrary scattered interpolation.")
- **D4 — `restrict` is full-solid** (user-selected). `restrict(field, region)` evaluates point-in-region against a general `Geometry` solid; this requires relocating/extending the field-sample dispatch so it can reach OCCT containment (the §6 seam). Sample inside → inner value; outside → `Value::Undef`.
- **D5 — `InterpolationMethod` = implementable + deferred-with-diagnostic** (user-selected). `enum InterpolationMethod { Linear, NearestNeighbor, Cubic, RBF, Kriging }`. `Linear`/`NearestNeighbor`/`Cubic` work; `RBF`/`Kriging` parse but emit **`E_INTERP_METHOD_UNSUPPORTED`** at eval. Doc's `Bilinear`/`Trilinear` are dropped (Linear infers dimension from the grid). `docs/reify-stdlib-reference.md` §11 is reconciled to this set (task η).
- **D6 — `compose(f,g)` semantics.** `compose(f, g)(p) = f(g(p))` with `f: Field<B,C>`, `g: Field<A,B>` → `Field<A,C>`. Retires the gap that composition was reachable only via the `composed{}` block. The block syntax stays (back-compat); `compose` is the callable form.
- **D7 — `threshold` codomain is `Bool`.** `threshold(f, value) -> Field<D, Bool>`; the lambda body `sample(f,p) > value` yields `Value::Bool`. (Reductions like `max` over a `Bool`/Analytical field stay undefined — analytical fields have no grid to reduce; not a regression.)
- **D8 — native primitives' types via the result-type table**, mirroring `is_geometry_query`/`is_affine_map_constructor` (`expr.rs:1581-1687`). Fixes the pre-existing `sample`/`gradient` mistyping as a side-effect (in-scope, required for chaining).

---

## §5 — Contract (B + H)

### 5.1 Compiler typing seam — `field_op_result_type` (task α)

Add to the `NoUserFunctions` arm of `compile_expr` (`reify-compiler/src/expr.rs`, alongside the existing family predicates):

```
is_field_op(name) ⊇ { "fn_field", "from_samples", "restrict", "compose", "sample",
                       "gradient", "divergence", "curl", "laplacian" }
field_op_result_type(name, arg_types) -> Type:
  fn_field(λ:Function{params:[D], ret:C})      -> Field<D, C>
  from_samples(List<D>, List<C>, InterpMethod) -> Field<D, C>
  restrict(Field<D,C>, _region)                -> Field<D, C>
  compose(Field<B,C>, Field<A,B>)              -> Field<A, C>
  sample(Field<D,C>, D)                        -> C          // FIX: today wrongly first-arg (Field)
  gradient(Field<P,Scalar<Q>>)                 -> Field<P, Vector<…, Q/Length>>   // codomain-correct
  …divergence/curl/laplacian per §11 table
```

Invariant: a field-op call's compile-time cell type is `Field<…>` (or the sampled codomain for `sample`), never the first-arg fallback. Pinned by a compiler test asserting `sample(fn_field(|p| …), p)` types as the codomain, not `Field`.

### 5.2 `FieldSourceKind` storage-layout contract (`reify-ir/src/value.rs`, task α)

Reuses existing variants where possible; adds the minimum:

| Op | source kind | `lambda` slot holds | sample-dispatch |
|---|---|---|---|
| `fn_field` | `Analytical` (reuse) | `Value::Lambda` | existing `apply_lambda_with_point_unpacking` |
| `from_samples` | `Sampled` (reuse) | `Value::SampledField` | existing `sampled::sample_at_point` |
| `compose` | `Composed` (reuse; **list form**) | `Value::List[f, g]` | new arm: `sample(f, sample(g, p))` |
| `restrict` | **`Restricted` (new)** | `Value::List[inner, region]` | new arm: §6 containment → `sample(inner,p)` \| `Undef` |
| `constant_field`/`clamp_field`/`remap_field`/`threshold` | `Analytical` (Tier-2; via `fn_field`) | `Value::Lambda` (captures) | existing |

Precedent for list-stored aux data: `FieldSourceKind::SafetyFactor` stores `List[field, yield_val]` (`value.rs:871`). `compose` mirrors it.

### 5.3 `restrict` geometry-containment seam (task δ — the load-bearing seam)

`reify-expr` (where `sample` is intercepted) has **no geometry/OCCT dependency**. The `Restricted` sample arm needs `contains(region, point) -> Bool`. Contract: the field-sample path gains a **containment hook** resolvable in `reify-eval` (which depends on the geometry/OCCT layer and already evaluates `contains` via `geometry_ops`). Two admissible implementations (architect's choice at dispatch; both satisfy the same observable):
- (a) relocate the field-`sample` interception for `Restricted`-source fields into `reify-eval`, or
- (b) inject a containment callback (trait object) into the eval context that `reify-expr`'s sample arm invokes.
Ordering/error semantics: outside-region or indeterminate containment → `Value::Undef` (consistent with strict-Undef propagation, `lib.rs:189`).

### 5.4 `InterpolationMethod` ↔ `InterpolationKind` mapping (tasks γ, η)

`InterpolationMethod.{Linear,NearestNeighbor,Cubic}` → `InterpolationKind::{Linear,NearestNeighbor,Cubic}`. `{RBF,Kriging}` → eval-time `E_INTERP_METHOD_UNSUPPORTED`. The Reify enum is the user surface; `InterpolationKind` stays the internal carrier (erased after compile, like other enums per the DCE F-Mono erasure model).

---

## §6 — Boundary-test sketch (B + H; two-way)

Integration gate **η** names this sketch as its observable signal. Scenarios face both the **producer** (constructor builds a correct `Value::Field`) and the **consumer** (`sample`/reduction reads it back).

| # | Scenario | Precondition | Postcondition (observable via `reify eval`/`check`) | Faces |
|---|---|---|---|---|
| B1 | `fn_field` round-trip | `fn_field(\|p\| 2.0*p)` | `sample(f, 3.0) == 6.0` | producer→consumer |
| B2 | `from_samples` gridded interp | grid pts `[0,1,2]`, vals `[0,10,20]`, `Linear` | `sample(f, 0.5) == 5.0` | producer→consumer |
| B3 | `from_samples` non-grid reject | scattered (non-grid) points | `E_FIELD_SAMPLES_NOT_GRID` | producer floor |
| B4 | unsupported method | `from_samples(…, RBF)` | `E_INTERP_METHOD_UNSUPPORTED` | producer floor |
| B5 | `restrict` inside/outside | field over `box(10,10,10)` region | inside pt → inner value; outside pt → `Undef` | seam (§5.3) both ways |
| B6 | `constant_field` | `constant_field(42.0)` | `sample(c, anyPoint) == 42.0` | generics→producer |
| B7 | `clamp_field` | clamp `[10,200]`MPa | `sample` of an over-range pt == `200MPa` | generics→producer |
| B8 | `threshold` Bool codomain | `threshold(f, 250MPa)` | `sample` returns `true`/`false`; cell types as `Field<_,Bool>` | codomain change |
| B9 | `compose` | `compose(f, g)` | `sample(compose(f,g), p) == sample(f, sample(g,p))` | producer→consumer |
| B10 | chaining types | `sample(clamp_field(fn_field(\|p\|…), lo, hi), p)` | compiles (no first-arg-fallback type error); evaluates | §5.1 typing |

---

## §7 — Cross-PRD relationship & seam ownership (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **generic-user-functions** (to author) | consumes | generic `fn` type-param resolution + call-site inference | that PRD (tracking task **G**) | **blocked-prereq** (ε/ζ/η `depends_on` G) |
| `P10 structural-traits` (future) | produces | `constant_field`/`fn_field`/`from_samples` feeding `Field`-typed trait params | this PRD | future consumer |
| `std.analysis` (landed) | produces | `Field` values consumed by `von_mises`/`safety_factor` | this PRD | consistent (no change) |
| imported-field / OpenVDB (2580/3439) | sibling | `FieldSourceKind::Sampled` / `build_sampled_field` shared with `from_samples` | this PRD for `from_samples`; that work for `Imported` | coordinate (no ownership fight) |
| field differential ops (4025, done) | extends | shares the `is_field_op` typing table + fields.ri packaging file | this PRD | extends 4025 |

No reciprocal-ownership ambiguity. The only contested-surface risk is `restrict`'s containment seam (§5.3), owned wholly by this PRD (task δ).

---

## §8 — Decomposition plan (one bullet per task; observable signal in *italics*)

**Tier 1 — native (no generics dep):**

- **α — Field-op compiler signatures + `FieldSourceKind` scaffolding** *(intermediate; consumers β/γ/δ/ε/ζ).* Add `is_field_op` + `field_op_result_type` (§5.1, fixes `sample`/`gradient` typing); add `FieldSourceKind::Restricted` + the `Composed` list-form + sample-dispatch arms skeleton delegating to per-op helpers (§5.2). *Signal: intermediate — unlocks β/γ/δ; pinned by the §5.1 typing test (`sample(...)` types as codomain, not Field).*
- **β — `fn_field` native primitive** *(leaf).* Intercepting builtin wrapping a lambda as an `Analytical` field. *Signal: `examples/fields/fn_field.ri` — `sample(fn_field(\|p\| 2.0*p), 3.0)` evaluates to 6.0 via `reify eval`; B1 + B10.* Deps: α.
- **γ — `from_samples` + `InterpolationMethod` enum** *(leaf).* Reify enum (D5) + gridded `SampledField` builder + the two diagnostics. *Signal: `examples/fields/from_samples.ri` — gridded sample interpolates (B2); non-grid emits `E_FIELD_SAMPLES_NOT_GRID` (B3); `RBF` emits `E_INTERP_METHOD_UNSUPPORTED` (B4).* Deps: α.
- **δ — `restrict` full-solid + geometry-containment dispatch seam** *(leaf; B+H seam, §5.3/§6).* `FieldSourceKind::Restricted` containment via the eval/OCCT layer. *Signal: `examples/fields/restrict.ri` — a field restricted to `box(...)`; inside-point sample == inner value, outside-point sample == `Undef` (B5), via `reify eval`.* Deps: α.

**Tier 2 — composable `.ri` (depend on generics G):**

- **ε — composable spatial ops `constant_field`/`clamp_field`/`remap_field`/`threshold`** *(leaf).* Generic `.ri` fns over `fn_field`+`sample`+scalar ops. *Signal: `examples/fields/spatial_ops.ri` — clamp/remap/threshold each sampled & asserted, incl. `Field<_,Bool>` for threshold (B6/B7/B8), via `reify check`.* Deps: G, β, α.
- **ζ — callable `compose(f,g)`** *(leaf).* Generic `.ri` fn `fn_field(\|p\| sample(f, sample(g,p)))`. *Signal: `examples/fields/compose.ri` — `sample(compose(f,g), p) == sample(f, sample(g,p))` (B9).* Deps: G, β, α.

**Integration + reconcile:**

- **η — full-surface integration gate + doc/gap-register reconcile** *(leaf; the B+H integration task).* Update `fields.ri` to declare/document the whole surface; reconcile `docs/reify-stdlib-reference.md` §11 (InterpolationMethod set D5, callable `compose` D6) and the `gap-register` P16 rows + the two §10-11 doc-reconcile rows. Ship `examples/fields/std_fields_surface.ri` exercising every symbol end-to-end. *Signal: the §6 boundary-test sketch runs green in CI (`examples/fields/std_fields_surface.ri` via `reify eval`); the gap-register P16 rows + the InterpolationMethod/compose doc-reconcile rows are marked closed.* Deps: β, γ, δ, ε, ζ.

**Out of batch (prerequisite):**

- **G — tracking task: author generic-user-function PRD** *(out-of-batch prerequisite).* The foundational generics feature; authored in a follow-up `/prd` session. ε/ζ/η `depends_on` G.

Dependency view: `α → {β, γ, δ}`; `{G, β} → {ε, ζ}`; `{β, γ, δ, ε, ζ} → η`. Tier 1 (α/β/γ/δ) is independently landable; the surface fully closes once G lands and ε/ζ/η follow.

---

## §9 — Out of scope

- Scattered-data interpolation (RBF/Kriging math) — deferred (D3/D5); `from_samples` is gridded.
- The generics feature itself — separate PRD (G).
- `Bilinear`/`Trilinear` as distinct variants (D5).
- Reductions (`max`/`min`/`argmax`) over analytical/derived fields — already partially handled (tasks 2913/4085); not extended here.
- `gradient`/`divergence`/`curl`/`laplacian` behavior (done, 4025) — only their *compile-time typing* is touched (α, §5.1).
- Geometry import / PointCloud (`P15`), structural-trait `Field` params (`P10`) — separate clusters.

---

## §10 — Open (tactical) questions

- **§5.3 restrict seam — (a) relocate vs (b) callback.** Either satisfies B5; the architect picks at dispatch. (Tactical: both keep the system coherent.)
- **`compose` storage — reuse `Composed` list-form vs a new `Composed2` kind.** Both work; reuse is preferred (fewer variants). Tactical.
- **Where the `InterpolationMethod` enum is declared** — `reify-compiler/stdlib/fields.ri` vs a dedicated `interpolation.ri`. Tactical; η decides.
- **`fn_field` arg arity surface** — whether to also accept a bare value (auto-lift to constant) before `constant_field` lands. Tactical; β decides (likely no — keep `fn_field` lambda-only, `constant_field` is the value form).

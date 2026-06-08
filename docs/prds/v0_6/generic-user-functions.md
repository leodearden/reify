# PRD: Generic User-Function Support (type-param resolution + call-site inference)

**Status:** authored 2026-06-02 · **Milestone:** v0_6 · **Approach:** B + H (design-first contract + two-way boundary tests)
**Tracking task:** 4218 (`Author PRD: generic user-function support`).
**Primary consumer:** `docs/prds/v0_6/std-fields-api.md` Tier-2 (`constant_field`/`clamp_field`/`remap_field`/`threshold`/`compose`) — its §3 row **G** and tasks ε/ζ/η (4223/4224/4225) hard-depend on this PRD.
**Sibling pattern:** `docs/prds/v0_6/generic-data-carrying-enums.md` (tasks 4029/4030/4031) — the analogous *enum*-side type-param threading. This PRD is the *function*-side of the same discipline: reuse the existing type-param machinery, do not build a new generics system.

---

## §0 — Premise correction (read first)

Task 4218 frames the problem as `FnDef.type_params` being parsed-but-never-read at `functions.rs:16`. That is true but **understates one place and over-states another**; both corrections are load-bearing:

1. **Grammar is already done (G3 clear).** `function_definition` already carries `optional($.type_parameters)` (`tree-sitter-reify/grammar.js:193`). Verified 2026-06-02 — all four generic-fn fixtures parse with **0 ERROR/MISSING** nodes, including the bounded form, `Field<D, Scalar<Q>>` return types, and lambda bodies (§3.5). **No grammar work is required** — unlike the sibling enum PRD, which had to add `optional($.type_parameters)` to `enum_declaration`. Every leaf here is `grammar_confirmed=true`.

2. **The gap is TWO hardcoded `empty_type_params`, not one.** Besides `functions.rs:16` (top-level param/return resolution), `resolve_parameterized_builtin_type` *also* hardcodes `let empty_type_params = HashSet::new();` (`type_resolution.rs:1335`) for **every** inner type-arg it resolves (`List<T>`, `Map<K,V>`, `Field<D,C>` domain/codomain, `Tensor`/`Matrix` quantity). Consequence: even a purely type-level `Field<D, C>` with type-params **does not resolve today**, because the inner `D`/`C` are resolved against an empty set → "unresolved type". Threading the type-param name-set through *both* sites is the core of Tier 1.

3. **`resolve_auto_type_params` is NOT the ordinary inference machinery.** The task and the fields-api §3 note both cite `resolve_auto_type_params` as the inference reuse point. Verified: that resolver (`auto_type_param.rs:1080`) is the **structure-candidate-enumeration** engine (Phase A/B/C over a `TopologyTemplate` registry) for `auto` type-params — it answers "search a candidate pool for a type that satisfies these bounds." Ordinary generic-fn call-site inference is the **opposite shape**: a single-pass unification of declared `Type::TypeParam` param slots against the supplied arguments' concrete types. That is the same machinery the sibling enum PRD's construction-inference leaf builds (task 4031 / DCE γ — also not yet landed), **not** `resolve_auto_type_params`. `resolve_auto_type_params` is reused *only* for the optional `auto`-on-fn-type-param path (§11, deferred), exactly as the enum PRD reuses it only for the deferred `auto`-on-enum path.

4. **Two capability tiers, one PRD (user-selected 2026-06-02).** The fields-api consumers split cleanly: `constant_field<D,C>` and `compose<A,B,C>` need only **type-level** params (`Type::TypeParam`-representable); `clamp_field`/`remap_field`/`threshold` need `Scalar<Q>` with `Q: Dimension` — a **dimension-level** param that `Type::Scalar { dimension: DimensionVector }` cannot represent (the dimension slot demands a concrete `DimensionVector`, `type_resolution.rs:1401-1406`). **Tier 1 (type-level)** is pure wiring over existing machinery and ships first. **Tier 2 (dimension-generics)** is a gated later tier that adds a dimension-param representation — genuinely new type-system surface, acknowledged as such, not wiring.

**This PRD threads functions into the existing type-param machinery. It does not invent resolution, substitution, or `auto` algorithms (Tier 1), and it adds exactly one new representation — a dimension-kinded param — in Tier 2.**

---

## §1 — Goal & observable surface

A user (and the stdlib) can declare a generic `fn`, call it with no explicit type arguments, and have the compiler infer the type arguments from the call's value arguments and substitute them into the return type.

### Tier 1 — type-level (ships first)

```reify
pub fn id<T>(x: T) -> T { x }                              // identity over any type
pub fn single<T>(x: T) -> List<T> { [x] }                 // type-param inside a builtin (List<T>)
pub fn constant_field<D, C>(value: C) -> Field<D, C> {    // the fields-api consumer
    fn_field(|p| value)
}
pub fn compose<A, B, C>(f: Field<B, C>, g: Field<A, B>) -> Field<A, C> {
    fn_field(|p| sample(f, sample(g, p)))
}
```

What lands (observable via `reify check` / `reify eval`):

- `id(5mm)` → `5 mm`; the call expression's compile-time type is `Length` (the substituted `T`), **not** a first-arg fallback or `unresolved type`.
- `single(5mm)` → `[5 mm]`; `T` is inferred as `Length` **inside** `List<T>` (exercises the `resolve_parameterized_builtin_type` threading at eval).
- `sample(constant_field(42.0), anyPoint)` → `42.0`; `constant_field`'s signature resolves (`D`, `C` are `Type::TypeParam` inside `Field<…>`), and the call types `C = Real`.
- A generic fn with a real trait bound `fn f<T: Solid>(x: T) -> T { x }` called with a non-conforming argument emits a bound diagnostic; with a conforming one it checks clean.
- A call whose arguments bind one type-param to two different types (e.g. `compose` with mismatched middle types) emits **`E_FN_TYPE_ARG_CONFLICT`**.

### Tier 2 — dimension-generics (gated later tier)

```reify
pub fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }
pub fn clamp_field<D, Q: Dimension>(f: Field<D, Scalar<Q>>, lo: Scalar<Q>, hi: Scalar<Q>)
    -> Field<D, Scalar<Q>> { fn_field(|p| clamp(sample(f, p), lo, hi)) }
```

What lands:

- `scale_q(10mm, 3.0)` → `30 mm` **and** `scale_q(5MPa, 2.0)` → `10 MPa` — the *same* generic fn applied at two dimensions, `Q` bound to `LENGTH` then `PRESSURE` per call, the return scalar carrying the bound dimension.
- The fields-api `clamp_field`/`remap_field`/`threshold` (`Q: Dimension`) type-check — the dimension-level consumers.

**Completion gate for dependents (per 4218):** generics *implemented on main* (not merely PRD authored) — concretely, a generic stdlib `.ri` fn type-checks end-to-end. Tier-1 leaf δ is that gate for type-level; Tier-2 leaf ζ for dimension-level.

---

## §2 — Consumer (G1)

This is a **core-language / type-system capability** (type resolution + call-site overload resolution + eval), **not an in-engine seam**. No `engine-integration-norm.md` §3 seam is touched — generic-fn type-param resolution is a compile-time concern plus a type-erased (unchanged) eval path. It reuses the *same* type-param resolver structures (`resolve_type_with_params`, `convert_type_params`, `satisfies_trait_bound`) that structures/traits already use, which are likewise not engine seams. Overlay G1 engine sub-check: **N/A**.

Named consumers (no orphans):

1. **Downstream PRD — `std-fields-api.md` Tier-2 (primary; the reason this PRD exists).** Tasks ε/ζ/η (4223/4224/4225) author `constant_field`/`compose` (type-level → Tier 1) and `clamp_field`/`remap_field`/`threshold` (dimension-level → Tier 2) as composable generic `.ri` stdlib fns. Their dependency on tracking task 4218 (G) is re-pointed at this PRD's integration leaves at decompose (§6, §9). Direction: that PRD consumes this one; this PRD does not depend back.
2. **User surface — CLI `reify check` / `reify eval`** over a `.ri` file declaring a generic fn, calling it with inferred type args. The in-batch G2 signal-bearer for every leaf (self-contained examples under `examples/generics/`, run in CI).
3. **User surface — stdlib `.ri`.** A generic fn placed in a stdlib `.ri` file type-checks during stdlib load (leaf δ confirms this — it is the literal 4218 completion-gate phrasing).
4. **Spec self-consistency — §3.9.** §3.9 states type parameters are "resolved at definition time (compile time)" and is written as if structure/trait/**fn** generics all work; fn generics are in fact unimplemented. Leaf η reconciles the spec to the implemented capability (type-level + dimension-kinded params, erasure, conservative inference).

No mechanism in this PRD is a producer without one of the above consumers.

---

## §3 — Background: current implementation chain

Verified 2026-06-02. The chain that must change, end to end. Tier marked **[T1]** (type-level) / **[T2]** (dimension-generics).

| Layer | File / site | Today | Needs |
|---|---|---|---|
| Grammar | `grammar.js` `function_definition` (line 193) | already `optional($.type_parameters)` | **none** — verified 0-ERROR (§3.5); `grammar_confirmed=true` |
| AST | `reify-ast/src/decl.rs` `FnDef` (736-751) | has `type_params: Vec<TypeParamDecl>` + `TypeParamDecl{name,bounds:Vec<String>,default}` (912-917) | **none** — already parsed/lowered to AST |
| Compile (sig) | `reify-compiler/src/functions.rs:16` | `let empty_params = HashSet::new();` → param/return types resolved against **no** type-params | **[T1]** build the set from `fn_def.type_params` (mirror the structure path, `entity.rs:560-564`) and pass it to param + return resolution |
| Compile (inner) | `reify-compiler/src/type_resolution.rs:1335` | `resolve_parameterized_builtin_type` hardcodes `empty_type_params` for **all** inner args (`List`/`Map`/`Field`/`Tensor`/…) | **[T1]** thread the type-param name-set through so `Field<D,C>`/`List<T>` inner args resolve to `Type::TypeParam` (a `_with_params` variant, or add the param) |
| Type | `reify-core/src/ty.rs:106` `Type::TypeParam(String)` | exists; consumed by structure/trait generics | **[T1]** reused verbatim for unresolved fn param/return types |
| IR | `reify-ir/src/expr.rs` `CompiledFunction` (245-292) | `params`, `param_defaults`, `return_type: Type`, **no `type_params`** | **[T1]** add `type_params: Vec<TypeParam>` (the IR `TypeParam`, `traits.rs:30`, as `TraitDef`/`EnumDef` carry it); lower via existing `convert_type_params` (`type_resolution.rs:1877`) |
| Compile (call) | `reify-compiler/src/expr.rs:1424` | `let result_type = matched_fn.return_type.clone();` — verbatim, no substitution | **[T1]** when the matched fn is generic, infer subst from args, substitute into `return_type` |
| Compile (overload) | `reify-compiler/src/type_compat.rs:335-396` `resolve_function_overload` | matches by `param_ty == arg_ty` (or trait-object wildcard) | **[T1]** a `Type::TypeParam` param never equals a concrete arg → add **unification-aware** matching for generic fns |
| Substitution | — | no generic walk-and-substitute over a resolved `Type` exists (alias DFS has `resolve_type_alias_expr_with_subst` over `HashMap<String,Type>`, `type_resolution.rs:1192`) | **[T1]** add `substitute_type_params(&Type, &HashMap<String,Type>) -> Type` (recurse List/Set/Map/Option/Field/Function/Tensor/…) |
| Bounds | `reify-compiler/src/entity.rs:3639` `check_type_param_bounds` / `:3732` `satisfies_trait_bound` | validate a concrete type-arg against `T: Trait` for **structures** | **[T1]** reuse for fn type-params (thread the `CompiledTrait` registry into the call-site bound check) |
| Body | `reify-compiler/src/type_compat.rs:361` `type_carries_trait_object` wildcard | trait-typed params already act as resolution wildcards | **[T1]** treat `Type::TypeParam` likewise — operations on type-param-typed values in a generic body are not eagerly rejected (permissive checking, D4) |
| Dim slot | `reify-compiler/src/type_resolution.rs:1401-1406` `Scalar<Q>` | resolves `Q` via `resolve_type_alias_expr_to_dimension` → a **concrete** `DimensionVector`; no param slot | **[T2]** dimension-kinded param representation (D7) so `Scalar<Q>`/`Vector3<Q>`/`Point3<Q>` resolve with `Q` a dimension-param |
| Eval | `reify-expr` / `reify-eval` | evaluates concrete `Value`s | **none** — type-erasure (D1): generic fns are monomorphic-by-value at eval; the arguments are already concrete `Value`s; eval is type-arg-agnostic |

### 3.5 Grammar reality check (G3) — fixtures (tree-sitter, 2026-06-02)

Per the silent-misparse trap, the signal is CST ERROR/MISSING-node count, not exit code. Run from `tree-sitter-reify/`. Fixtures committed under the manifest evidence set.

| Fixture | Syntax | ERROR/MISSING | Verdict |
|---|---|---|---|
| `guf-3-simple.ri` | `pub fn id<T>(x: T) -> T { x }` | **0** | bare type-param fn parses |
| `guf-1-generic-fn.ri` | `pub fn constant_field<D, C>(value: C) -> Field<D, C> { fn_field(\|p\| value) }` | **0** | type-param inside `Field<D,C>` + lambda body parse |
| `guf-2-bounded.ri` | `pub fn clamp_field<D, Q: Dimension>(f: Field<D, Scalar<Q>>, lo: Scalar<Q>, hi: Scalar<Q>) -> Field<D, Scalar<Q>> { … }` | **0** | bounded param + `Scalar<Q>` parse (the gap is *resolution*, not parse) |
| `guf-4-compose.ri` | `pub fn compose<A, B, C>(f: Field<B, C>, g: Field<A, B>) -> Field<A, C> { … }` | **0** | multi-param + nested `sample` parse |

**G3 resolution:** zero grammar work. The entire feature is compiler-side resolution + call-site inference. `grammar_confirmed=true` on every leaf. (Contrast: the sibling enum PRD's `gde-6` showed 3 ERROR nodes from `<T>` after the enum name — `enum_declaration` lacked `optional($.type_parameters)`; `function_definition` does not.)

---

## §4 — Sketch of approach

### 4.1 Tier 1 — thread the type-param set (mirror the structure path)

Structures already do exactly what functions must (`entity.rs:560-564`):

```rust
let type_param_names: HashSet<String> =
    structure.type_params.iter().map(|tp| tp.name.clone()).collect();
// … passed as the `type_param_names` arg to resolve_type_expr_with_aliases for every member type
```

`compile_function` mirrors this: build the set from `fn_def.type_params`, pass it where `empty_params` is passed today (functions.rs:27 param loop + the return-type resolution), and **also** thread it into `resolve_parameterized_builtin_type` so a param/return type of `Field<D, C>` resolves its inner `D`/`C` to `Type::TypeParam` (today they hit the hardcoded empty set, `type_resolution.rs:1335`). The name→`Type::TypeParam` mapping already exists: `resolve_type_with_params` (`type_resolution.rs:591-602`) returns `Type::TypeParam(name)` when the name is in the set. Convert AST `type_params` → IR via the existing `convert_type_params` (`type_resolution.rs:1877`) and store on `CompiledFunction`. A signature type naming an undeclared param → `E_FN_UNKNOWN_TYPE_PARAM`.

### 4.2 Tier 1 — call-site inference (single-pass unification) + return substitution

At the call site (`expr.rs:1424`, where `result_type = matched_fn.return_type.clone()`), when the matched `CompiledFunction` has non-empty `type_params`:

```
subst : HashMap<String, Type> = {}
for (declared_param_ty, arg_ty) in zip(fn.params, arg_types):
    unify(declared_param_ty, arg_ty, &mut subst)        // structural walk; binds Type::TypeParam leaves
result_type = substitute_type_params(&fn.return_type, &subst)
```

`unify` walks the declared type structurally; at a `Type::TypeParam(P)` leaf it binds `P → arg_ty` (and on a second, differing binding for the same `P` emits **`E_FN_TYPE_ARG_CONFLICT`**). Params not reached by any argument stay **unbound** — the substituted return type may legitimately still contain a `Type::TypeParam` (e.g. `constant_field`'s `D` is undetermined by `value: C`; it is pinned later by the *enclosing* `sample` call's inference, or stays a wildcard). This is **conservative, payload-driven, single-pass** unification — not general Hindley–Milner, not `resolve_auto_type_params` candidate enumeration (§0.3). `resolve_function_overload` (type_compat.rs:335) is made unification-aware so a generic candidate whose param is `Type::TypeParam` *matches* a concrete arg instead of failing the `param_ty == arg_ty` test.

### 4.3 Tier 1 — bounds + permissive body checking

`fn f<T: Solid>(…)`: the inferred concrete arg for `T` is validated against the bound via the existing `satisfies_trait_bound` / `check_type_param_bounds` (`entity.rs:3639/3732`), threading the `CompiledTrait` registry into the call-site check. An *unbound* bounded param is not checked (no concrete to check; if a concrete arg is required but undetermined → `E_FN_TYPE_ARG_UNRESOLVED`). Inside a generic body, a value typed `Type::TypeParam` is a **resolution wildcard** — operations on it (`fn_field(|p| value)`, `clamp(sample(f,p), …)`) are not eagerly rejected, mirroring how `type_carries_trait_object` (type_compat.rs:361) already relaxes trait-typed params. Full bounded-operation checking (where-clauses that *license* specific operations on a bounded param) is **out of scope** (§11) — the permissive model is what the fields-api bodies need and matches the language's existing trait-object wildcard behavior.

### 4.4 Tier 2 — dimension-kinded params

`Q: Dimension` declares `Q` **dimension-kinded** (D7). `Dimension` is a **built-in kind-bound**, not a user trait — it does not live in the trait registry; it marks that `Q` ranges over *dimensions*, so `Q` may appear in a dimension slot (`Scalar<Q>`, `Vector3<Q>`, `Point3<Q>`). A dimension-param needs a representation the quantity slot can hold (today `Type::Scalar { dimension: DimensionVector }` is concrete-only). The `Scalar<Q>`/`Vector3<Q>`/`Point3<Q>` arms (`type_resolution.rs:1401-1417`) gain a dimension-param branch. Call-site inference binds `Q` to the concrete `DimensionVector` of the supplied scalar's quantity and substitutes it into the return (extending §4.2's unification into the dimension slot). This is the one **new** type-system representation in the PRD; it is gated behind Tier 1 and acknowledged as more than wiring.

### 4.5 Type erasure (eval unchanged)

Matching structures (`Value::StructureInstance` carries no type args) and the sibling enum PRD (F-Mono erasure), generic-fn type args are resolved/checked at **compile time and erased**. At eval, a generic-fn call is a call whose arguments are already concrete `Value`s; the body evaluates monomorphically-by-value. **No eval change** — leaf δ's boundary test pins this (INV-2).

---

## §5 — Resolved design decisions

- **D0 — Thin extension (Tier 1 is wiring).** Reuse `Type::TypeParam`, `resolve_type_with_params`, `convert_type_params`, `satisfies_trait_bound`/`check_type_param_bounds`, and the structure threading pattern. No new resolution/`auto` algorithm for type-level generics.
- **D1 — Type-erasure at compile time.** Type args resolved/checked at compile, erased before eval; `CompiledFunction.type_params` is compile-time-only; eval is type-arg-agnostic. Matches structures + the enum F-Mono decision. **No monomorphization-by-cloning** — one `CompiledFunction` per generic fn, per-call-site substitution for type-checking only.
- **D2 — Call-site inference = conservative single-pass unification.** Unify declared param types (with `Type::TypeParam` leaves) against concrete arg types → `HashMap<String,Type>`. Conflict for one param → `E_FN_TYPE_ARG_CONFLICT`; never silently picks one. Params unmentioned by any arg stay **unbound** (the result type may retain a `Type::TypeParam`), never guessed. This is the *function* analog of the enum PRD's construction-inference (4031); it is **not** `resolve_auto_type_params` (§0.3).
- **D3 — Return-type substitution via a new `substitute_type_params` walk.** No generic type-substitution helper exists; add one (recurse `List`/`Set`/`Map`/`Option`/`Field`/`Function`/`Tensor`/`Matrix`/…). Model on the alias `_with_subst` `HashMap<String,Type>` pattern (type_resolution.rs:1192).
- **D4 — Permissive generic body checking.** `Type::TypeParam`-typed values are resolution wildcards (mirror `type_carries_trait_object`); builtin/op calls on them are not eagerly rejected. Full bounded-operation licensing is out of scope (§11).
- **D5 — Trait bounds reuse the structure machinery.** `T: Trait` (a real trait) is validated against the inferred concrete arg via `satisfies_trait_bound`. Unbound bounded params: no check (or `E_FN_TYPE_ARG_UNRESOLVED` where a concrete arg is required).
- **D6 — Thread through `resolve_parameterized_builtin_type` too.** The second hardcoded `empty_type_params` (type_resolution.rs:1335) is load-bearing: without it, even type-level `Field<D,C>` fails. Tier-1 leaf α fixes both sites.
- **D7 — `Dimension` is a built-in kind-bound (Tier 2).** Not a user trait. `Q: Dimension` marks `Q` dimension-kinded; dimension-kinded params may appear in dimension slots. Representation of a dimension-param in the quantity slot is the one new type-system surface (recommended: a dedicated `Type::ScalarParam(String)` quantity wrapper; alternative: generalize the dimension slot to `concrete | param` — tactical, §12 Q3, the impl picks; both satisfy the same observable). Gated behind Tier 1.
- **D8 — Dimension-param call-site inference (Tier 2).** Extends D2's unification to bind a dimension-param `Q` to the concrete `DimensionVector` of the argument scalar's quantity, then substitute into the return. Erased at eval like D1.
- **D9 — Inference-only call sites.** No explicit turbofish (`f<Length>(x)`) at call sites in v1 — the consumer relies entirely on inference (matching the enum PRD, which also infers from the construction payload). Explicit call-site type-args are out of scope (§11); the `type_arg_list` grammar in type position is untouched.
- **D10 — Back-compat: empty `type_params` is today's behavior bit-for-bit.** A non-generic fn has empty `type_params`; the call path is unchanged (no unification, `result_type = return_type.clone()` as today). INV-6.

---

## §6 — Cross-PRD relationship & seam ownership (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **`std-fields-api.md` Tier-2** (4223 ε, 4224 ζ, 4225 η) | this **produces**, that **consumes** | generic `fn` type-param resolution + call-site inference + return substitution (type-level → Tier 1) and `Scalar<Q>` dimension-generics (→ Tier 2) | **this PRD** | **re-wired at decompose**: 4224 (compose, type-level) `depends_on` this PRD's Tier-1 gate (δ); 4223 (`constant_field` type-level + `clamp_field`/`remap_field`/`threshold` dimension-level) `depends_on` this PRD's Tier-1 gate (δ) **and** Tier-2 gate (ζ); the old `depends_on 4218` edges on 4223/4224 are replaced. 4225 (η) inherits transitively. |
| **`generic-data-carrying-enums.md`** (4029/4030/4031) | sibling | both thread `type_params` into resolution; both build single-pass payload/arg-driven unification; both erase at eval (F-Mono) | enum PRD owns the *enum* side; this PRD owns the *function* side | **coordinate, no contest.** The unification logic (D2) and a `substitute_type_params` walk (D3) are candidates to share a helper crate-internally; neither PRD blocks the other (enum tasks operate on `EnumDef`/construction, fn tasks on `CompiledFunction`/call-sites). No ownership fight. |
| **`structure-instance-runtime.md` (GR-001)** | independent | — | n/a | erasure (D1) adds no runtime type-arg carrier; no GR-001 edge. |
| `auto type-param` resolver (`auto_type_param.rs`, landed) | reuse-only (deferred path) | `resolve_auto_type_params` Phase A/B/C | that code | **not used** by ordinary inference (§0.3); reused only for the deferred `auto`-on-fn-type-param path (§11). |

**Seam-ownership statement (G4).** The call-site inference seam is owned wholly **here**. No contested-ownership pair from `phase-3-breadcrumb-map.md` §3 is touched (none involves generics or fn call-sites). The `satisfies_trait_bound`/`check_type_param_bounds` and `convert_type_params` machinery is **reused, not re-owned** — this PRD plugs fn type-params into the existing structure-side owners; it does not fork them. The one **new** seam is the Tier-2 dimension-param representation (D7), owned wholly here, gated behind Tier 1.

---

## §7 — Contract (B + H)

The seam is between `reify-compiler` (resolves type-params in signatures, infers type-args at call sites by unification, substitutes into return types, checks bounds) and `reify-expr`/`reify-eval` (evaluates — **unchanged** under D1 erasure; the eval-side contract is "the value model is type-arg-agnostic").

### 7.1 Data structures (the contract surface)

```rust
// reify-ir/src/expr.rs — CompiledFunction gains type_params (the ONLY shape change to the IR)
pub struct CompiledFunction {
    pub name: String,
    pub params: Vec<(String, Type)>,           // a param Type MAY be Type::TypeParam(name) [T1]
    pub param_defaults: Vec<Option<CompiledExpr>>,
    pub return_type: Type,                      // MAY be Type::TypeParam / contain one [T1]
    pub type_params: Vec<reify_ir::TypeParam>,  // [T1] NEW — reuses traits.rs:30 TypeParam, as TraitDef/EnumDef do
    pub body: CompiledFnBody,
    // …doc, is_pub, content_hash, annotations, optimized_target unchanged…
}

// reify-core/src/ty.rs — Type::TypeParam(String) reused unchanged for type-level params [T1].
// Tier 2 adds ONE dimension-param representation (D7); recommended:
//   Type::ScalarParam(String)   // a scalar whose dimension is the named dimension-param [T2]
// (alternative: generalize the Scalar/Vector/Point quantity slot to concrete|param — §12 Q3)

// reify-compiler — compile-time-only, NOT persisted on IR/Value:
//   substitute_type_params(ty: &Type, subst: &HashMap<String, Type>) -> Type   [T1, D3]
//   unify(declared: &Type, arg: &Type, subst: &mut HashMap<String, Type>) -> Result<(), Conflict>  [T1, D2]
```

### 7.2 Invariants

- **INV-1 (reuse, not reinvent).** Fn type-params use the same grammar, the same `Type::TypeParam`, the same IR `TypeParam`, the same `convert_type_params`, and the same `satisfies_trait_bound`/`check_type_param_bounds` as structure/trait generics. No fn-specific generics machinery beyond `substitute_type_params`/`unify` (which the enum side needs too). (Test: a fn's `type_params` are the same `TypeParam` type as a trait's and feed the same bound-check entry point.)
- **INV-2 (erasure / eval unchanged).** `CompiledFunction.type_params` is compile-time-only; eval of a generic-fn call is indistinguishable from a non-generic call with the same concrete arguments. (Boundary test: `id(5mm)` and a hypothetical `id_length(5mm)` produce identical eval traces / results.)
- **INV-3 (conservative inference).** A type-param is bound only when an argument unambiguously determines it (D2). Conflicting bindings → `E_FN_TYPE_ARG_CONFLICT`; unmentioned params stay unbound, never guessed.
- **INV-4 (typed result).** The call expression's compile-time type is `substitute_type_params(return_type, subst)` — never a first-arg fallback. (Test: `id(5mm)` types as `Length`; `sample(constant_field(42.0), p)` types as the codomain `Real`, and `single(5mm)` types as `List<Length>`.)
- **INV-5 (bounds enforced).** A bound `T: Trait` rejects a non-conforming inferred arg (`satisfies_trait_bound`); a conforming arg checks clean. (Test: `f<T: Solid>` on a non-solid arg → bound diagnostic.)
- **INV-6 (back-compat).** Empty `type_params` reproduces today's non-generic fn behavior bit-for-bit (D10); existing stdlib `.ri` and `examples/` stay green.
- **INV-7 (dimension-genericity, Tier 2).** The same generic fn applied at two dimensions yields results carrying each call's dimension. (Test: `scale_q(10mm,3.0) == 30mm` and `scale_q(5MPa,2.0) == 10MPa`.)

### 7.3 Error semantics (user-visible diagnostics — G2 leaf signals)

| Code (illustrative) | Trigger | Where |
|---|---|---|
| `E_FN_UNKNOWN_TYPE_PARAM` | a signature type names a param not in `<…>` (e.g. `fn f<T>(x: U) -> T`) | compiler, decl |
| `E_FN_TYPE_ARG_CONFLICT` | one type-param bound to two types by the arguments (e.g. `compose` with mismatched middle types) | compiler, call-site |
| `E_FN_TYPE_ARG_UNRESOLVED` | a param undetermined by inference, unpinned, at a site requiring it concrete | compiler, call-site |
| (existing bound diag) | inferred arg fails `T: Trait` — reuse the structure-side bound diagnostic, do **not** mint a new code unless the registry lacks one | compiler, call-site |
| `E_DIM_PARAM_KIND` (T2, illustrative) | a dimension slot names a non-dimension-kinded param, or a dimension-kinded param used as an ordinary type | compiler, decl/resolve |

Exact codes are tactical (§12 Q1) — reuse/extend existing diagnostic-registry codes where they exist.

---

## §8 — Boundary-test sketch (B + H; two-way)

Integration leaves δ (Tier 1) and ζ (Tier 2) name this sketch as their observable signal. Each scenario faces both the **producer** (signature resolves / inference binds) and the **consumer** (`reify eval` reads back the right value/type).

| # | Scenario | Precondition | Postcondition (via `reify check`/`eval`) | Faces |
|---|---|---|---|---|
| B1 | identity round-trip | `fn id<T>(x: T) -> T { x }` | `id(5mm) == 5mm`; call types as `Length` | producer→consumer |
| B2 | type-param inside a builtin | `fn single<T>(x: T) -> List<T> { [x] }` | `single(5mm) == [5mm]`; types as `List<Length>` (exercises the :1335 fix) | producer→consumer |
| B3 | `Field<D,C>` signature resolves | `fn constant_field<D,C>(value: C) -> Field<D,C> { fn_field(\|p\| value) }` | compiles (no "unresolved type"); `sample(constant_field(42.0), p) == 42.0` | producer→consumer |
| B4 | inference conflict | a call binding one param to two types | `E_FN_TYPE_ARG_CONFLICT` | producer floor |
| B5 | unbound param tolerated | `constant_field(42.0)` with `D` undetermined | checks clean; result type retains `Type::TypeParam("D")` (pinned by the enclosing `sample`) | producer floor |
| B6 | trait bound | `fn f<T: Solid>(x: T) -> T { x }` | non-conforming arg → bound diagnostic; conforming arg → clean | bounds |
| B7 | back-compat | a non-generic fn | identical behavior/results to pre-change (INV-6) | regression pin |
| B8 | erasure | any generic-fn call | eval trace identical to the monomorphic equivalent (INV-2) | seam (eval unchanged) |
| B9 (T2) | dimension-genericity | `fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x*k }` | `scale_q(10mm,3.0)==30mm` **and** `scale_q(5MPa,2.0)==10MPa` | producer→consumer |
| B10 (T2) | dim-param signature resolves | `fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>` | compiles (no "unresolved type" on `Scalar<Q>`); `Q`-as-ordinary-type misuse → `E_DIM_PARAM_KIND` | producer floor |

---

## §9 — Decomposition plan (DAG; Greek labels, real IDs at decompose)

**B + H.** Type-param threading first (the substrate), then the call-site inference/substitution seam, then bounds+body, then the Tier-1 integration gate; Tier-2 (dimension-param representation, then its integration gate) gated behind Tier 1; doc last. `grammar_confirmed=true` on every leaf (§3.5).

### Tier 1 — type-level generics

- **α — Thread fn type-params through signature resolution + `CompiledFunction.type_params`** *(intermediate; unlocks β/γ/ε).* In `compile_function` build the type-param name-set from `fn_def.type_params` (mirror `entity.rs:560-564`) and pass it to param + return-type resolution; thread it into `resolve_parameterized_builtin_type` (D6, the `:1335` site) so `Field<D,C>`/`List<T>` inner args resolve to `Type::TypeParam`; add `type_params: Vec<TypeParam>` to `CompiledFunction` via `convert_type_params`; emit `E_FN_UNKNOWN_TYPE_PARAM`. *Signal (intermediate): a compiler unit test — `fn constant_field<D,C>(value: C) -> Field<D,C>` lowers to a `CompiledFunction` with `type_params == [D,C]` and `params`/`return_type` containing `Type::TypeParam` **including inside `Field<…>`**; a signature naming an undeclared param emits `E_FN_UNKNOWN_TYPE_PARAM`; a non-generic fn lowers to empty `type_params` (INV-6). Unlocks β/γ/ε.* **Crates:** reify-compiler (functions.rs, type_resolution.rs), reify-ir (expr.rs). `grammar_confirmed=true`.

- **β — Call-site type-arg inference (unification) + `substitute_type_params` + return substitution** *(leaf; producer seam).* Add `substitute_type_params` (D3) and `unify` (D2); make `resolve_function_overload` unification-aware for generic candidates (type_compat.rs:335); at the call site (expr.rs:1424) compute the subst and set `result_type = substitute_type_params(return_type, subst)`; emit `E_FN_TYPE_ARG_CONFLICT` / `E_FN_TYPE_ARG_UNRESOLVED`. *Signal: `reify check`/`eval` — `id(5mm)` types as `Length` and evaluates `5mm` (B1); `single(5mm)` types as `List<Length>` and evaluates `[5mm]` (B2); `sample(constant_field(42.0), p)` types as the codomain not a fallback (B3); a conflicting call emits `E_FN_TYPE_ARG_CONFLICT` (B4); an unbound param is tolerated (B5).* **Crates:** reify-compiler (expr.rs, type_compat.rs, new subst/unify helpers). **Deps:** α.

- **γ — Trait-bound validation + permissive generic body checking** *(leaf; bound+body side).* Thread the `CompiledTrait` registry into the call-site bound check; validate inferred args against `T: Trait` via `satisfies_trait_bound`/`check_type_param_bounds` (D5); treat `Type::TypeParam`-typed values as resolution wildcards in generic bodies (D4). *Signal: `reify check` — `fn f<T: Solid>(x:T)->T` rejects a non-conforming arg (bound diagnostic, B6) and accepts a conforming one; a generic body invoking a builtin on a type-param-typed value compiles (D4).* **Crates:** reify-compiler (functions.rs, expr.rs/type_compat.rs, entity.rs reuse). **Deps:** α, β.

- **δ — Tier-1 end-to-end integration gate (THE B+H boundary test + 4218 completion gate)** *(leaf).* Ship `examples/generics/identity.ri` (and a `single<T>`/generic-container example) exercising B1/B2/B5; confirm a generic fn placed in a stdlib `.ri` file type-checks during load (the literal 4218 completion-gate phrasing); pin the INV-2 erasure boundary test (eval type-arg-agnostic, B7/B8) in `reify-expr`/`reify-eval` tests. *Signal: `examples/generics/*.ri` run green in CI via `reify eval` (B1/B2 values assert; B5 checks clean); a generic stdlib `.ri` fn type-checks end-to-end; the INV-2 erasure test passes. This is the §1 Tier-1 signal and the leaf the fields-api `compose` (4224) + the `constant_field` part of `ε` (4223) depend on.* **Crates:** reify-compiler, reify-expr/reify-eval, examples/, reify-cli. **Deps:** α, β, γ.

### Tier 2 — dimension-generics (gated behind Tier 1)

- **ε — Dimension-kinded params: `Dimension` built-in kind-bound + `Scalar<Q>`/`Vector3<Q>`/`Point3<Q>` resolution** *(intermediate; new type-system surface, D7).* Add the dimension-param representation (recommended `Type::ScalarParam(String)`; §12 Q3); recognize `Dimension` as a built-in kind-bound marking a param dimension-kinded; extend the `Scalar`/`Vector3`/`Point3` resolver arms (type_resolution.rs:1401-1417) to admit a dimension-param; emit `E_DIM_PARAM_KIND` for kind misuse. *Signal (intermediate): `fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>` resolves (no "unresolved type" on `Scalar<Q>`, B10); a unit test pins the dim-param-carrying type; a dimension-kinded param used as an ordinary type (or vice-versa) emits `E_DIM_PARAM_KIND`. Unlocks ζ.* **Crates:** reify-core (Type repr), reify-compiler (type_resolution.rs), reify-ir. **Deps:** α. `grammar_confirmed=true`.

- **ζ — Dimension-param call-site inference + substitution + Tier-2 integration gate** *(leaf).* Extend β's unification into the dimension slot (D8): bind `Q` to the concrete `DimensionVector` of the argument scalar's quantity, substitute into the return; ship `examples/generics/dim_param.ri` exercising B9. *Signal: `reify eval` — `scale_q(10mm,3.0) == 30mm` **and** `scale_q(5MPa,2.0) == 10MPa` (B9), the same generic fn at two dimensions, in CI. This is the §1 Tier-2 signal and the leaf the fields-api `clamp_field`/`remap_field`/`threshold` part of `ε` (4223) depend on.* **Crates:** reify-compiler (expr.rs/type_compat.rs, type_resolution.rs), reify-expr/eval, examples/. **Deps:** ε, β, δ.

### Doc

- **η — Spec §3.9 reconcile for generic user functions** *(leaf; doc).* Document the implemented capability: type-level generic fns (threading, conservative inference, return substitution, erasure D1), trait bounds (D5), permissive body checking (D4), and dimension-kinded params (D7); state inference-only call sites (D9) and the `auto`-on-fn deferral (§11). Reference the `guf-*` fixtures. *Signal: `docs/reify-language-spec.md` §3.9 updated; no code change; doc lint passes.* **Crates:** none (docs). **Deps:** δ, ζ.

### Dependency view

```
α ─┬─→ β ─→ γ ─→ δ ──────────┐
   │                          ├─→ η
   └─→ ε ─────────→ ζ ────────┘
                    (ζ also deps β, δ)

cross-PRD re-wire (at decompose):
  fields-api 4224 (compose, T1)                      depends_on  δ        (drop 4218)
  fields-api 4223 (constant_field T1 + clamp/remap/threshold T2)  depends_on  δ, ζ  (drop 4218)
  fields-api 4225 (η)  unchanged (inherits transitively)
  tracking 4218  →  done (authoring complete; PRD committed on main; impl tracked by α–η)
```

Tier 1 (α/β/γ/δ) is independently landable and unblocks `compose` + `constant_field`. Tier 2 (ε/ζ) gates behind Tier 1 and unblocks the `Q: Dimension` field ops.

---

## §10 — Premise validation (G6)

Every §9 leaf signal classified. Domain: this PRD has **no numeric/accuracy/closed-form premise** — it is a type-system feature; G6 branches 1 (numeric bound) and 2 (exactness) do not fire. The relevant branch is 3 (end-to-end capability producible from the task's own dependency set).

- **δ primary signal — end-to-end (`id(5mm)==5mm`, `single(5mm)==[5mm]`, generic stdlib fn type-checks).** Trace: requires (a) type-param threading incl. `resolve_parameterized_builtin_type` [α], (b) call-site unification + return substitution [β], (c) bound/body handling [γ]. All in δ's dependency set (α→β→γ→δ); none owned by a task depending on δ. The arithmetic is identity/list-construction over already-concrete `Scalar<Length>` values — **no new numeric capability**. **Passes** the dependency-set trace.
- **ζ primary signal — dimension-genericity (`scale_q` at two dimensions).** Trace: requires the dimension-param representation [ε] + dimension-slot unification [ζ, extending β] + a Tier-1 gate [δ]. All in ζ's dependency set. `10mm*3.0 == 30mm` / `5MPa*2.0 == 10MPa` is exact dimensioned-scalar multiplication already in the language. **Passes.**
- **Inference decidability premise (D2/D8).** Call-site inference is single-pass structural unification of each `Type::TypeParam` leaf against the supplied concrete arg type — finite, decidable, no recursion across call-sites, no general HM. Strictly *less* than the already-shipped structure-side `auto` resolution. **Decidable — premise holds.**
- **Reuse premises (D0/D5/INV-1).** `convert_type_params`, `resolve_type_with_params`, `satisfies_trait_bound`, `check_type_param_bounds` all exist and are parameterized over `TypeParam`/bounds (verified §3) — fn type-params feed them without a signature change to the resolver. **Reuse sound — premise holds.**
- **`resolve_parameterized_builtin_type` threading premise (D6).** Verified the `empty_type_params` hardcode at `type_resolution.rs:1335` is the reason `Field<D,C>` fails today; α threads the set, exactly as the structure path already does for member types. **No false premise** — this is the precise corrected gap (§0.2).
- **Grammar premise (§3.5).** `tree-sitter parse` 0-ERROR on all four fixtures — mechanically verified 2026-06-02. **No grammar fiction.**
- **`Dimension` substrate premise (Tier 2).** Verified `Dimension` is **not** a trait today, and `Type::Scalar { dimension: DimensionVector }` cannot hold a param — so Tier 2 is correctly scoped as *new representation* (D7), not wiring, and gated. The std-fields-api §3 "`Q: Dimension`" assumed generics would supply this for free; this PRD corrects that — dimension-generics is a distinct, gated tier. **False premise in the consumer PRD surfaced and contained.**
- **α/ε intermediate signals — unit tests on `CompiledFunction`/`Type` shape.** Mechanically verifiable. No quantitative premise.
- **β/γ/ζ diagnostic signals — `E_FN_*`/`E_DIM_PARAM_*` emission.** Illustrative codes (§7.3, §12 Q1); no quantitative premise; pass trivially.

No leaf asserts an accuracy bound, a closed-form reproduction, or a capability owned downstream. **G6 clear.**

---

## §11 — Out of scope

- **Dimension-generics is NOT cut, but gated.** Tier 2 (ε/ζ) is in this PRD; it just lands after Tier 1. What is genuinely out: any *further* dimensional polymorphism beyond `Scalar<Q>`/`Vector3<Q>`/`Point3<Q>` over a single dimension-param (e.g. dimension arithmetic on params `Scalar<Q1*Q2>`).
- **Explicit call-site type-args (turbofish `f<Length>(x)`).** Inference-only in v1 (D9). The consumer relies on inference; explicit type-args are a future addition.
- **`auto`-on-fn-type-param (`fn f<auto T: Trait>(…)`).** The `resolve_auto_type_params` reuse falls out for free (INV-1 plumbing) but is **not** gated by a leaf signal here — no consumer needs it (the fields-api fns infer from value args). Deferred, exactly as the enum PRD defers `auto`-on-enum.
- **Full bounded-operation checking / where-clauses that license operations on a bounded param.** Body checking is permissive (D4); a body that misuses a bounded param is not statically rejected at the definition. A future PRD could add operation-licensing bounds.
- **Monomorphization / runtime type-arg reflection.** Erasure (D1); no per-instantiation `CompiledFunction` clones, no runtime `type_args` on values.
- **Generic *structures*/*traits*/*enums*.** Structures/traits already exist; enums are the sibling PRD (4029/4030/4031). This PRD adds only the **function** form.
- **Higher-kinded / variance on fn type-params** beyond the existing `T: Trait` bound + `T = Default` the `type_parameter` rule already provides.

---

## §12 — Open (tactical) questions

1. **Exact diagnostic codes/strings** (`E_FN_UNKNOWN_TYPE_PARAM`, `E_FN_TYPE_ARG_CONFLICT`, `E_FN_TYPE_ARG_UNRESOLVED`, `E_DIM_PARAM_KIND` illustrative). Decide at α/β/ε against the diagnostic-code registry; reuse the structure-side bound diagnostic rather than minting a new bound code.
2. **`unify`/`substitute_type_params` placement** — a fn-local helper in `reify-compiler`, or a shared module also consumed by the enum PRD's construction-inference (4031). Tactical; either drives the same compile-time check. Coordinate with the enum side if it lands first.
3. **Tier-2 dimension-param representation** — a dedicated `Type::ScalarParam(String)` quantity wrapper (recommended; smallest blast radius — one new `Type` variant, only reached inside dimension-kinded generic fn signatures) vs generalizing the `Scalar`/`Vector`/`Point` quantity slot to `concrete | param` (broader, touches every quantity-slot consumer). Decide at ε. Both satisfy B9/B10.
4. **`single<T>` example body** — `[x]` (list literal) vs a builtin that yields `List<T>`; whichever most cleanly exercises `Type::TypeParam` inside `List<…>` at eval. Decide at β/δ.
5. **Whether δ also adds a generic fn to an existing stdlib `.ri` file** (to literally satisfy "generic *stdlib* fn type-checks") vs leaving the stdlib generic fns to the fields-api consumer (4223/4224). Tactical; δ decides — a self-contained `examples/generics/` file already satisfies the gate.

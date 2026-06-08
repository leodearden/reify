# Capability Manifest — generic user-function support

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/generic-user-functions.md`. Each leaf's user-observable signal is decomposed into the capabilities it asserts; each capability binds to evidence. **Any FAIL binding blocks the batch.** Evidence verified on main 2026-06-02 + grammar fixtures committed at `tree-sitter-reify/test/fixtures/guf-*.ri`.

Sentinel (field-population check): **N/A** — this is a type-system feature, not a result-field producer; no `Value::Undef` population claim. Numeric floor: **N/A** — no accuracy/exactness premise (G6 §10). The governing G6 branch is 3 (end-to-end capability producible from the task's own dependency set). Grammar reality: **0-ERROR on all fixtures** — `function_definition` already carries `optional($.type_parameters)` (`grammar.js:193`); `grammar_confirmed=true` on every leaf, **no grammar-producer dependency** (contrast the enum sibling's `producer:DCE-3936`).

`α` and `ε` are intermediate (no leaf signal); their deliverables (type-param threading / dimension-param representation) are the upstream `producer:α` / `producer:ε` evidence cited below.

---

## α — thread fn type-params through signature resolution + `CompiledFunction.type_params`  *(Tier 1; intermediate)*

Signal (intermediate): `constant_field<D,C>` lowers to a `CompiledFunction` with `type_params==[D,C]` and `Type::TypeParam` in `params`/`return_type` **including inside `Field<…>`**; undeclared-param ref → `E_FN_UNKNOWN_TYPE_PARAM`; non-generic fn → empty `type_params` (INV-6).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `fn f<…>` parses (type-param head + bounded + `Field<D,C>`/`Scalar<Q>` sig + lambda body) | grammar reality | `grammar-fixture:tree-sitter-reify/test/fixtures/guf-{1,2,3,4}.ri` parse 0-ERROR; `grammar.js:193` (`optional($.type_parameters)`) | PASS |
| name→`Type::TypeParam` when in the in-scope set | capability→producer (wired) | `grep:reify-compiler/src/type_resolution.rs:591-602` (`resolve_type_with_params` returns `Type::TypeParam(name)`) | PASS |
| structure threading pattern to mirror | capability→producer (wired) | `grep:reify-compiler/src/entity.rs:560-564` (build `HashSet<String>` from `type_params`, pass to `resolve_type_expr_with_aliases`) | PASS |
| AST→IR type-param conversion | capability→producer (wired) | `grep:reify-compiler/src/type_resolution.rs:1877` (`convert_type_params`); IR `TypeParam` `traits.rs:30` (as `TraitDef.type_params:94`) | PASS |
| inner type-args of `Field<D,C>`/`List<T>` resolve to `Type::TypeParam` | capability→producer (the second-site fix) | **`empty_type_params` hardcoded** at `grep:reify-compiler/src/type_resolution.rs:1335` is why `Field<D,C>` fails today → **α threads the set** = `producer:α` (in-scope, D6) | PASS (in-scope fix) |
| `CompiledFunction.type_params` field | capability→producer (DAG) | ABSENT today (`reify-ir/src/expr.rs:245-292` has no `type_params`) → **α adds it** = `producer:α` | PASS (in-scope) |

## β — call-site inference (unification) + `substitute_type_params` + return substitution  *(Tier 1; leaf)*

Signal: `id(5mm)` types `Length`, evals `5mm` (B1); `single(5mm)` types `List<Length>`, evals `[5mm]` (B2); `sample(constant_field(42.0), p)` types codomain not fallback (B3); conflict → `E_FN_TYPE_ARG_CONFLICT` (B4); unbound param tolerated (B5).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| generic fn signature (`type_params` + `Type::TypeParam` slots) | capability→producer (DAG) | `producer:α` upstream | PASS |
| call-site has the matched fn's declared param/return types | capability→producer (wired) | `grep:reify-compiler/src/expr.rs:1424` (`result_type = matched_fn.return_type.clone()` — the substitution hook); `grep:reify-compiler/src/type_compat.rs:335-396` (`resolve_function_overload`) | PASS |
| single-pass unification (not HM, not auto-resolution) | capability→producer (DAG) + premise | **`unify` is NEW** = `producer:β`; decidable single-pass (G6 §10); the sibling enum PRD builds the analog (`task-4031`) — `resolve_auto_type_params` is candidate-enumeration, NOT this (§0.3) | PASS (in-scope; decidable) |
| type-substitution walk over a resolved `Type` | capability→producer (DAG) | **no generic walk exists**; alias `_with_subst` `HashMap<String,Type>` pattern at `grep:reify-compiler/src/type_resolution.rs:1192` is the model → **β adds `substitute_type_params`** = `producer:β` | PASS (in-scope) |
| value arithmetic in B1/B2 | numeric floor | identity / list-construction over already-concrete `Scalar<Length>` — **no new numeric capability** | PASS (no floor) |

## γ — trait-bound validation + permissive generic body checking  *(Tier 1; leaf)*

Signal: `fn f<T: Solid>(x:T)->T` rejects non-conforming arg (bound diagnostic, B6), accepts conforming; generic body invoking a builtin on a type-param-typed value compiles (D4).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| bound-satisfaction check for a concrete type vs `T: Trait` | capability→producer (wired) | `grep:reify-compiler/src/entity.rs:3732` (`satisfies_trait_bound`), `:3639` (`check_type_param_bounds`) — exist for structures, reused | PASS |
| `CompiledTrait` registry reachable at the call-site check | capability→producer (DAG) | `compile_function` takes `trait_names` but not the trait registry (`functions.rs:6`); **γ threads the registry** (callers `compile_builder/{traits_phase.rs:168,functions_phase.rs:80}`) = `producer:γ` | PASS (in-scope) |
| `Type::TypeParam` as a body resolution wildcard | capability→producer (mirror) | precedent: trait-typed params already wildcard at `grep:reify-compiler/src/type_compat.rs:361` (`type_carries_trait_object`); γ extends the same relaxation to `Type::TypeParam` = `producer:γ` | PASS (in-scope) |

## δ — Tier-1 end-to-end integration gate  *(Tier 1; leaf; the 4218 completion gate)*

Signal: `examples/generics/*.ri` run green in CI via `reify eval` (B1/B2 assert, B5 clean); a generic stdlib `.ri` fn type-checks end-to-end; INV-2 erasure test passes (B7/B8).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| α/β/γ landed | capability→producer (DAG) | `producer:task-{α,β,γ}` upstream | PASS |
| `.ri` example runs in CI via `reify eval` | capability→producer (wired) | `grep:examples/` + `reify eval` CLI path (precedent: existing `examples/m*.ri` in CI) | PASS |
| eval is type-arg-agnostic (erasure) | field-population / anti-inversion | erasure (D1): generic-fn call eval consumes already-concrete `Value`s — **no eval change**; INV-2 boundary test in `reify-expr`/`reify-eval` = `producer:δ`. Containment NOT downstream of δ | PASS |

## ε — dimension-kinded params: `Dimension` kind-bound + `Scalar<Q>` resolution  *(Tier 2; intermediate)*

Signal (intermediate): `fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>` resolves (no "unresolved type" on `Scalar<Q>`, B10); kind misuse → `E_DIM_PARAM_KIND`.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `Q: Dimension` parses | grammar reality | `grammar-fixture:tree-sitter-reify/test/fixtures/guf-2.ri` (bounded `Q: Dimension`) parses 0-ERROR | PASS |
| dimension-param representable in the quantity slot | capability→producer (DAG; NEW repr) | **ABSENT** — `Type::Scalar { dimension: DimensionVector }` is concrete-only (`grep:reify-compiler/src/type_resolution.rs:1401-1406`, `resolve_type_alias_expr_to_dimension`); `Dimension` is **not** a trait (verified) → **ε adds the repr (D7)** = `producer:ε`. *New type-system surface, gated behind Tier 1, acknowledged not-wiring.* | PASS (in-scope; new repr, gated) |
| type-param threading | capability→producer (DAG) | `producer:α` upstream | PASS |

## ζ — dimension-param call-site inference + Tier-2 integration gate  *(Tier 2; leaf)*

Signal: `scale_q(10mm,3.0)==30mm` **and** `scale_q(5MPa,2.0)==10MPa` (B9) — same generic fn at two dimensions, in CI.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| dimension-param representation | capability→producer (DAG) | `producer:ε` upstream | PASS |
| call-site unification + return substitution | capability→producer (DAG) | `producer:β` upstream (ζ extends it into the dimension slot, D8) | PASS |
| Tier-1 gate landed | capability→producer (DAG) | `producer:δ` upstream | PASS |
| `10mm*3.0==30mm` / `5MPa*2.0==10MPa` | numeric floor | exact dimensioned-scalar multiplication already in the language — **no new numeric capability** | PASS (no floor) |

## η — spec §3.9 reconcile  *(leaf; doc)*

Signal: `docs/reify-language-spec.md` §3.9 updated; no code change; doc lint passes.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| δ + ζ landed (describe what shipped) | capability→producer (DAG) | `producer:task-{δ,ζ}` upstream | PASS |
| spec file + fixtures exist to cite | capability→producer (wired) | `docs/reify-language-spec.md` §3.9; `tree-sitter-reify/test/fixtures/guf-*.ri` | PASS |

---

## Gate result

**No FAIL bindings.** Every "ABSENT on main" capability is an **in-scope producer** of the very leaf that asserts it (`producer:α` type-param threading + `CompiledFunction.type_params`; `producer:β` `unify`/`substitute_type_params`; `producer:γ` registry-threading + `Type::TypeParam` wildcard; `producer:ε` dimension-param representation), not a downstream/absent dependency. The one genuinely new type-system surface (the Tier-2 dimension-param representation, D7) is explicitly gated behind Tier 1 and flagged as not-wiring. Grammar is 0-ERROR on committed fixtures with **no grammar-producer dependency**. No numeric/exactness premise exists. Batch is clear to queue.

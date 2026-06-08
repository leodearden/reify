# Capability manifest: real-dimensionless-unification

Binds each task's asserted capabilities to evidence (mechanizing G3 + G6). Any FAIL blocks the batch.
All tasks `grammar_confirmed=true` — **no novel grammar**; `Real`/`Dimensionless`/`Scalar<Q>`/`Vector3<Q>` all parse today, and bare-`Scalar` removal is a resolution-level rejection of an already-parsing form (verified: `Tensor<2,3,Real>` parses; probe corpus `/tmp/ri-probes`).

## δ — corpus bare-Scalar migration (behavior-preserving)
- **bare `Scalar == Length` today** (the semantics-preservation premise) → PASS `grep:crates/reify-compiler/src/type_resolution.rs:562` (`"Scalar" => Some(Type::length())`). Migration `: Scalar` → `Length`/`Scalar<Length>` is value-identical; corpus stays green.
- **migration completeness** → PASS-criterion: post-task `grep ': Scalar[^<a-zA-Z]'` over `examples/`, `crates/**/stdlib/*.ri`, inline `.rs` fixtures returns 0.
- DAG-direction: upstream of α, γ. ✓

## α — delete `Type::Real` → `Scalar{DIMENSIONLESS}`
- **`Type::dimensionless_scalar()` exists** (the replacement constructor) → PASS `grep:crates/reify-core/src/ty.rs:227`.
- **canonical bridge site** (dimensionless literal = `Value::Real` + `Scalar{DIMENSIONLESS}` type) → PASS `grep:crates/reify-compiler/src/expr.rs:719` (today `(Value::Real(f), Type::Real)`; α rewrites the type half).
- **`is_representable_cell_type` admits `Scalar`** (so a `Scalar{DL}` cell holding `Value::Real` is representable) → PASS `grep:crates/reify-eval/src/engine_eval.rs:68` (predicate exists; admits `Scalar { .. }`). α must keep this true — leak-guard in η (invariant T).
- **no `Value::Real`↔`Type::Real` coupling assert** (asymmetric canon is safe) → PASS: grep for such an assert returned none (engine_eval/value.rs).
- DAG-direction: upstream of β, γ, η (ε folded into 4318). Cross-batch seam: `4234 depends_on α` (Leo, 2026-06-08). ✓

## β — value-layer chokepoint
- **`Value::from_real_scalar` collapses dimensionless→Real** (the chokepoint) → PASS `grep:crates/reify-ir/src/value.rs:1045` (returns `Value::Real` when `is_dimensionless()`).
- **producers currently bypass it** (the gap β closes) → PASS `grep:crates/reify-expr/src/lib.rs:2718` (`eval_mul` Scalar×Scalar builds `Value::Scalar{ad.mul(bd)}` un-collapsed; the comment `:2746` "Intentionally returns Scalar{dimension} even when dimension is DIMENSIONLESS" marks the leak), `eval_div`/`eval_pow` likewise; plus external geometry/tolerance/modal producers in reify-eval.
- **RE-SCOPED at decompose (task 4319 DONE, merged `a2348f81`):** the additive `(Real,Scalar{DL})`/`(Scalar{DL},Real)` arms in `eval_add`/`eval_sub` are **already on main** (`grep:crates/reify-expr/src/lib.rs:2799,2805` `if dimension.is_dimensionless()`); β no longer re-fixes the additive `Undef` (4319 did). β = route the multiplicative + external **producers** through `from_real_scalar` so `Value::Scalar{DL}` is never constructed, add the leak-guard test, and simplify 4319's now-defensive additive arms. Does **not** re-file 4319.
- **dead consumer guards to delete** → PASS `grep:crates/reify-expr/src/lib.rs:3225,3269` (eval_eq/eval_cmp dimensionless arms), `crates/reify-stdlib/src/fea.rs:190`, `crates/reify-eval/src/compute_targets/elastic_static.rs:1303`.
- **G6 field-population:** N/A — β reads/produces scalar arithmetic, not result-fields.
- DAG-direction: upstream of η; downstream of α. ✓

## γ — grammar-resolution unification
- **resolution sites exist** → PASS `grep:crates/reify-compiler/src/type_resolution.rs:573` (`"Real"=>Type::Real`, γ retargets), `:562` (`"Scalar"=>length()`, γ removes), `:887` `resolve_type_alias_expr_to_dimension` (γ adds `Real` synonym arm).
- **`E_BARE_SCALAR` new code** → PASS substrate: `grep:crates/reify-core/src/diagnostics.rs:156` (`enum DiagnosticCode`); append a variant (pattern exists, e.g. `:371 DimensionMismatch`). No grammar work.
- **`Vector3<Real>` parses** (only resolution rejected it) → PASS: probe `/tmp/ri-probes/p2` — error was "cannot resolve 'Real' to a dimension type" (resolution, not parse). γ fixes resolution.
- DAG-direction: upstream of ζ, η; downstream of α, δ. ✓

## ε — struct-param default type-check — **DROPPED at decompose; folded into task 4318 (PENDING)**
ε's `param t : Length = 1.0` default-literal case is a subset of 4318 ("declared-type vs initializer-dimension cross-check at the declaration", same `FnParamDefaultTypeMismatch` mechanism; 4318's corpus-sweep note already enumerates literal-default firing sites). Not filed separately. Substrate below remains valid evidence for 4318.
- **reuse existing diagnostic** → PASS `grep:crates/reify-core/src/diagnostics.rs:344` (`FnParamDefaultTypeMismatch`).
- **strict-equality helper exists** → PASS `grep:crates/reify-compiler/src/type_compat.rs:280` (`fn_param_default_compatible`, exact-equality; 4318 reuses for struct params).
- **the hole is real** → PASS: probe `/tmp/ri-probes/p5` — `param t : Scalar = 1.0` accepted, `t=1`, `t + 5mm` = `undef` silently.
- **G6:** signal asserts a diagnostic emission (no numeric premise) → trivial PASS.

## ζ — doc/style reconcile (LEAF, doc-reconcile)
- **target sections exist** → PASS `grep:docs/reify-language-spec.md:219,226,330` (alias claims), `docs/reify-stdlib-reference.md` §1.2 (trig `-> Real`). Doc-only; no substrate.

## η — integration-gate example in CI (LEAF)
- **closed-form exactness (G6 branch 2)** → PASS: `lead=2mm`, ratios `3.0+1.5=4.5`, `groove_len = 2mm*4.5 = 9mm = 0.009 m` — exact in f64 (all operands and the product are exactly representable; `0.002*4.5=0.009` holds bit-exactly). Exact because post-β both `lead*(a+b)` and `lead*a+lead*b` are the same float ops. No method floor (not a numerical-method bound).
- **capability: mixed Real/Dimensionless + Vector3<Real> + groove_len eval** → all delivered by upstream α/β/γ (in η's transitive dependency closure), never by a task depending on η. ✓ DAG-direction PASS.
- **field-population:** N/A — eval of scalar/vector arithmetic, no result-field sampling.
- **wired-on-main:** the example runs through the production `reify eval` CLI path + the CI example-check harness (the project's standard `.ri`-example-in-CI signal, overlay G2 menu).

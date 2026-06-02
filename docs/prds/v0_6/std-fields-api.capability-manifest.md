# Capability Manifest — std.fields §11

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/std-fields-api.md`. Each leaf's user-observable signal is decomposed into the capabilities it asserts; each capability binds to evidence. **Any FAIL binding blocks the batch.** Evidence verified on main (HEAD `57c08122fc`) + grammar fixtures in `/tmp/prd-fields-fixtures/`.

Sentinel (field-population check): `Value::Undef`. Production entry paths: `reify-expr/src/lib.rs` FunctionCall dispatch, `reify-eval/src/engine_eval.rs` field elaboration, `reify-compiler/src/expr.rs` result-type table, `reify-compiler/stdlib/*.ri`.

`α` is intermediate (consumers β/γ/δ/ε/ζ) — no leaf signal; its deliverable (the typing table + source-kind scaffolding) is the upstream `producer:task-α` evidence cited below.

---

## β — `fn_field` native primitive  *(Tier 1)*

Signal: `examples/fields/fn_field.ri` — `sample(fn_field(|p| 2.0*p), 3.0) == 6.0` via `reify eval`.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `FieldSourceKind::Analytical` + lambda sample dispatch | capability→producer (wired) | `grep:reify-expr/src/lib.rs:205` (`Value::Lambda` arm of `sample`) | PASS |
| lambda literal as a call argument | grammar reality | `grammar-fixture:/tmp/prd-fields-fixtures/e1_lambda_arg.ri` parses 0-ERROR; `grammar.js:1029` | PASS |
| `fn_field` typed as `Field<D,C>` (not first-arg fallback) | capability→producer (DAG) | `producer:task-α` upstream (§5.1 table) | PASS |
| intercepting-builtin arm | capability→producer | `grep:reify-expr/src/lib.rs:195` (FunctionCall match); β adds its arm — `producer:β` | PASS |

## γ — `from_samples` + `InterpolationMethod` enum  *(Tier 1)*

Signal: gridded sample interpolates (B2); non-grid → `E_FIELD_SAMPLES_NOT_GRID` (B3); `RBF` → `E_INTERP_METHOD_UNSUPPORTED` (B4).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| gridded `SampledField` builder | capability→producer (wired) | `grep:reify-eval/src/engine_eval.rs:971` (`build_sampled_field`); sampled dispatch `lib.rs:211` | PASS |
| internal `InterpolationKind` carrier | capability→producer | `grep:reify-ir/src/value.rs:926` | PASS |
| Reify `enum` decl + qualified-variant call arg | grammar reality | `grammar-fixture:/tmp/prd-fields-fixtures/e4_enum_and_variant_arg.ri` parses+checks 0-ERROR; lowering `grep:reify-compiler/src/expr.rs:2692` | PASS |
| gridded-only domain (not scattered) | numeric/capability floor | floor stated (D3): `interp.rs` is regular-grid-only → `from_samples` validates grid, non-grid diagnoses; **bound = gridded, not arbitrary scattered** | PASS |
| `from_samples` typed as `Field<D,C>` | capability→producer (DAG) | `producer:task-α` upstream | PASS |

## δ — `restrict` full-solid + geometry-containment dispatch seam  *(Tier 1; B+H seam)*

Signal: `examples/fields/restrict.ri` — field restricted to `box(...)`; inside-point sample == inner value, outside-point → `Undef` (B5).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `FieldSourceKind::Restricted` + sample arm | capability→producer (DAG) | `producer:task-α` upstream (§5.2) | PASS |
| point-in-solid containment reachable from sample path | capability→producer (the seam) | `contains` EXISTS in `reify-eval` geometry-query layer (`geometry_ops`) but **NOT reachable from `reify-expr`** (no geom dep, Cargo.toml). **δ delivers the bridge** (§5.3 relocate/callback) → `producer:δ` (in-scope, NOT downstream/absent) | PASS (highest-risk; in-scope) |
| region geometry value (`box(...)`) | capability→producer (wired) | `grep:reify-stdlib` geometry ctors (`box` exists on main) | PASS |
| DAG-direction (containment not downstream of δ) | anti-inversion | containment producer is existing `reify-eval`; δ bridges it — not `producer-downstream` | PASS |

## ε — composable spatial ops `constant_field`/`clamp_field`/`remap_field`/`threshold`  *(Tier 2)*

Signal: `examples/fields/spatial_ops.ri` — clamp/remap/threshold sampled & asserted, incl. `Field<_,Bool>` (B6/B7/B8).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| **generic user `fn` resolution** (`<D, Q: Dimension>`, return-type substitution) | capability→producer (DAG) | **ABSENT on main** (`FnDef.type_params` parsed-not-read, `functions.rs:16`) → resolved by `producer:task-G` upstream (hard cross-task dep). *Without the dep this is `producer-absent` (FAIL); the dep makes it PASS.* | PASS (via dep on G) |
| `fn_field` primitive | capability→producer (DAG) | `producer:task-β` upstream | PASS |
| scalar `clamp` / `remap` | capability→producer (wired) | `grep:reify-stdlib/src/numeric.rs:101` (clamp), `:219` (remap) | PASS |
| lambda captures enclosing fn params | capability→producer | `grep:reify-compiler/src/expr.rs:2991-3000` (free-var capture) + `functions.rs:155-161` (params in scope) — mechanism verified | PASS |
| `threshold` → `Field<D, Bool>` codomain | field-population | `Value::Bool` exists; Analytical sample applies lambda returning Bool — non-sentinel | PASS |

## ζ — callable `compose(f,g)`  *(Tier 2)*

Signal: `examples/fields/compose.ri` — `sample(compose(f,g), p) == sample(f, sample(g,p))` (B9).

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| generic user `fn` resolution | capability→producer (DAG) | `producer:task-G` upstream (as ε) | PASS (via dep on G) |
| `fn_field` primitive | capability→producer (DAG) | `producer:task-β` upstream | PASS |
| nested `sample` inside a fn-body lambda | capability→producer | fn-body lambda inherits scope + captures (`expr.rs:2920/2991`); the e3 `unresolved name` was field-def-block-only empty scope (`functions.rs:597`), NOT fn-body — verified | PASS |

## η — full-surface integration gate + doc/gap-register reconcile  *(leaf; B+H integration)*

Signal: §6 boundary-test sketch runs green in CI (`examples/fields/std_fields_surface.ri` via `reify eval`); gap-register P16 rows + InterpolationMethod/compose doc-reconcile rows marked closed.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| all of β/γ/δ/ε/ζ landed | capability→producer (DAG) | `producer:task-{β,γ,δ,ε,ζ}` upstream | PASS |
| `.ri` example runs in CI via `reify eval` | capability→producer (wired) | `grep:examples/fields/composed_stiffness.ri` (precedent exists) | PASS |
| doc + gap-register files exist | capability→producer | `docs/reify-stdlib-reference.md` §11, `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` | PASS |

---

## Gate result

**No FAIL bindings.** The single binding that would FAIL as `producer-absent` — the Tier-2 ops' reliance on generic user-`fn` resolution — is resolved by the explicit hard upstream dependency on tracking task **G** (the generics PRD), per G3 resolution (b). The δ containment binding is `producer:δ`-in-scope (δ delivers the bridge), not absent/downstream. Batch is clear to queue.

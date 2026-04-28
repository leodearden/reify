# Field Source Kinds — PRD

> **Scope** This document specifies the compile-time and runtime semantics of the
> four `field def` source variants: `analytical`, `sampled`, `composed`, and
> `imported`.
>
> **v0.1 coverage per task:**
> - § Analytical — type-checking & sampling semantics (**this file; task 2336**)
> - § Composed — composition chain type-checking (task 2343, TBD)
> - § Imported — v0.2 deferral diagnostic (task 2344, TBD)
> - § Sampled — v0.1 deferral diagnostic (task 2416); v0.2 implementation (task 2341)
> - § Cross-cutting smoke tests (task 2346, TBD)

---

## § Analytical source kind

### Syntax

```
field def F : D -> C {
    source = analytical { |p| body }
}
```

The lambda `|p| body` is the *sampling function*. `D` is the domain type and
`C` is the codomain type. The lambda may use a single parameter (bound to the
entire sample point) or multiple parameters (one per spatial component of a
structured domain — see *Sampling semantics* below).

---

### Compile-time codomain type-check

**Rule:** The inferred type of `body` must implicitly convert to the declared
codomain `C`. If not, the compiler emits a `FieldCodomainMismatch` diagnostic
(mnemonic `E_FIELD_CODOMAIN_MISMATCH`).

| Concept | Location |
|---------|----------|
| `DiagnosticCode::FieldCodomainMismatch` | `crates/reify-types/src/diagnostics.rs` |
| Emit site | `crates/reify-compiler/src/functions.rs::compile_field` — `FieldSource::Analytical` arm |
| Compatibility predicate | `crates/reify-compiler/src/type_compat.rs::implicitly_converts_to` |

The check uses `implicitly_converts_to(body_result_type, codomain_type)` rather
than strict equality. This inherits the asymmetric anti-cascade contract
(task-1918): a poisoned body type (`Type::Error`) silently converts to any
declared codomain, preventing a follow-on mismatch diagnostic from shadowing the
root-cause body error.

**Worked mismatch example:**

```
// Declared codomain:  Scalar<Temperature>
// Lambda body returns Scalar<Length> (1.0m literal)
field def heat_field : Real -> Temperature {
    source = analytical { |x| 1.0m }   // ERROR: Scalar<Length> != Scalar<Temperature>
}
```

Emitted diagnostic (severity `Error`, code `FieldCodomainMismatch`):

```
field 'heat_field' codomain mismatch: declared codomain `Temperature`, lambda body produces `Scalar[m]`
  --> declared codomain
```

**Suppression conditions (no diagnostic emitted):**
- Body type is `Type::Error` (anti-cascade — a prior body error already reported)
- Codomain type is `Type::Error` (anti-cascade — domain resolution already failed)

---

### Sampling semantics

`sample(field, point)` invokes the compiled lambda with `point` as the argument:

- **1-param lambda** — the entire `point` value (whether `Value::Point`,
  `Value::Vector`, or a scalar) is bound to the single parameter.
- **multi-param lambda** (`params.len() > 1`) — if `point` is a `Value::Point`
  or `Value::Vector` whose element count equals `params.len()`, the components
  are *unpacked* into individual scalar arguments (one per parameter). If the
  lengths differ, or the input is a scalar, `apply_lambda` receives the whole
  point as a single argument and returns `Value::Undef` via its arity check.

The calling convention is documented on `Value::Field.lambda` in
`crates/reify-types/src/value.rs`. The dispatch implementation lives in
`crates/reify-expr/src/lib.rs::eval_expr` (the `"sample"` arm, with
`apply_lambda_with_point_unpacking` delegating to `apply_lambda`).

---

### Kleene three-valued logic for undef propagation

`sample` and the analytical lambda body respect Kleene three-valued logic (True /
False / Undef) throughout evaluation. The following rules are enforced:

1. **Undef sample argument → Undef result.**  
   Before dispatching `sample`, `eval_expr` checks whether *any* evaluated
   argument is `Value::Undef`. If so, `Value::Undef` is returned immediately
   without invoking the lambda body.  
   *Implementation:* `evaluated_args.iter().any(|v| v.is_undef())` guard at
   `crates/reify-expr/src/lib.rs` (line ~119).

2. **Undef captured in lambda environment → Undef result.**  
   If a captured variable in the lambda's closure evaluates to `Value::Undef`,
   any expression that reads that variable will propagate Undef through the
   per-op Kleene rules (rule 3 below).

3. **Undef arising mid-body (per-op rule) → Undef propagated.**  
   Arithmetic and comparison operators short-circuit to `Value::Undef` when
   either operand is Undef (strict Kleene propagation). Examples: division by
   zero produces `Value::Undef`; any arithmetic on Undef produces `Value::Undef`.  
   *Implementation:* `eval_binop` in `crates/reify-expr/src/lib.rs`.

4. **Kleene boolean shortcuts inside the lambda body.**  
   Logical `and`/`or` follow Kleene shortcuts:
   - `false AND undef = false`  
   - `true OR undef = true`  
   These are implemented as explicit `Bool(false)` / `Bool(true)` early-outs in
   the `and`/`or` dispatch arms of `eval_expr`.

**Regression tests** pinning rules 1 and 3:
- `crates/reify-expr/tests/field_eval_tests.rs::sample_propagates_undef_point_argument`
  — pins rule 1 (Undef argument short-circuit).
- `crates/reify-expr/tests/field_eval_tests.rs::sample_propagates_undef_from_lambda_body_division_by_zero`
  — pins rule 3 (division-by-zero in body propagates Undef through the sample result).

---

## § Sampled source kind

> **v0.1**: deferral diagnostic implemented (task 2416).
> **v0.2**: implementation in progress (task 2341); interpolation primitives
> already landed via task 2338 (`crates/reify-expr/src/interp.rs`).

A field with a `sampled` source declares discrete point-value data with an
interpolation strategy. v0.1 supports `analytical` and `composed` only — the
compiler emits a `FieldSampledV02` diagnostic (mnemonic `E_FIELD_SAMPLED_V02`)
when a `sampled { ... }` source is encountered.

v0.2 implements the full sampling pipeline:

- **Grid kinds** — `RegularGrid1`, `RegularGrid2`, `RegularGrid3`, parameterised
  by `BoundingBox` bounds and per-axis `Length` spacing.
- **Interpolation** — uses the `InterpolationMethod` enum from
  `crates/reify-expr/src/interp.rs` (Linear, NearestNeighbor, Cubic; RBF and
  Kriging emit `W_INTERPOLATION_DEFERRED` and fall back to Linear).
- **Out-of-bounds policy** — sample lookups outside the grid return
  `Value::Undef` and emit `W_FIELD_OUT_OF_BOUNDS` once per field per session.
- **Config syntax** — `grid = ...`, `interpolation = ...`, `data = ...`
  key-value pairs inside the `sampled { ... }` block; parsed in
  `crates/reify-compiler/src/functions.rs::compile_field` (Sampled arm) and
  materialised at runtime in `crates/reify-eval/src/engine_eval.rs`.

When v0.2 lands, the `FieldSampledV02` compile-time error is removed and the
existing `CompiledFieldSource::Sampled` arm replaces the v0.1 `Value::Undef`
fallback.

---

## § Composed source kind

> **TBD — task 2343**

A field with a `composed` source applies a lambda that composes other named
fields (e.g., `f2(f1(p))`). The compiler validates that the codomain of each
inner field call is compatible with the domain of the outer call.
Full specification deferred to task 2343.

---

## § Imported source kind

> **TBD — task 2344** (v0.2 deferral diagnostic already implemented)

A field with an `imported` source declares that field data will be loaded from
an external file (e.g., a VTK `.vtu` mesh). In v0.1 the compiler emits a
`FieldImportedV02` diagnostic (mnemonic `E_FIELD_IMPORTED_V02`) to indicate
that this feature is deferred to v0.2. Full import-pipeline specification
deferred to task 2344.

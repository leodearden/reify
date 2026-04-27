# Money Dimension — PRD

> **Scope** This document specifies the compile-time and runtime semantics of the
> money dimension (slot 9 of `DimensionVector`) and its stdlib bindings.
>
> **v0.1 coverage per task:**
> - § Overall money dimension design — base implementation, DimensionVector slot 9 (**this file; ground-truth**)
> - § Stdlib unit declaration `unit USD : Money` — task 2378 (done)
> - § Quantity-literal & currency-mass arithmetic — task 2379 (in-flight)
> - § Dimension-mismatch diagnostic (Money vs Force) — task 2380 (in-flight)
> - § Cost-aggregation stdlib idiom & canonical example — task 2381 (in-flight)
> - § Source-form unit display in errors & LSP hovers — task 2382 (in-flight)
> - § Acceptance / regression sweep — task 2383 (in-flight)

---

## § Overall money dimension design

### Slot mapping

See `docs/reify-language-spec.md` §3.2 for the authoritative definition. The 10-slot
vector is:

```
[Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle, SolidAngle, Money]
  0       1     2    3        4             5       6           7      8           9
```

Money occupies **slot 9** (the 10th and final base dimension). All monetary arithmetic
uses this slot exclusively; physical dimensions occupy slots 0-8 and are unaffected by
monetary operations.

**Worked example — CostPerMass:**

```
CostPerMass = Money * Mass^-1
            = [0, -1, 0, 0, 0, 0, 0, 0, 0, 1]
                 ^                            ^
              Mass=-1                    Money=+1
```

Multiplication adds exponent vectors; division subtracts. Checked at compile time with
zero runtime cost (see spec §3.2).

### Design rationale

From `docs/reify-language-spec.md` §3.2:

> All monetary values within a project use **constant conversion factors**. Time-varying
> exchange rates are out of scope. Enables expressions like `25USD/kg` for cost
> estimation. Money composes with physical dimensions via multiplication/division like
> any other dimension.

The consequence is that a project may declare `unit GBP : Money = 1.27` to express a
fixed GBP/USD rate at project-configuration time, but live market data is not modelled.

### Implementation pointer table

| Concept | Location |
|---------|----------|
| `DimensionVector` struct (10 slots) | `crates/reify-types/src/dimension.rs` lines 102–116 |
| Slot-9 index comment in struct doc | `crates/reify-types/src/dimension.rs` line 114 |
| `DimensionVector::MONEY` const | `crates/reify-types/src/dimension.rs` line 129 |
| `DimensionVector::mul` (add exponents) | `crates/reify-types/src/dimension.rs` lines 225–231 |
| `DimensionVector::div` (subtract exponents) | `crates/reify-types/src/dimension.rs` lines 234–240 |
| `to_display_units` — Money branch | `crates/reify-types/src/dimension.rs` lines 280–281 |
| `Display` impl name table (`"USD"` at index 9) | `crates/reify-types/src/dimension.rs` lines 312–340 |

---

## § Stdlib unit declaration `unit USD : Money` — task 2378 (done)

### Syntax

```
unit USD : Money
```

No `= scale` body is required for a base monetary unit. The spec syntax is defined in
`docs/reify-language-spec.md` §4.6:

```
unit mm  : Length = 0.001m      // scaled length unit
unit USD : Money                // base monetary unit — no scale body
unit degC : Temperature offset 273.15K
```

USD is the project's canonical base monetary unit (scale factor 1 by definition). Future
currencies are declared as:

```
unit GBP : Money = 1.27         // constant GBP/USD rate at project configuration time
unit EUR : Money = 1.09
```

### Implementation pointer table

| Concept | Location |
|---------|----------|
| Declaration site | `crates/reify-compiler/stdlib/units.ri` |
| Regression tests | `crates/reify-compiler/tests/money_units_tests.rs` |

### Notes

- USD is not present in `units.ri` at the worktree base; task 2378 lands the declaration
  on a sibling branch. This PRD documents the target state as the canonical contract.
- The compiler reads `std.units` at a bootstrap stage before parsing user code (spec §4.6).
  `unit USD : Money` therefore enters scope for all user files without an explicit import.
- **Conversion factors must be compile-time constants.** A declaration of the form
  `unit GBP : Money = exchange_rate()` (i.e. a non-constant initialiser) is a static
  error: the unit-declaration scale factor must be a constant expression. The
  unit-decl error path enforces this; live FX is structurally excluded.

---

## § Quantity-literal & currency-mass arithmetic — task 2379 (in-flight)

> **TBD — task 2379**

### Spec references

- `docs/reify-language-spec.md` §2.6 — quantity literals (`25USD` lexical form; no space
  between number and unit)
- `docs/reify-language-spec.md` §2.7 — unit expressions (`25USD/kg` cost-per-unit-mass;
  `/` subtracts exponents)

### Dimensional-arithmetic invariants

The integration tests owned by task 2379 must enforce:

| Expression | Expected result dimension |
|------------|--------------------------|
| `25USD * 4kg` | `Money·Mass` (slot 9 +1, slot 1 +1) |
| `25USD / 1kg` | `Money·Mass^-1` (CostPerMass) |
| `25USD / 25USD` | `dimensionless` |
| `25USD * 25USD` | `Money^2` |
| `DimensionVector::LENGTH.mul(&DimensionVector::MASS)` slot 9 | `0` (purity) |

The purity invariant — Money does not leak into arithmetic that doesn't involve a
monetary quantity — is pinned at the type level by `money_does_not_leak_into_unrelated_arithmetic`
(see regression-pin index below) and must also hold at the eval level.

### Worked example

```
let cost_per_kg : Scalar<Money/Mass> = 25USD/kg
let quantity    : Scalar<Mass>       = 4kg
let total       : Scalar<Money>      = cost_per_kg * quantity  // types to USD
```

`total` has dimension `Money·Mass^-1 * Mass = Money`. The compiler verifies this
statically; no runtime cost.

### Target test file

`crates/reify-eval/tests/money_arithmetic_tests.rs` (owned by task 2379)

---

## § Dimension-mismatch diagnostic (Money vs Force) — task 2380 (in-flight)

> **TBD — task 2380**

### Rule

When the two operand dimensions have a non-trivial Money-vs-non-Money difference, the
diagnostic must name the user-visible unit (`"USD"` or the composite dimension string)
on the monetary side and the offending mechanical dimension on the other side. Raw
exponent vectors (e.g. `[0,0,0,0,0,0,0,0,0,1]`) must not appear in user-facing output.

### Worked mismatch example

```
let bad = 25USD + 5N
// ERROR: dimension mismatch: left `Scalar<USD>`, right `Scalar<kg·m·s^-2>` (Money vs Force)
```

The exact message format is owned by task 2380, but must satisfy: the left operand is
rendered via `DimensionVector::Display` (which already produces `"USD"` for `MONEY` and
`"kg·m·s^-2"` for `FORCE`) rather than raw slot arrays.

### Implementation pointer table

| Concept | Location |
|---------|----------|
| `format_dimension_mismatch_diagnostic` | `crates/reify-compiler/src/type_compat.rs` (in-flight, task 2380) |
| `DimensionVector::canonical_name()` | `crates/reify-types/src/dimension.rs` (in-flight, task 2380) |
| `DiagnosticCode::DimensionMismatch` | `crates/reify-types/src/diagnostics.rs` (in-flight, task 2380) |
| `DimensionVector::Display` (renders `"USD"` at slot 9) | `crates/reify-types/src/dimension.rs` lines 312–340 |

### Suppression conditions

No diagnostic is emitted if either operand type is `Type::Error` (anti-cascade contract,
task 1918). This mirrors the `FieldCodomainMismatch` suppression in `field-source-kinds.md`.

### Target test file

`crates/reify-compiler/tests/money_force_diag_tests.rs` (owned by task 2380)

---

## § Cost-aggregation stdlib idiom & canonical example — task 2381 (in-flight)

> **TBD — task 2381**

### Spec reference

`docs/reify-stdlib-reference.md` §9 (`std.io`):

```
trait Buy : Source {
    param supplier    : String
    param part_number : String
    param unit_cost   : Scalar<Money>
    param lead_time   : Time = undef
}
```

`Buy` is the canonical stdlib consumer of the Money dimension.

### Idiom

A `Buy` instance times a quantity yields `Scalar<Money>`; aggregation over a collection
of `Scalar<Money>` values yields total cost:

```
// Given: a list of Buy occurrences, each with unit_cost and quantity
let total_cost : Scalar<Money> = sum(buy.unit_cost * buy.quantity for buy in buys)
```

`unit_cost * quantity` has dimension `Money·Mass^-1 * Mass = Money`; `sum` over
`Scalar<Money>` stays `Scalar<Money>`.

### Target canonical-example file

`examples/cost-aggregation.ri` (path to be confirmed by task 2381)

### Implementation pointer table

| Concept | Location |
|---------|----------|
| `Buy` trait declaration | `docs/reify-stdlib-reference.md` §9, `crates/reify-compiler/stdlib/analysis.ri` (or equivalent) |
| `sum` aggregation builtin | `crates/reify-compiler/stdlib/analysis.ri` (in-flight) |
| Canonical example | `examples/cost-aggregation.ri` (in-flight, task 2381) |

---

## § Source-form unit display in errors & LSP hovers — task 2382 (in-flight)

> **TBD — task 2382**

### Rule

All rendered output — diagnostics, LSP hover tooltips, `Display`-formatted dimension
strings — must use the user-visible unit name (`"USD"`, `"USD·kg^-1"`) rather than the
raw slot-vector serialization (`[0,-1,0,0,0,0,0,0,0,1]`).

The `DimensionVector::Display` implementation at lines 312–340 of
`crates/reify-types/src/dimension.rs` already satisfies this rule for the base `MONEY`
dimension (produces `"USD"`) and for composed monetary dimensions (produces `"USD·kg^-1"`
for `CostPerMass`). The remaining work in task 2382 is to wire this `Display` through all
diagnostic and hover rendering paths that previously used debug/raw serialisation.

### Implementation pointer table

| Concept | Location |
|---------|----------|
| `DimensionVector::Display` (correctly maps slot 9 → `"USD"`) | `crates/reify-types/src/dimension.rs` lines 312–340 |
| LSP hover module | `crates/reify-lsp/src/hover.rs` |
| LSP diagnostic formatter | `crates/reify-lsp/src/diagnostics.rs` |
| Engine error formatter (`EngineError::DimensionMismatch`) | `crates/reify-eval/` (uses `Display` per commit `59a45c364`) |

### Regression pins (landed)

These tests in `crates/reify-types/src/dimension.rs` pin the display behaviour that
task 2382 must not regress:

- `to_display_units_recognises_money` — `to_display_units(25.0)` returns `(25.0, "USD")`
  for `MONEY`; asserts the `"SI"` fallback is NOT used.
- `money_display_outputs_usd` — `format!("{}", MONEY)` == `"USD"`.
- `money_per_mass_display_renders_compositely` — `format!("{}", MONEY.div(&MASS))` ==
  `"USD·kg^-1"` (Unicode middle dot U+00B7).
- `money_dimensionless_after_self_cancel_displays_dimensionless` — `MONEY / MONEY` →
  `"dimensionless"`.

---

## § Acceptance / regression sweep — task 2383 (in-flight)

> **TBD — task 2383**

### Acceptance criteria

The money-dimension feature is complete when all of the following hold:

**(a) All sections 2–7 are implemented and their named tests pass:**
- `crates/reify-compiler/tests/money_units_tests.rs` — §2 (task 2378)
- `crates/reify-eval/tests/money_arithmetic_tests.rs` — §3 (task 2379)
- `crates/reify-compiler/tests/money_force_diag_tests.rs` — §4 (task 2380)
- Example `examples/cost-aggregation.ri` compiles and evaluates — §5 (task 2381)
- LSP hover and diagnostics render user-visible names — §6 (task 2382)

**(b) Angle/Torque-vs-Energy regression remains green:**

The Angle dimension was added specifically to prevent `torque + energy` false-equality
(spec §3.2). Money must not disturb this invariant. Task 2383 must run the existing
Angle/Torque regression and confirm it still passes after the monetary feature set is
fully integrated.

**(c) Dimensional-purity guard passes:**

`money_does_not_leak_into_unrelated_arithmetic` (in `crates/reify-types/src/dimension.rs`,
line 700) must remain green. This test asserts that `LENGTH.mul(&MASS)` leaves slot 9 as
`ZERO`. Task 2383 must verify the analogous property at the eval level.

**(d) Canonical example evaluates cleanly:**

The file from §5 (`examples/cost-aggregation.ri`) must compile and evaluate without error
or warning under `cargo test` or an equivalent integration harness.

---

## Regression-pin index

| Section | Test / file | Status |
|---------|-------------|--------|
| § Overall design | `crates/reify-types/src/dimension.rs::money_constant_populates_slot_9` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_mul_with_mass_keeps_both_slots` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_div_by_mass_produces_cost_per_mass` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_div_by_money_is_dimensionless` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_pow_2_doubles_slot_9` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_root_2_halves_slot_9` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_content_hash_is_deterministic` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_content_hash_differs_from_other_base_dimensions` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::content_hash_buffer_covers_slot_9` | landed |
| § Overall design | `crates/reify-types/src/dimension.rs::money_does_not_leak_into_unrelated_arithmetic` | landed |
| § Stdlib unit declaration | `crates/reify-compiler/tests/money_units_tests.rs` | in-flight (task 2378) |
| § Quantity-literal arithmetic | `crates/reify-eval/tests/money_arithmetic_tests.rs` | in-flight (task 2379) |
| § Dimension-mismatch diagnostic | `crates/reify-compiler/tests/money_force_diag_tests.rs` | in-flight (task 2380) |
| § Cost-aggregation example | `examples/cost-aggregation.ri` | in-flight (task 2381) |
| § Source-form display | `crates/reify-types/src/dimension.rs::to_display_units_recognises_money` | landed |
| § Source-form display | `crates/reify-types/src/dimension.rs::money_display_outputs_usd` | landed |
| § Source-form display | `crates/reify-types/src/dimension.rs::money_per_mass_display_renders_compositely` | landed |
| § Source-form display | `crates/reify-types/src/dimension.rs::money_dimensionless_after_self_cancel_displays_dimensionless` | landed |
| § Acceptance sweep | Angle/Torque regression (existing test suite) | in-flight (task 2383) |

---

## Out of scope

The following are explicitly **not** covered by the v0.1 money-dimension feature
set; each layers atop or sits adjacent to this primitive and may be addressed in
later releases:

- **Time-varying / live exchange rates.** Conversion factors between currencies
  are constant for the lifetime of a project (see § Stdlib unit declaration
  notes). Live FX feeds are out of scope.
- **Currency-specific formatting / locale rules in the GUI.** Rendered output
  uses the literal's source unit name (e.g. `25USD` displays as `"USD"` per
  § Source-form unit display). Locale-aware grouping, currency symbol prefix
  ordering, and decimal-separator rules are deferred.
- **Tax / discount / margin idioms beyond raw aggregation.** The stdlib idiom
  in § Cost-aggregation covers `sum(unit_cost * quantity)` only; tax/discount
  layering is out of scope.
- **Multi-currency disambiguation policies.** v0.1 picks "use the literal's
  source unit" on display (e.g. mixed `USD`+`GBP` shows each in its declared
  unit). Heuristics like "prefer USD" are deferred.
- **Manufacturing-cost models.** Per-process or BOM-traversal cost models layer
  atop the `Buy` aggregation primitive and are not in this scope.

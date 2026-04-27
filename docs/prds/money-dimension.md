# PRD: Money Dimension End-to-End

## Goal

Light up the `Money` base dimension end-to-end for v0.1: enable user `unit USD : Money` declarations, enable `25USD/kg`-style quantity literals composing currency with physical dimensions, ship at least one built-in currency unit (`USD`), and provide a stdlib cost-aggregation idiom that sums `unit_cost * quantity` across `Buy` occurrences.

## Background

- Spec §3.2 (lines 224-240): the dimension exponent vector has 9 base dimensions with `Money` at position 9. Money is already in the vector but unused for user-facing currency declarations. The spec calls out: "Monetary units (`USD`, `GBP`, `EUR`, etc.) are declared with the `unit` keyword. All monetary values within a project use constant conversion factors. Time-varying exchange rates are out of scope. Enables expressions like `25USD/kg` for cost estimation."
- Spec §3.4 (line 393 area) and §11 stdlib: `Buy`-style occurrences carry a `unit_cost : Scalar<Money>` parameter; cost-aggregation across an assembly is summing these times the produced quantities.
- Spec §4.6 (lines 797-805): unit declarations syntax includes `unit USD : Money` (no conversion factor needed for a pure base unit).
- Existing infra: #208 (unit registry compile-pass) and #209 (user-defined `unit` declarations) are done. This PRD bolts Money onto that machinery and ships the stdlib bits.
- Architectural decision: Money composes with physical dimensions via multiplication/division like any other dimension. Multiple currencies in one project use compile-time-constant conversion factors (`unit GBP : Money = 1.25USD`). Time-varying exchange rates remain out-of-scope.

## Scope

- **Dimension vector slot 9 wired through compiler.** Verify the `Money` slot in dimension exponent vectors round-trips through:
  - Type construction (`Scalar<Money>`, `Money * Length^-1`, etc.).
  - Type-equality / type-display.
  - Dimensional analysis on arithmetic (`USD/kg * kg = USD`).
  - Unit-registry lookup (`USD` resolves to dimension vector with Money=1).
- **Built-in currency unit `USD`.** Declared in stdlib (`std.units.money` or similar). Acts as the "anchor" currency. Other currencies declared by users (or in stdlib follow-ups) use conversion factors against USD: `unit GBP : Money = 1.25USD`.
- **Quantity-literal syntax for currency.** `25USD`, `25USD/kg`, `2.5USD` work via the existing quantity-literal lexer (#208 plumbing). No new lexer rule expected — the existing `<number><unit>` rule generalizes once the unit is registered.
- **Cost-aggregation stdlib idiom.** Provide an idiomatic pattern (likely a helper trait or `let total_cost = sum_costs(self)` in a base trait) that iterates over the `Buy` occurrences in scope and sums `unit_cost * quantity_produced`. Concrete shape:
  - Trait `BuyOccurrence` exposing `unit_cost : Scalar<Money>` and the produced quantity (units depend on what's bought).
  - Helper that walks an assembly and sums per-occurrence costs into a single `Scalar<Money>`.
  - Smoke-test example demonstrating `25USD/kg * 2kg = 50USD` and assembly-wide aggregation.
- **Type-check coverage.** `25USD/kg + 5N` is a dimension error. `25USD + 30USD = 55USD` is fine. `25USD + 30GBP` typechecks (same `Money` dimension) and the unit-conversion machinery applies.

## Out of scope

- Time-varying / live exchange rates.
- Currency-specific formatting / locale rules in the GUI (display picks the literal's source unit).
- Tax / discount / margin idioms beyond raw aggregation.
- Multi-currency disambiguation policies (e.g., prefer USD on display) — pick simple "use literal's source unit" for v0.1.
- Manufacturing-cost models (those layer atop this primitive).

## Acceptance criteria

1. `unit USD : Money` (declared in stdlib) registers a Money-dimension unit; `25USD` parses as a quantity literal of type `Scalar<Money>`.
2. `unit GBP : Money = 1.25USD` (declared in user module) registers; `30GBP` typechecks as `Scalar<Money>` with conversion factor 1.25 against USD.
3. Dimensional arithmetic: `25USD / 2kg` typechecks as `Scalar<Money * Mass^-1>` with the right exponent vector (Money=1, Mass=-1).
4. `25USD/kg * 2kg = 50USD` reduces to a Money scalar; integration test asserts numeric value.
5. `25USD + 5N` → dimension error with clear diagnostic citing Money vs Force.
6. A stdlib `Buy`-occurrence trait or example exposes `unit_cost : Scalar<Money>` and supports aggregation across an assembly via a documented idiom (`forall b in self.buys: ...` or a stdlib helper).
7. Cost-aggregation example file (`examples/m6_purpose_cost.ri` or similar) compiles and produces an expected total cost.
8. Unit-display in error messages and LSP hovers shows currency units in their source form (e.g., `USD/kg`, not `Money * Mass^-1`).
9. No regression to non-Money dimensions: existing 8-dim arithmetic, unit conversions, Angle/Torque tests still pass.

## Task breakdown

1. Audit `Dimension` representation in compiler / runtime and verify Money slot is populated, displayed, hashed, and serialized correctly. Add unit tests if any path was Money-blind.
2. Add `unit USD : Money` to stdlib (`std/units/money.ri` or appropriate path). Verify registry pickup.
3. End-to-end quantity-literal test: `25USD`, `25USD/kg`, currency-mass arithmetic.
4. Type-check / diagnostic test: dimension-mismatch errors mention Money distinctly.
5. Stdlib cost-aggregation idiom: design a helper / trait pattern; ship one canonical example (`examples/cost_aggregation.ri`) demonstrating per-buy aggregation.
6. LSP / display path: confirm currency units render in source form, not as `Money` exponent vectors.
7. Tests for (1)-(8) above, including a regression test asserting Angle/Torque distinction is unaffected.

## Notes

- Re-uses #208 (unit registry) and #209 (user-defined units) plumbing — no new pre-pass infrastructure expected.
- Currency conversion factors are compile-time constants. A user supplying `unit GBP : Money = exchange_rate()` is a static error (factor must be a constant expression). Document this in the unit-decl error path if not already.

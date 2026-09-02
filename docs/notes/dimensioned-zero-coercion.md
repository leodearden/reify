# Dimensioned-zero coercion — the `DIMENSIONED-ZERO CONVENTION` referenced from stdlib

**Task #6038 | 2026-09-02** — supersedes the stale esc-3115-112 rationale (`task #3115`).

Operational digest for the §7.2 syntactic-zero operand coercion landed by **task-4485/β**.
Implementation authority is `coerce_zero_operand`'s rustdoc in
`crates/reify-compiler/src/expr.rs` (with `type_compat::is_syntactic_zero_literal`); design
authority is `docs/prds/v0_6/type-hygiene.md` §35. This note exists so the stdlib `.ri` comments
that lean on the mechanism can point at a **stable path** rather than at one structure's
constraint comment — the previous canonical statement lived inside `StepForce.magnitude > 0N` in
`modal_analysis.ri`, and renaming or deleting `StepForce` would have silently broken six pointers.

---

## The rule

In **comparison / additive operand position** a syntactic zero is rewritten to `Scalar<D>(0.0)` at
compile time, taking `D` from the *sibling* operand, before the dimension guard and type inference
run. Concretely, `coerce_zero_operand` fires when one operand is a syntactic zero (or
`const_folds_to_zero`) whose compiled type is dimensionless, AND the sibling's compiled type is
`Type::Scalar{D}` with non-dimensionless `D`.

It covers:

| Axis | Coverage | Why |
|---|---|---|
| Literal form | `0`, `0.0`, and negated `-0` / `-0.0`; also constant-folded zeros such as `1 - 1` | `is_syntactic_zero_literal` recurses through the `UnOp{"-"}` wrapper; `const_numeric_value` catches the folded case |
| Dimension family | every family — base, compound-product, compound-quotient | the gate is dimension-agnostic; it copies `D` off the sibling |
| Operator | `Lt Le Gt Ge Eq Ne Add Sub` | the `matches!` gate in `compile_binop`; the call runs BEFORE `infer_binop_type` |
| Operand order | both | two symmetric arms in `coerce_zero_operand` |
| Sibling shape | any expression whose COMPILED TYPE is a non-dimensionless `Scalar` — a member access such as `material.density` included | the rewrite keys on that type, not on the sibling's expression shape |

**Consequence.** A dimensioned RHS literal is never *required* in this position. `magnitude > 0N`
and `magnitude > 0` compile to the same thing, and the compiled RHS is dimensioned by the time
`eval_cmp` checks dim-equality at runtime either way. Where the stdlib keeps the dimensioned form
(`0N`, `0Hz`, `0kg`, `0kg/m^3`, `0.0V/m`, `0 * 1N * 1s`) it is a **readability convention**, not a
requirement.

## Where the rule does NOT reach

- **Non-zero literals are never coerced.** `resistivity < 0.0001` really is a compile error
  (`Scalar[m^3·kg·s^-3·A^-2]` vs `Real`), so `materials_electrical.ri`'s `trait Conductive` bound
  genuinely needs its `ohm*m`. This is the load-bearing distinction the sweep preserves.
- **Param defaults.** `coerce_zero_operand`'s sole call site is inside `compile_binop`, so the
  rewrite never reaches a param default. The literal guard in `check_param_default_type` merely
  early-`return`s to SUPPRESS `ParamDefaultTypeMismatch`; it performs no rewrite. So
  `param phase : Angle = 0` stores a DIMENSIONLESS default while `= 0deg` stores `Scalar[rad]` —
  see the `HarmonicForce.phase : Angle = 0deg` note in `modal_analysis.ri`.
- **Index access.** `structural_physical.ri` deliberately does not lean on the coercion for the
  `moi_principal[0]` `IndexAccess` shape; that is a recorded choice, not a gap in the rule.

## Why a clean compile is a real signal (and when it is not)

A "no error diagnostics" probe is only evidence that the zero WAS coerced if the same shape with a
*mismatched* operand would have errored. Two guards make that true:

- Scalar-vs-Scalar with differing dimensions → `DiagnosticCode::DimensionMismatch`.
- Dimensioned `Scalar` vs non-dimensionless `Int` → an "incompatible types in comparison" error
  (note: **no `code`** is attached on this arm, so assert on the message, not on a
  `DiagnosticCode`).

Both live in `emit_comparison_operand_diagnostics`. So a regressed coercion is **loud**, not a
silent degradation to `Satisfaction::Indeterminate`.

**The one vacuity condition — measured, task #6038 amendment pass.** A trait body compiled with
**NO conformer** is not dimension-checked at all: `trait T { param material : Material; constraint
material.density > 1m }` yields *zero* diagnostics. Add a conformer and the same body emits
`DimensionMismatch`. So the vacuity condition is precisely *trait body with no conformer* — a
trait-body probe written **with** a conformer is fully checkable at compile level, and is pinned
that way.

## Test pins

| Level | File | What it pins |
|---|---|---|
| Compile | `crates/reify-compiler/tests/polymorphic_zero_tests.rs` | operand/operator/dimension/shape breadth, including trait-body-with-conformer, plus the non-vacuity guards |
| Compile (negatives) | `crates/reify-compiler/tests/comparison_operand_guard_tests.rs` | the guards that make a clean compile meaningful |
| Eval | `crates/reify-eval/tests/polymorphic_zero_eval.rs` | `Satisfaction::Satisfied` at runtime, incl. compound dimensions |
| Eval (trait bodies) | `crates/reify-eval/tests/harness_engine/polymorphic_zero_trait_eval.rs` | the runtime satisfaction signal for the two trait-body stdlib sites — `Satisfied` vs `Violated`, which no compile pin can express |

## Stdlib sites pointing here

`modal_analysis.ri` (`StepForce.magnitude`, `ImpulseForce.impulse`, `HarmonicForce.amplitude` /
`.frequency`), `dynamics.ri` (`MassProperties.mass >= 0kg`), `trajectory.ri`
(`JointLimit.max_force`, `ZVShaper.target_frequency`), `structural_physical.ri` (`trait Physical`,
`StiffnessRequirement`), `materials_electrical.ri` (`trait Insulating`).

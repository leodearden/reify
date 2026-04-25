# Purpose Reflective Aggregation — Runtime Expansion Blockers

**Status:** Open — runtime expansion deferred to task-2199 follow-up
**Date:** 2026-04-25
**Source:** esc-2199-15 S2; analysis relocated from inline FIXME in
`crates/reify-compiler/src/expr.rs` (task-2213)

---

## Context

`subject.params`, `subject.geometric_params`, and `subject.material_params` in purpose
bodies compile today to an **empty `ListLiteral`** with element type `Type::Real`.  This
means a constraint like:

```
forall p in subject.params: determined(p)
```

evaluates **vacuously true** at runtime (the list is always empty, so the quantifier
body is never entered).  The placeholder is deliberate and anti-cascade-consistent, but
it is a trap for callers who expect the constraint to actually fire.

Placeholder code location:
`crates/reify-compiler/src/expr.rs` — the `return CompiledExpr::list_literal(…)`
inside the `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS.contains(…)` branch of
`compile_expr_guarded`.

The vacuous-true trap is pinned by the §8 characterization test
`manufacturing_ready_silently_passes_for_undetermined_params_trap` in
`crates/reify-eval/tests/purpose_activation.rs`.  **That test's assertion MUST be
flipped from Satisfied to Violated when expansion lands.**

---

## Blocker 1 — List population at `activate_purpose`

`activate_purpose` must replace this empty list with
`ListLiteral([ValueRef(entity_ref, member), ...])` for each param of the bound entity.
The cell IDs are already available in `CompiledPurpose.resolved_queries`
(see `compile_purpose` in `traits.rs`); the wiring to the activation path is missing.

---

## Blocker 2 — Quantifier variable identity carry-through

`forall p in [Bracket.x]: determined(p)` compiles `determined(p)` with the
Quantifier's synthetic `variable_id` as the predicate cell.  At runtime the loop binds
`variable_id` to the *value* of `Bracket.x`, but the determinacy snapshot has no entry
for the synthetic ID — `eval_expr` debug-asserts a "wiring bug" panic (see
`DeterminacyPredicate` handling in `eval_expr`).  The quantifier must carry the actual
`ValueCellId` of each iterated element into the predicate, not the synthetic loop var.

Note: a new `Type::ParamRef` variant is **not** required; what is missing is identity
carry-through so the predicate resolves to the bound cell, not the loop var.

---

## Blocker 3 — Element type lockstep (task-1904)

`Type::Real` is a placeholder element type.  Any populator MUST ensure the element type
matches each param's declared type, or `forall` typechecks `p` against the wrong
type — silently, because the list is empty today.

Cross-reference: task 1904 (`integration_full_v01.rs:660-662`) tracks the same
runtime-expansion concern from a different angle.

---

## When this can change

All three blockers must land **together** in a single coordinated change.  Partial
progress that populates the list without fixing quantifier identity carry-through
(Blocker 2) will trigger the `eval_expr` wiring-bug panic at runtime.

When expansion lands, flip the §8 test's assertion from Satisfied to Violated (see
Context section above) to confirm the constraint now fires correctly.

# Purpose Reflective Aggregation — Runtime Expansion Blockers

**Status:** Resolved (task-2289) for `subject.params`;
`geometric_params` / `material_params` filter-kind resolution
remains deferred to task-1904 follow-up.
**Date:** 2026-04-25 (initial), 2026-04-26 (resolution)
**Source:** esc-2199-15 S2; analysis relocated from inline FIXME in
`crates/reify-compiler/src/expr.rs` (task-2213); resolved by task-2289.

---

## Context

`subject.params`, `subject.geometric_params`, and `subject.material_params` in purpose
bodies historically compiled to an **empty `ListLiteral`** with element type `Type::Real`.
That meant a constraint like:

```
forall p in subject.params: determined(p)
```

evaluated **vacuously true** at runtime (the list was always empty, so the quantifier
body was never entered).  The placeholder was deliberate and anti-cascade-consistent,
but it was a trap for callers who expected the constraint to actually fire.

The vacuous-true trap was pinned by a §8 characterization test
(`manufacturing_ready_silently_passes_for_undetermined_params_trap`) in
`crates/reify-eval/tests/purpose_activation.rs`, which task-2289 has flipped into
the acceptance test `manufacturing_ready_violates_for_undetermined_params`.

Compiler emit point (now a marker variant rather than an empty list):
`crates/reify-compiler/src/expr.rs` — the `return CompiledExpr::purpose_reflective_aggregation(…)`
inside the `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS.contains(…)` branch of
`compile_expr_guarded`.

---

## Blocker 1 — List population at `activate_purpose` — RESOLVED (task-2289)

**Resolved by:** A new `CompiledExprKind::PurposeReflectiveAggregation { param_name,
query_kind }` variant (TAG byte 25) is emitted by `compile_expr_guarded` in place of
the empty `ListLiteral`. `activate_purpose` in
`crates/reify-eval/src/engine_purposes.rs` walks each constraint expression (and the
objective, if any) immediately after the existing `remap_entity` rewrite and rewrites
every placeholder via the private free function
`expand_purpose_reflective_placeholders`.

For the `params` query, resolution prefers the compile-time
`CompiledPurpose.resolved_queries` entry (populated by `compile_purpose` in
`reify-compiler/src/traits.rs` for concrete-typed purpose params); a wildcard-subject
fallback scans `state.snapshot.graph.value_cells` for cells whose `entity` matches
`entity_ref` and whose `kind` is `Param` or `Auto`, sorted for determinism.

For `geometric_params` / `material_params`, no compile-time `ResolvedSchemaQuery`
entry exists today and there is no activation-time fallback heuristic, so the
expansion replaces the placeholder with an empty `ListLiteral` — preserving today's
vacuous-true behavior for those filter kinds. Defining the filter semantics
(LENGTH-dimensioned? `StructureRef`-typed?) and populating the corresponding
`resolved_queries` entries is task-1904 territory.

---

## Blocker 2 — Quantifier variable identity carry-through — RESOLVED (task-2289)

**Resolved by:** A new cell-iteration branch at the top of the `Quantifier` arm in
`crates/reify-expr/src/lib.rs::eval_expr`. When the quantifier's collection is a
`ListLiteral` whose elements are all `ValueRef(_)`, the evaluator iterates over the
*cell IDs* rather than values: per iteration it clones the predicate, calls
`predicate_clone.remap_cell(variable_id, cell_id)` (a new helper on `CompiledExpr`
mirroring `remap_entity` and rewriting `ValueRef`, `Quantifier.variable_id`,
`DeterminacyPredicate.cell`, and `Lambda.captures` / `param_ids`), and also inserts
the cell's value into the per-iteration scope so non-`DeterminacyPredicate` uses of
the bound variable (e.g. `forall p in subject.params: p > 0`) keep working. The
existing Kleene short-circuit (forall: false → return; undef tracked; exists: true
→ return; undef tracked) is preserved. When the collection isn't a `ListLiteral` of
pure `ValueRef`s, the existing value-iteration code is the fallback so previously
green tests like `forall x in [1, 2, 3]: x > 0` keep passing.

This makes `determined(p)` inside `forall p in subject.params: determined(p)`
resolve to the iterated entity's actual cell ID (e.g. `Bracket.x`), not the synthetic
loop var — closing the wiring-bug `debug_assert!` path noted in the original
`DeterminacyPredicate` handling.

---

## Blocker 3 — Element type lockstep (task-1904) — RESOLVED for `params`

**Resolved by:** `expand_purpose_reflective_placeholders` sources each populated
element's `result_type` from the looked-up `ValueCellNode.cell_type` (via
`state.snapshot.graph.value_cells`); the outer `ListLiteral.result_type` adopts
`Type::List(Box::new(first_element_type))` when populated, falling back to
`Type::List(Box::new(Type::Real))` for the empty-list case (anti-cascade-safe).
The compile-time placeholder element type stays `Type::Real` to avoid cascading
typecheck changes; activation-time expansion sets correct per-element types.

The homogeneous-list assumption (outer list type taken from the first element)
matches today's typecheck. A heterogeneous-param structure is a real-but-rare case
left for follow-up if it ever appears.

Cross-reference: task 1904 (`integration_full_v01.rs:660-662`) tracks the same
runtime-expansion concern from a different angle for `geometric_params` /
`material_params`.

---

## Remaining gap — `geometric_params` / `material_params` (task-1904)

`compile_purpose` in `crates/reify-compiler/src/traits.rs:442-460` only populates
`CompiledPurpose.resolved_queries` entries for `query_kind="params"` (selecting on
`vc.kind == Param | Auto`). Filter-kind resolution for `geometric_params` and
`material_params` is task-1904 territory: the work needed is to define what each
filter means at the schema level (LENGTH-dimensioned vs. `StructureRef`-typed vs.
something else) and extend the `resolved_queries` loop to populate the matching
entries.

The activation-side machinery is already in place: `expand_purpose_reflective_placeholders`
will pick up any new `ResolvedSchemaQuery` entries automatically and produce
populated `ListLiteral`s. No additional engine work is needed once the compile-time
filter logic lands. The `activate_expands_geometric_params_placeholder_to_empty_list`
test in `crates/reify-eval/tests/purpose_activation.rs` pins the current
empty-list-on-no-resolved-query behaviour so a future task-1904 change can flip it
the same way task-2289 flipped the §8 trap.

---

## Resolution commits (task-2289)

The full task-2289 stack lands as a sequence of TDD commits — see the task branch
`task/2289` for the per-step history. Headline commits:

  - `feat(types): add PurposeReflectiveAggregation variant + tag` (step-4)
  - `feat(compiler): emit PurposeReflectiveAggregation placeholder` (step-7)
  - `feat(expr): add cell-iteration mode to Quantifier eval` (step-9)
  - `feat(eval): expand reflective-aggregation placeholders at activation` (step-11)
  - `test(eval): flip §8 trap to acceptance — purpose violates on undetermined params` (step-13)

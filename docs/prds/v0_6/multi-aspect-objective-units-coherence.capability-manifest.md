# Capability manifest — multi-aspect-objective-units-coherence

Mechanizes G3+G6 per leaf. Evidence gathered 2026-07-05 against `cf50240115` (debug binary + `tree-sitter parse` + grep). Fixtures under `/tmp/prd-gate-fixtures/mauc-*.ri` (session-scoped; the committed CI fixtures land in δ/β).

## α — units-coherence guard

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `E_OBJECTIVE_MIXED_DIMENSION` rejection fires on a 2-term mixed-dim objective | rejection-mechanism (anti-silent-accept) | **Currently ABSENT** (the RED): `mauc-5-silent.ri` (`minimize scale*1USD/1mm` + `minimize scale*1kg/1mm`) `reify check` exits 0, no diagnostic; `reify eval` silently resolves `scale = 0.01 m`. α *builds* the rejection; the signal observes it fire post-α. Mechanism modelled on the existing `check_objective_conflict` (`entity.rs:650`, emits `E_OBJECTIVE_CONFLICT`). | PASS (RED confirmed; α is the producer, upstream of its own leaf signal) |
| `dimension_of(term.expr.result_type)` available without eval | capability→producer (wired) | `grep reify-constraints/src/solver.rs:99` `dimension_of` exists; used on `expr.result_type` at `solver.rs:1380`,`1406`. `CompiledExpr.result_type: Type` at `reify-ir/src/expr.rs:11`. | PASS (wired on main) |
| the three fold sites exist to assert against | capability→producer (wired) | `solver.rs:835`, `registry.rs:495`, `engine_eval.rs:2728` — all fold `term.weight * as_f64(v)` (grep-confirmed); triplication comment `engine_eval.rs:2704`. | PASS |
| new `DiagnosticCode` mintable | capability→producer (wired) | `reify-core/src/diagnostics.rs` mints distinct codes idiomatically (`NonIntegerExponentOnDimensioned` `:1866`; `DimensionMismatch` `:497`). | PASS |
| numeric floor | anti-floor | N/A — no numeric bound asserted (a coherence check, not an accuracy claim). | N/A |

## β — aspect-trait vocabulary (`Massive`)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `trait Massive { param mass : Mass }` parses | grammar-fixture (anti-mismatch) | `mauc-1-trait.ri` → `tree-sitter parse --quiet` exit 0 (0 ERROR nodes). Standalone traits legal (`trait Joint { }` `kinematic.ri:71`, `trait Source {}` `io.ri:57`). | PASS |
| `Mass` resolves as a dimensioned type; `0.4kg` literal | capability→producer (wired) | `pub unit kg : Mass` `units.ri:28`; `DimensionVector::MASS` canonical name "Mass" `dimension.rs:481,1016`. `param density : Density = 7850kg/m^3` in stdlib `materials_fea.ri:160`. | PASS |
| `structure def X : Costed + Massive` conforms (multi-trait) | capability→producer (wired) + grammar | `mauc-2-multiaspect.ri` parses (exit 0) and `reify check` clean (only `W_MODULE_DECL_MISSING` + indeterminate-auto). `Costed` at `io.ri:111`; nominal conformance `structural_query.rs:592` `satisfies_trait_bound`. | PASS |
| `filter(self.descendants, Massive)` selects massive children | capability→producer (wired) | `apply_trait_filters` `structural_query.rs:541`; `filter(self.descendants, Bolt)` idiom `examples/structural_query_filter.ri:47` (δ `#3991`, landed). `.sum` roll-up `reify-expr/src/lib.rs:3523` (`#4292`, landed). | PASS |

## δ — coherent multi-aspect CI example (integration gate)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `minimize total_cost/1USD + w*total_mass/1kg` parses + normalises to dimensionless | grammar-fixture + field-population | `mauc-2-multiaspect.ri` parses (exit 0) and `reify check` clean — `/1USD`, `/1kg` de-dimensionalise (precedent `cost_robustness_tradeoff(… thickness/1m …)`). | PASS |
| coherent solve resolves the auto param | end-to-end (branch 3) | Requires `minimize`+`DimensionalSolver` (`#4795`, landed), `.sum` (`#4292`), `Costed` (landed), **`Massive` (β, upstream)**, guard (**α, upstream**, for the negative fixture). Every capability in the transitive dep set. | PASS (DAG-direction: α,β upstream of δ) |
| negative fixture emits `E_OBJECTIVE_MIXED_DIMENSION` | rejection-mechanism | Produced by **α** (upstream). δ's negative fixture is the observation site. | PASS (producer α upstream) |

## ε — companion doc-correction

Prose-only; no code capability. Signal: parent `continuous-cost-minimisation.md` §10 row-4 points at the realized mechanism. No manifest binding required (docs).

## Summary

No `FAIL` / `UNPROVABLE` bindings. The one RED-by-design is α's rejection mechanism (`E_OBJECTIVE_MIXED_DIMENSION`), whose current ABSENCE is confirmed (`mauc-5` silent-accept) and whose producer is α itself — the correct DAG direction. δ's premises are all delivered by α/β (upstream) or landed substrate. Grammar reality for every novel fragment is fixture-proven (exit 0).

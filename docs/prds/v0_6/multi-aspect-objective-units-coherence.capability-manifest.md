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
| aspect fields aggregate via explicit named-sub member access (`[a.mass,b.mass].sum`, `[a.line_cost,b.line_cost].sum`) | capability→producer (wired) | Verified `reify eval` on a two-`Bracket : Costed+Massive` frame: `total_mass = 0.8 kg`, `total_cost = 6 USD`. `.sum` roll-up `reify-expr/src/lib.rs:3523` (`#4292`, landed); member access on named subs works. | PASS |
| `filter(self.descendants, Massive).count` participates in structural query | capability→producer (wired) | `apply_trait_filters` `structural_query.rs:541` (δ `#3991`). **`.count` only** — member projection (`.mass`) is NOT wired (see below), so β does not depend on it. | PASS (count); member-projection **excluded** |
| ~~`filter(self.descendants, Massive).mass` projects the aspect field~~ | member-projection | **REMOVED (D3-verification 2026-07-05):** `reify check` on `filter(self.descendants, Massive).mass` → `error: member access not yet supported: .mass`. β rewritten to use explicit named-sub aggregation; filter-project-member is unshipped and out of scope. | resolved by removal |

## δ — coherent multi-aspect CI example (integration gate)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `minimize cost(thickness)/1USD + w*mass(thickness)/1kg` parses + normalises to dimensionless | grammar-fixture + field-population | `delta-pos.ri` (cost & mass closed-form in own `auto(free)` `thickness`) `reify check` clean; `/1USD`,`/1kg` de-dimensionalise (precedent `cost_robustness_tradeoff(… thickness/1m …)`). | PASS |
| coherent solve **resolves the own auto param** | end-to-end (branch 3) | **Verified** `reify eval delta-pos.ri` → `MultiAspectPlate.thickness = 0.01 m` (feasible, no dimension error). Two-decl same-dimensionless variant also resolves (`Plate2.thickness = 0.01 m`). Requires `minimize`+`DimensionalSolver` (`#4795`, landed), **`Massive` (β, upstream)** for the aspect vocabulary. Objective over the scope's OWN auto param (child-aggregation objective is cross-scope `#4785`, excluded). | PASS (DAG-direction: β upstream of δ) |
| negative fixture emits `E_OBJECTIVE_MIXED_DIMENSION` | rejection-mechanism | Produced by **α** (upstream). δ's negative fixture is the observation site. Current ABSENCE confirmed (`mauc-5` silent-accept). | PASS (producer α upstream) |

## ε — companion doc-correction

Prose-only; no code capability. Signal: parent `continuous-cost-minimisation.md` §10 row-4 points at the realized mechanism. No manifest binding required (docs).

## D3-verification outcome (2026-07-05)

The mechanized decompose-verify workflow (`scripts/prd-decompose-verify.mjs`, run `wf_93ce116e-036`) **blocked** the first draft and surfaced two genuine false premises, now **fixed**:
1. **filter-project-member** — `filter(self.descendants, Massive).mass` is unshipped (`member access not yet supported`). β/δ rewritten to explicit named-sub aggregation; filter-project excluded from scope.
2. **degenerate objective** — the original δ fixture aggregated *child* aspects (constants) so the solve was `not uniquely determined / infeasible`. δ rewritten to minimise over the scope's **own** auto param (cost & mass closed-form in it); verified `thickness = 0.01 m`.

**Residual (accepted, not a false premise):** the workflow also flags α's and δ-negative's `E_OBJECTIVE_MIXED_DIMENSION` as "rejection absent." That is correct — the mechanism is α's **deliverable** (its RED test); the workflow verifies against current `main` and cannot model "this leaf builds the asserted mechanism." Bound as `producer: α, upstream` (the standard RED-first pattern, mirroring how `check_objective_conflict` was built). Not re-run after the fix (the residual α-RED is un-modelable by the harness); the corrected β/δ premises were re-verified manually against the debug binary (evidence above).

## Summary

No unresolved `FAIL`/`UNPROVABLE` bindings after the D3-driven corrections. Every code premise is either wired-on-main (grep/eval evidence), grammar-fixture-proven (exit 0), or delivered by an upstream leaf (α/β). The only RED-by-design is α's rejection mechanism, whose producer is α itself.

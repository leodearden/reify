# PRD: Multi-aspect objective units-coherence

**Milestone:** v0_6 · **Status:** ACTIVE — expanded from forward-stub 2026-07-05 (design decisions resolved with Leo) · **Date:** 2026-07-05
**Parent:** `continuous-cost-minimisation.md` §10 (out-of-scope row 4). **Cluster:** `cost-optimisation`. **Tracking task:** #4786.

## Goal (user-observable)

Make combining **multiple aspects** in one objective — cost **and** mass **and** part-count **and** waste — *dimensionally honest*, and ship a coherent multi-aspect `minimize` a user can actually write. Two observable outcomes:

1. **The silent hazard becomes a loud diagnostic.** An author who writes two same-sense objective terms in different dimensions — `minimize cost` + `minimize mass` — today gets a **silently-wrong** solve (the fold sums `USD + kg` as raw f64). After this PRD, `reify check` emits `error: E_OBJECTIVE_MIXED_DIMENSION` naming the two incoherent dimensions.
2. **A coherent multi-aspect objective solves.** A CI `.ri` minimises `cost(thickness)/1USD + w * mass(thickness)/1kg` — cost and mass both closed-form in the scope's own `auto(free)` param — and resolves the param, the normalised dimensionless tradeoff idiom working end-to-end. (`Costed`/`Massive` supply the *aspect vocabulary*; the objective is over the scope's own auto params — aggregating an aspect over children into an objective is cross-scope, `#4785`.)

## Background — the hazard, ground-truthed (2026-07-05)

The stub framed the hazard as `minimize cost + mass` silently computing `36.88 (USD) + 2.4 (kg) = 39.28`. **Empirical correction (verified against the debug binary at `cf50240115`):** that exact *single-expression* form is **already caught** — `c + m` lowers to `BinOp::Add`, `eval_add` returns `Undef` on a dimension mismatch (`reify-expr/src/lib.rs:4084`), and the compiler surfaces `error: dimension mismatch in addition: Scalar[USD] vs Scalar[kg]` at `reify check`. The single-expression path is dimensionally honest.

The **real** silent hazard is the **multi-term `WeightedSum` fold**:

- `check_objective_conflict` (`reify-compiler/src/entity.rs:650`) rejects only an **opposite-sense** (`minimize`/`maximize`) pair over distinct expressions. **Two same-sense `minimize` decls do not conflict** — they lower to a **2-term** `ObjectiveSet{WeightedSum}` (one `ObjectiveTerm` per decl, coefficients living inside each `expr`).
- `eval_objective_set` (`reify-constraints/src/solver.rs:815-840`) then folds `acc += term.weight * eval_expr(term.expr).as_f64()` **across terms** — `as_f64()` (`reify-ir/src/value.rs:1633`) discards the dimension, so the sum of a `Money` term and a `Mass` term is a bare-f64 nonsense number with **no diagnostic**.
- Verified reachable today: `minimize scale*1USD/1mm` + `minimize scale*1kg/1mm` checks clean and `reify eval` silently resolves `scale = 0.01 m`. No multi-aspect authoring surface is needed to trigger it — two `minimize` statements suffice.

This is the project's `feedback_silent_defaults_pattern` antipattern at the objective-combination seam: the honest per-term `eval_add`/`.sum` machinery is bypassed the instant the fold calls `as_f64()`.

**The fold is triplicated (I3).** The same dimension-stripping fold appears at three sites, flagged by the comment at `reify-eval/src/engine_eval.rs:2704` ("If PRD §6.2 invariant I3 changes, update all three call sites"):
1. `reify-constraints/src/solver.rs:829-838` — `eval_objective_set` (canonical argmin cost).
2. `reify-constraints/src/registry.rs:489-499` — `eval_rank_cost` (ε-band Lexicographic per-rank).
3. `reify-eval/src/engine_eval.rs:2716-2736` — `objective_term_contributions` (post-solve provenance).

A contrast case worth noting: the ε-band Lexicographic path *also* builds a combined cost **symbolically** via `BinOp::Add → eval_add` (`registry.rs:513` `signed_term_expr`) and is therefore already dimension-honest — evidence that folding through the honest primitive is viable.

## Activation status

All FLIP-CONDITION deps landed: `#3991` (structural-query δ — `filter(self.descendants, Trait)`), `#4292` (BOM `.sum` roll-up), `#4795` (`continuous-cost-minimisation.md` terminal). The tracking milestone `#4786` dispatched and escalated to L2 (`esc-4786-7`) for this expansion. No further activation gate.

## Resolved design decisions

### D1 — Option 1 (guard + expression-level normalisation). `ObjectiveTerm.weight` stays `f64`. *[irreversible call — resolved]*

The stub's fork was: (1) dimensionless-only terms + a guard; (2) dimensioned "shadow-price" weights (`weight: f64 → Value`, breaking IR change); (3) project-everything-to-Money.

**Chosen: Option 1.** The load-bearing observation: **options 1 and 2 produce the *same* dimensionless objective** — they differ only in *where the normaliser lives*. Option 1 puts it in the expression (`minimize cost/1USD + w*mass/1kg`, `weight` stays `1.0`); option 2 puts it in a dimensioned `weight` field. Since the end-state is equivalent, option 1 is strictly cheaper and reversible:

- **No IR change.** `ObjectiveTerm.weight` (`reify-ir/src/constraint.rs:83`) stays `f64`. `weight` is **never DSL-parsed** today (production is always `1.0`; author coefficients live inside `expr` as `BinOp::Mul`), and the objective IR has **no serde / wasm / snapshot** persistence — so dimensioning `weight` later is no harder than now. Deferral costs nothing.
- The normalisation idiom is **already established**: `cost_robustness_tradeoff(unit_cost * (thickness / 1m), …)` divides by a reference quantity to de-dimensionalise. `minimize total_cost/1USD + w*total_mass/1kg` is the same trick; the dimensionless coefficient `w` is the author-controlled tradeoff (e.g. `w = 0.5` per kg-normalised means "1 USD trades against 2 kg").

**Option 3 rejected as the primary model:** monetising every aspect via stdlib price models privileges `Money` — the "cost is special" trap the stub's own precondition warns against — and some aspects resist monetisation (part-count for reliability, waste for compliance). Price-model *helpers* may still live in stdlib as optional convenience for genuinely-monetisable aspects; that is not this PRD's mechanism.

**Option 2b deferred, not rejected.** The genuinely-more-powerful variant — `weight = Money/aspect_dim` (a real shadow price → a **Money-valued** objective that is interpretable as "total equivalent cost = \$X" and yields shadow-price sensitivity, "the shadow price of mass at the optimum is \$4.20/kg") — is a distinct, larger capability. It is **documented here and deferred to a future PRD** (proposed slug `shadow-price-objectives.md`, "M-SHADOW"), with an impl-site breadcrumb at the guard (per `feedback_breadcrumb_design_alternatives_at_impl_site`). `Money/Mass` is already a first-class derived dimension (`reify-core/src/dimension.rs` — "USD·kg⁻¹"), so the substrate for 2b exists whenever it is wanted.

### D2 — Guard rule: "all terms share one dimension" (refinement over the stub)

The stub said "require each term to evaluate **dimensionless**". That is **too strong** — it would reject the single-aspect `Money` objectives `continuous-cost-minimisation` just shipped (`minimize cost` would suddenly need `/1USD`). The correct invariant is:

> **A multi-term `WeightedSum` is coherent iff all its terms share one dimension** (`dimension_of(term.expr.result_type)` equal across terms).

- Accepts: `minimize cost` (single term); `minimize cost_a` + `minimize cost_b` (all `Money` — a meaningful summed-cost objective); `minimize cost/1USD` + `minimize mass/1kg` (all dimensionless — the multi-aspect idiom).
- Rejects: `minimize cost` + `minimize mass` (`Money` vs `Mass`) → `E_OBJECTIVE_MIXED_DIMENSION`.

Weight multiplication preserves a term's dimension, so weights are irrelevant to the check; the dimension is available **statically** as `term.expr.result_type` (no evaluation needed — `dimension_of` already exists at `solver.rs:99`).

### D3 — Aspect vocabulary: independent traits; introduce `Massive { mass : Mass }`

To avoid the "cost is special" trap, co-design a second aspect trait alongside the sole existing one (`Costed`, `reify-compiler/stdlib/io.ri:111`):

- `trait Massive { param mass : Mass }` — an ordinary stdlib `.ri` trait, **no privileged path, no mandatory common supertrait**. (Verified: standalone traits like `trait Joint { }` are legal; `Mass` resolves as a first-class dimensioned type; `param mass : Mass` and `0.4kg` parse and check.)
- Further aspects (waste-value, part-count) follow the same shape and are named as exemplars, not all built here.
- **Aggregation uses explicit named-sub member access** — `[a.mass, b.mass].sum` (dimension-preserving `.sum`, `#4292`). Verified: this evaluates (`total_mass = 0.8 kg`, `total_cost = 6 USD` in a two-`Bracket` frame). **Correction to the stub's premise (D3-verification, 2026-07-05):** the `filter(self.descendants, Trait) → project the aspect field → .sum` idiom does **not** compose today — `filter(self.descendants, Massive).mass` yields `error: member access not yet supported: .mass`. `filter(…, Trait).count` (a built-in aggregate) *does* work (δ `#3991`), so the trait participates in structural queries, but **member projection off filtered descendants is unshipped** and is **not** required by this PRD. Aggregating a custom aspect field over `self.descendants` (rather than named subs) is out of scope; if wanted later it is a distinct member-projection substrate task, not part of M-UNITS.

### D4 — Guard locus: compile-time check + shared helper + fold-site backstops

Install the coherence check as a **compile-time diagnostic** in the compiler, mirroring `check_objective_conflict` (`entity.rs:650`) — it is a `reify check` error (best UX, fires before any solve) rather than a solver-time failure. Factor the predicate into a **shared** `objective_terms_coherent(terms) -> Result<(), (DimensionVector, DimensionVector)>` reachable from both `reify-compiler` and (for the merged-solve case) `reify-constraints`/`reify-eval`. The three numeric fold sites get `debug_assert!` backstops (coherence is guaranteed upstream by the compile check for authored objectives; the assert catches any un-checked set — e.g. a future merged set that skipped the gate). See the Contract section.

## Sketch of approach

- **α** installs `objective_terms_coherent` + the compile-time `E_OBJECTIVE_MIXED_DIMENSION` check + fold-site asserts. This is the load-bearing safety mechanism — it is what makes multi-term objectives (and, later, `#4785`'s merged cross-scope objective) safe.
- **β** adds `Massive` (and names waste/count exemplars), proving the aspect vocabulary generalises past `Costed`.
- **δ** is the user-observable integration gate: a CI `.ri` that solves a coherent normalised multi-aspect objective **and** a negative fixture that observes `E_OBJECTIVE_MIXED_DIMENSION` firing.
- **γ** (dimensioned weight / shadow-price, option 2/2b) is **not built** — documented above as deferred M-SHADOW.

The authoring idiom shipped is the single normalised expression (one coherent term) or normalised same-dimension multi-term; the guard hardens the fold so the raw-f64 triplication can never again silently produce nonsense.

## Contract section (the shared coherence seam) — H

**Invariant (I-UNITS).** For any `ObjectiveSet` with `combination == WeightedSum` and `terms.len() > 1`, all terms satisfy `dimension_of(term.expr.result_type)` equal. A set violating I-UNITS must never reach a numeric fold; it is rejected with `DiagnosticCode::ObjectiveDimensionIncoherent` at the earliest gate that owns it.

**Seam signature (single source of truth):**

```rust
// reachable from reify-compiler (authored objectives) AND reify-constraints/reify-eval (merged sets)
fn objective_terms_coherent(terms: &[ObjectiveTerm]) -> Result<(), DimensionIncoherence>;
struct DimensionIncoherence { first: DimensionVector, offending: DimensionVector, term_index: usize }
```

- **Compiler gate (owner: this PRD, α).** `entity.rs`, adjacent to `check_objective_conflict`: on a multi-term `WeightedSum`, call `objective_terms_coherent`; on `Err`, emit `error: E_OBJECTIVE_MIXED_DIMENSION` with both dimension names and the offending term's span. Covers every **authored** objective.
- **Fold-site obligation (owner: this PRD, α).** The three folds (`solver.rs:835`, `registry.rs:495`, `engine_eval.rs:2728`) carry a `debug_assert!(objective_terms_coherent(&obj.terms).is_ok())`. They do **not** re-diagnose (the compile gate already did) — they assert the invariant held.
- **Merged-set obligation (owner: `#4785`).** `#4785`'s cross-scope merge builds an `ObjectiveSet` at solve time that did not pass a single per-scope compile gate; it **must call `objective_terms_coherent` on the merged term set** and surface the diagnostic, rather than spawn a fourth un-guarded fold. This PRD owns the helper; `#4785` owns invoking it on merged sets.

**Error semantics.** `E_OBJECTIVE_MIXED_DIMENSION` is a hard `error` (not a warning) — an incoherent objective has no defensible solve. Single-term and same-dimension multi-term sets are unaffected (no regression to shipped single-aspect objectives).

## Boundary-test sketch — H

| # | Scenario | Preconditions | Postcondition (asserted) | Faces |
|---|---|---|---|---|
| BT1 | Mixed-dimension two-decl objective rejected | `minimize cost` + `minimize mass` (`Money`,`Mass`) | `reify check` exit 1, `E_OBJECTIVE_MIXED_DIMENSION` naming `Money`/`Mass` | author / compiler |
| BT2 | Same-dimension multi-term still legal | `minimize cost_a` + `minimize cost_b` (both `Money`) | `reify check` clean; solves | author / compiler |
| BT3 | Shipped single-aspect unaffected | `minimize cost` (one `Money` term) | `reify check` clean; solves (no regression) | author / compiler |
| BT4 | Coherent normalised multi-aspect solves | `minimize cost(thickness)/1USD + w*mass(thickness)/1kg` — cost & mass both closed-form in the scope's **own** auto param `thickness` (`auto(free)`) | `reify eval` resolves `thickness` to a feasible value, no `E_OBJECTIVE_MIXED_DIMENSION` (verified: resolves to `0.01 m`) | author / solver |
| BT5 | Fold-site invariant holds | any solve reaching `eval_objective_set` | `debug_assert` never trips (coherence upstream-guaranteed) | solver backstop |
| BT6 | Merged-set guard (cross-PRD) | `#4785` merged cross-scope objective with mixed dims | merged builder calls `objective_terms_coherent`, surfaces the diagnostic; no 4th fold site | `#4785` / this seam |

BT4 is δ's observable signal (closes G2). BT6 is `#4785`'s obligation, listed here so the seam contract is two-way.

## Pre-conditions for activating

All landed — this PRD is active:
- `#3991` structural-query δ (`filter(self.descendants, Trait)`).
- `#4292` BOM `.sum` roll-up (dimension-preserving aggregation).
- `#4795` `continuous-cost-minimisation.md` terminal (single-aspect `Money` objective baseline, `ObjectiveSet{WeightedSum}` → `DimensionalSolver`).

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `continuous-cost-minimisation.md` | consumes | `ObjectiveSet{WeightedSum}` + `DimensionalSolver` objective machinery (landed) | continuous-cost-min | wired |
| `whole-model-objective-coupling.md` (`#4785`) | produces-for | `objective_terms_coherent(terms)` — the merged cross-scope solve must route through this guard, not a 4th fold site | **this PRD** (helper) / `#4785` (invocation on merged sets) | queued |
| `shadow-price-objectives.md` (M-SHADOW, future 2b) | produces-for | dimensioned `weight` (`Money/aspect_dim`) → Money-valued objective + sensitivity | future PRD | deferred (breadcrumb only) |

**Orthogonality with `#4785` (confirmed).** The two PRDs move on orthogonal axes — `#4785` is scope-domain (`build_solver_problem`, which cells enter one solve), this PRD is value/units-domain (the fold arithmetic). `#4785` makes **no** `ObjectiveTerm`/`ObjectiveSet` shape change and consumes `weight` unchanged (`f64`) — **zero rebase** for `#4785`. **No landing-order dependency.** The one hard coordination requirement is captured as BT6 / the merged-set obligation above. **Out of scope for both:** a whole-model objective that is *also* multi-aspect (a joint downstream, owned by neither alone).

No new contested-ownership seam is introduced (checked against the overlay's three known pairs; this seam has a clear single owner for the helper).

## Decomposition plan

Labels are intra-batch (α, β, δ, ε); task IDs assigned at decompose time.

- **α — units-coherence guard at the (triplicated) fold sites.**
  - *Modules:* `reify-ir` (shared `objective_terms_coherent` helper + expose `dimension_of` / `DimensionVector` for `Type`), `reify-core/src/diagnostics.rs` (new `DiagnosticCode::ObjectiveDimensionIncoherent` / `E_OBJECTIVE_MIXED_DIMENSION`), `reify-compiler/src/entity.rs` (compile-time check next to `check_objective_conflict`), `reify-constraints/src/{solver.rs,registry.rs}` + `reify-eval/src/engine_eval.rs` (fold-site `debug_assert` backstops). Impl-site breadcrumb naming deferred 2b (M-SHADOW).
  - *Observable signal:* `reify check` on a 2-term mixed-dimension objective (`minimize cost` + `minimize mass`) emits `error: E_OBJECTIVE_MIXED_DIMENSION` naming both dimensions (**today it silently solves** — confirmed). Same-dimension multi-term and all shipped single-aspect objectives still check clean (BT1/BT2/BT3).
  - *Prereqs:* none new (all substrate landed). **The load-bearing task.**

- **β — aspect-trait vocabulary (`Massive`).**
  - *Modules:* `reify-compiler/stdlib/io.ri` (add `trait Massive { param mass : Mass }`; doc-note waste/count exemplars) + a stdlib example `.ri`.
  - *Observable signal:* a stdlib `.ri` in CI where a `structure def X : Costed + Massive` conforms (`reify check` clean) and **both** aspect fields aggregate via explicit named-sub member access — `[a.line_cost, b.line_cost].sum` and `[a.mass, b.mass].sum` (`reify eval` prints `total_cost = 6 USD`, `total_mass = 0.8 kg` — verified). (Aggregation is the *reporting* surface, distinct from the objective; NOT via `filter(…).mass` — see D3 correction.)
  - *Prereqs:* none new (`Costed`, `.sum` `#4292`, `Mass` type all landed). Independent of α.

- **δ — coherent multi-aspect CI example (integration gate).**
  - *Modules:* `examples/multi_aspect_objective.ri` (positive) + `examples/multi_aspect_objective_mixed.ri` (negative) + CI registration.
  - *Observable signal:* **positive** — `reify eval` on `minimize unit_cost*(thickness/1mm)/1USD + w*mass_per_mm*(thickness/1mm)/1kg` (cost & mass both closed-form in the scope's **own** `auto(free)` `thickness`) resolves `thickness` to a feasible value with no dimension error (BT4, verified `thickness = 0.01 m`). **Negative** — `minimize cost` + `minimize mass` (mixed) emits `E_OBJECTIVE_MIXED_DIMENSION` (BT1). Both run in CI.
  - *Prereqs:* **α** (guard/diagnostic for the negative) + **β** (`Massive` for the positive's aspect vocabulary).
  - *Note:* the objective is over the scope's **own** auto params. Minimising an aspect **aggregated over children** (driving child dimensions) is **cross-scope** — `#4785`'s territory, out of scope here (a parent objective sees child aspect values as frozen constants).

- **ε — companion doc-correction.**
  - *Modules:* `docs/prds/v0_6/continuous-cost-minimisation.md` (§10 row-4 pointer → this expanded PRD) + this PRD's status cross-links.
  - *Observable signal:* the parent PRD's out-of-scope row points at the realized M-UNITS mechanism (not the stub); prose-only.
  - *Prereqs:* α/β/δ landed (correct the record after the mechanism ships).

γ (dimensioned weight / shadow-price) is intentionally **absent** — deferred to M-SHADOW (out of scope below).

## Out of scope for this PRD

- **Dimensioned `weight` / first-class shadow prices (option 2b).** `weight = Money/aspect_dim`, Money-valued objectives, shadow-price sensitivity → future `shadow-price-objectives.md` (M-SHADOW). Breadcrumb at α's guard.
- **Project-everything-to-Money as the primary model (option 3).** Rejected (Money-privilege trap). Optional stdlib price-model helpers for monetisable aspects may be authored separately.
- **A whole-model objective that is also multi-aspect** (multi-aspect × cross-scope cross-product) — joint downstream of this PRD and `#4785`, owned by neither alone.
- **Automatic aspect enumeration / an `Aspect` marker supertrait.** Aspects are independent traits (D3); no aspect-generic machinery until a second consumer demands it.
- **Full ε-band Lexicographic staged solve.** Owned by the existing objective-set task ε lineage, not this PRD.

## Open questions (surfaced but not decided in this session)

1. **Guard locus if a merged-set-only path emerges before `#4785` lands.** α installs the compile-time gate + shared helper; if some non-`#4785` path builds multi-term sets at solve time before the merged-set obligation is wired, the `debug_assert` (not a user diagnostic) is the only backstop in release builds. **Suggested resolution:** keep the compile gate as the sole user-facing diagnostic; add a release-mode guarded return (`Infeasible{diagnostics}`) at `solve_with_meta` only if such a path is discovered. Decide during α.
2. **`Massive.mass` sign/units for assemblies with subtractive geometry** (mass of a pocketed part). Tactical — `Mass` is non-negative by convention; `.sum` over child masses is additive. **Suggested resolution:** document additive-only in β; revisit if a subtractive-mass aspect is needed. Decide during β.
3. **Whether ε folds into δ's commit or is a standalone task.** Tactical filing choice. Decide at decompose.

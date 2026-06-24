# Continuous closed-form cost minimisation (in-scope)

**Milestone:** v0_6 · **Status:** active (authored in interactive `/prd` session, 2026-06-24, under G1–G6+META) · **Approach:** B + H
**Cluster:** `cost-optimisation`. First of a staged program (see §10): this PRD ships the *in-scope, continuous, closed-form* slice; subtree/whole-model cost, discrete cost, geometry-dependent (material/waste) cost, and multi-aspect units-coherence are separate forward-stub milestones.

---

## §0 — Purpose and scope

Let a user say **"choose the auto values that minimise cost"** and have the existing dimensional solver select the cost-minimising point in the feasible region — *without* the result silently parking on a constraint boundary (a fragile, zero-margin design).

This is **not** greenfield optimisation. `minimize`/`maximize` objective declarations already ship end-to-end (grammar → `MinimizeDecl` → `ObjectiveTerm`/`ObjectiveSet{WeightedSum}` → `DimensionalSolver` Nelder-Mead penalty solve), and `Money`/`line_cost` cost vocabulary already evaluates. What is missing for *cost* specifically is:

1. The **idiom + examples** establishing cost-as-objective (cost is just a `Money`-dimensioned objective expression over the scope's own auto params).
2. A **robustness default** so cost-min doesn't drive designs onto constraint boundaries (the central UX problem — pure cost-min's optimum is almost always *on* a constraint).
3. An explicit **`cost_robustness_tradeoff(cost_expr, λ)`** override for users who want to dial the cost-vs-robustness balance.

### §0.1 — What this is NOT (scope boundaries, resolved 2026-06-24)

These are deliberately deferred to named successors so this PRD stays small and decisively shippable:

- **NOT subtree / whole-model cost.** A `minimize` objective optimises **only its own scope's** auto params (`build_solver_problem` collects only own-template auto cells; cross-scope reads are dropped; child scopes freeze before parents solve). So a parent's `minimize cost(self.descendants)` would see child costs as **frozen constants** — un-optimisable without a cross-scope coupling foundation. Subtree/whole-model cost-as-objective → **`whole-model-objective-coupling.md`** (M-WHOLE). The BOM **report** chain (`structural-query` γ #3988 / δ #3991 / `reify report --bom` #4292) is a *separate consumer* of the descendants walk and is unaffected by this PRD.
- **NOT discrete cost choices.** Supplier / stock-size / count selection (`Int`/`Bool`/`Enum` auto) is unsupported by the continuous solver and needs an enumeration/decomposition harness + a ranked-result carrier → **`discrete-cost-minimisation.md`** (PRD 2) over **`ranked-solve-result.md`** (F-result).
- **NOT geometry-dependent cost.** Material/waste cost that depends on realised kernel geometry (volume×density, offcut/nesting) cannot be evaluated in the solver's inner loop (it sees only the param `ValueMap`, never kernel output) → **`material-waste-cost-minimisation.md`** (M-WASTE).
- **NOT multi-aspect combination.** Combining cost with mass/count/etc. in one objective hits a dimensional-coherence hazard (the WeightedSum fold adds raw f64, i.e. `USD + kg`) → **`multi-aspect-objective-units-coherence.md`** (M-UNITS). This PRD ships **single-aspect** (`Money`-only) objectives.

---

## §1 — Consumer (G1)

| Mechanism | Consumer (user-observable surface) |
|---|---|
| Cost-as-objective idiom (`minimize <Money-expr-over-own-auto-params>`) | `examples/continuous_cost_min.ri` (CI): a part with an auto dimension and a closed-form material cost resolves (`reify eval`) to the cost-minimising dimension subject to its constraints. |
| `Money`-objective → robustness-floor default | The same example: the resolved auto value sits **off** the binding constraint by ≥ the margin (contrast: pure cost-min sits exactly on it). Observable via `reify eval` resolved value + an eval test asserting `value ≥ bound + margin`. |
| Robustness-floor info diagnostic | `reify check`/`eval` emits an info/`W_*` line noting a robustness floor was applied to a cost objective and how to override. |
| `cost_robustness_tradeoff(cost_expr, λ)` override | `examples/cost_robustness_tradeoff.ri` (CI): λ=1 resolves to the boundary (pure cost), λ=0 to the robust centre, an intermediate λ between — shown via `reify eval`. |

**Engine-integration sub-check (G1).** The solver-side mechanisms plug into the catalogued **§3.5 ConstraintSolver** seam (`engine-integration-norm.md`) — they extend `DimensionalSolver`/objective lowering, not a new seam. No orphan-producible `pub fn` in a `kernel-*` crate.

---

## §2 — Resolved design decisions

### §2.1 — Cost is an ordinary `Money`-dimensioned objective (no new aggregation surface)
Cost-min reuses the shipped `minimize <expr>`. The objective expression is any `Money`-dimensioned closed-form expression over the **scope's own** continuous auto params (and constants / material-property literals) — e.g. `minimize price_per_kg * density * volume_expr(self.thickness)`. No `cost(...)` aggregation builtin in this PRD (aggregation over sub-children is cross-scope → M-WHOLE). A scope that is itself `Costed` may `minimize self.line_cost` when `line_cost` is closed-form in its own auto params.

### §2.2 — The robustness-floor default is triggered by the objective's **dimension** `[load-bearing]`
When a scope's resolved objective is **`Money`-dimensioned**, the solver synthesises a **robustness floor**: every inequality constraint's signed slack must be ≥ a margin `m` (`slack_i ≥ m`), and cost is minimised subject to that floor. Non-`Money` objectives (e.g. the existing `0.7*mass − 0.3*stiffness`) are **unchanged** — this avoids any backward-compat break to `objective_set_weighted` and other shipped objectives.
- **Rationale.** Pure cost-min's optimum is generically *on* a constraint boundary (zero margin → fragile). The dimension trigger keeps the safe default scoped to exactly the case Leo flagged (cost-min UX) without re-litigating every objective.
- **Loud, not silent.** Applying the floor emits an info/`W_*` diagnostic naming the override — per the project "loud diagnostics over silent defaults" norm.
- **Breadcrumb (deferred alternatives).** Considered + rejected for v1: an opt-in `robust` keyword on `minimize` (needs grammar), and making the floor default for *all* objectives (breaks existing non-cost objectives). Record this at the implementation site with an "at time of writing" framing.

### §2.3 — Margin source: configurable default now, tolerance-tied as enhancement `[G3 — see §4]`
The floor needs a per-constraint margin `m`. The dimensionally-honest, engineering-meaningful source is the **per-purpose tolerance scope** ("stay at least a manufacturing-tolerance margin off every limit"). Whether that scope exposes a usable per-constraint margin is a **substrate question** (§4): v1 ships a **configurable default margin** (relative + absolute floor) so the slice is unblocked; the tolerance-tie is a follow-on task gated on the §4 investigation. The floor *mechanism* (synthesise `slack_i ≥ m` constraints) does not depend on the margin *source*.

### §2.4 — Override surface: `cost_robustness_tradeoff(cost_expr, λ)` as an objective special-form `[load-bearing]`
`minimize cost_robustness_tradeoff(<money-expr>, <λ:Real>)` is recognised by the compiler as a special objective form (analogous to the existing synthetic centrality objective). It produces a **normalised** scalarisation: solve the two anchor points (min-cost, max-robustness), normalise each term to [0,1] over its anchor range, and minimise `λ·ĉost + (1−λ)·(−r̂obustness)`. λ∈[0,1]: **λ=1** → pure cost (boundary), **λ=0** → pure robustness (the existing Chebyshev centre), intermediate → blended. Normalisation makes λ a true dimensionless dial and sidesteps the `USD + length` incoherence of a naïve weighted sum.
- When `cost_robustness_tradeoff` is present, it **replaces** the §2.2 floor default (the user has taken explicit control).
- Grammar-confirmed: `minimize cost_robustness_tradeoff(<expr>, 0.3)` parses today (§4).

### §2.5 — Determinism
Unchanged from the shipped solver: deterministic for a fixed seed; finds a **local** optimum (no multi-start). For the closed-form, single-aspect, in-scope objectives in scope here, the cost surface over the feasible box is typically monotone/convex, so local = global in practice. The PRD asserts no global-optimality claim (a global guarantee is F-result/PRD 2 territory). The two-anchor solve in §2.4 inherits the same local-determinism.

---

## §3 — Interactions & risks

- **Backward-compat:** the §2.2 dimension trigger confines the new default to `Money` objectives; existing non-`Money` objectives are byte-for-byte unchanged (pinned by re-running `objective_set_signal.rs` unchanged — boundary-test §8.2).
- **Floor feasibility:** a robustness floor can make a tight design **infeasible** (no point has ≥`m` slack). The solver must report this as a *distinct* diagnostic ("infeasible under robustness floor `m`; relax the margin or use `cost_robustness_tradeoff`") rather than a bare "infeasible", and the margin default must be conservative enough to rarely trip.
- **Caching / warm-eval:** in-scope continuous cost-min is regime-A — the objective is a cheap `CompiledExpr` over the param `ValueMap`; no realised geometry in the inner loop, so no extra kernel rebuilds and no interaction with the realisation cache beyond what `minimize` already incurs. (Regime-B is M-WASTE.)
- **`Real`-typed auto bounds:** an observed wrinkle — a bare `Real = auto` objective can resolve `infeasible/undef` where a `Length = auto` does not (default-bound differences). Tactical (§9); examples use dimensioned auto params (the proven `objective_set_weighted` pattern).

---

## §4 — Substrate gate (G3)

**Grammar (verified 2026-06-24, `tree-sitter parse --quiet`, fixtures `/tmp/prd-gate-fixtures/cm-*.ri`):** all surfaces parse — **NO grammar work required.**

| Fragment | Result |
|---|---|
| `minimize cost(self.descendants)` (not used in v1; tested for the family) | OK |
| `let total_cost : Money = cost(self.descendants)` | OK |
| `minimize cost_robustness_tradeoff(self.descendants, 0.3)` → with a `Money` expr arg | OK |
| `sum(flat_map(filter(self.descendants, Costed), \|c\| [c.line_cost]))` | OK |
| bare `self` as a call arg | OK |

`cost_robustness_tradeoff` parses as an ordinary call; its *semantics* are new work owned by this PRD (the compiler special-form recognition + solver blend). `minimize <expr>` and `Money` arithmetic are shipped.

**Semantic/behavioural substrate:**
- **Verified shipped (calibration, `reify eval`):** `crates/reify-eval/tests/fixtures/objective_set_weighted.ri` resolves `mass → 1µm`, `stiffness → 10m` — the `minimize`→`DimensionalSolver` pipeline drives auto params observably. The cost case is the same pipeline with a `Money` objective.
- **Open substrate question (resolved by a queued prereq, not an assumption):** does the per-purpose tolerance scope (`engine_purposes.rs` / `tolerance_scope.rs`) expose a per-constraint margin usable as the §2.3 floor source? **Task α investigates and reports; v1 does not depend on the answer** (configurable default margin). This is a G3-clean resolution (queue the substrate work; the dependent enhancement, task δ, gates on it).

---

## §5 — Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `whole-model-objective-coupling.md` (M-WHOLE) | produces-for | subtree/whole-model cost-as-objective + cross-scope coupling; this PRD's `Money`-objective + robustness machinery is reused, scaled to spanning scopes | M-WHOLE | queued (stub) |
| `discrete-cost-minimisation.md` (PRD 2) over `ranked-solve-result.md` (F-result) | sibling | discrete/mixed cost via enumeration + ranked results; orthogonal to this PRD's continuous solve | PRD 2 / F-result | queued (stub / spawned) |
| `material-waste-cost-minimisation.md` (M-WASTE) | produces-for | geometry-dependent cost via an outer candidate loop over `ranked-solve-result` + `cache-input-cone-rekey` (F-cache) | M-WASTE | queued (stub) |
| `multi-aspect-objective-units-coherence.md` (M-UNITS) | produces-for | multi-aspect dimensional-coherence at objective combination; this PRD stays single-aspect (`Money`) | M-UNITS | queued (stub) |
| `structural-query-traversal.md` (γ #3988 / δ #3991) + `io-lifecycle-bom-cost.md` (#4292) | non-seam (this PRD) | the descendants walk + BOM **report** are a *separate consumer*; this PRD's in-scope objective does not use them | n/a (they belong to M-WHOLE / report) | independent |

No reciprocal-ownership ambiguity: this PRD owns the in-scope `Money`-objective robustness default + the `cost_robustness_tradeoff` special-form; every cross-scope/discrete/geometry/multi-aspect extension is owned by a named successor.

---

## §6 — G6 premise validation

- **"auto value resolves to the cost-minimising point" (branch 3, end-to-end):** every required capability — `minimize` lowering, `DimensionalSolver` objective solve, `Money` arithmetic — is **shipped** (calibrated §4); the robustness floor + special-form are delivered by this PRD's own tasks (α/β/γ). Nothing is owed by a downstream task. **Pass.**
- **"resolved value sits ≥ margin off the binding constraint" (branch 1, comparison):** not an accuracy bound against a numerical floor — it is a structural property of the synthesised `slack ≥ m` constraint; achievable by construction whenever the floor is feasible (§3 feasibility caveat handled by a distinct diagnostic). **Pass.**
- **"λ=1 → boundary, λ=0 → robust centre" (branch 3):** λ=0 reduces to the existing Chebyshev-centre objective (shipped `build_centrality_objective`); λ=1 reduces to pure cost (shipped penalty solve); the normalised blend is this PRD's task γ. In-set. **Pass.**
- No closed-form-exactness (branch 2) or rejection (branch 4) premises beyond the standard "unknown override arg → diagnostic" (delivered by γ's typing).

---

## §7 — Approach: B + H (G5)

G5 triggers: blast radius ≥ 3 crates (`reify-constraints` floor synthesis + blend; `reify-compiler` `Money`-objective detection + `cost_robustness_tradeoff` special-form typing; `reify-eval` objective plumbing + diagnostic; `examples/`), **and** it touches the load-bearing **ConstraintSolver** seam. → **B + H.**

### §8.1 — Seam contract (signatures, invariants)

- **Robustness floor.** In `DimensionalSolver` problem assembly: when `objective` is `Money`-dimensioned and the scope has ≥1 inequality constraint, for each inequality with signed slack `s_i(x)` add a synthetic constraint `s_i(x) ≥ m` (margin `m` from §2.3). **Invariants:** (i) applied iff objective dimension is `Money`; (ii) non-`Money` objectives produce a bit-identical problem to today; (iii) floor-infeasibility surfaces as a *distinct* diagnostic, never a bare "infeasible"; (iv) deterministic.
- **`cost_robustness_tradeoff(c: Money, λ: Real) -> <objective>`.** Compiler recognises it in objective position; `λ` must be a compile-time-known `Real` in [0,1] (else a named diagnostic); first arg must be `Money`-dimensioned (else diagnostic). Solver: two anchor solves (min `c`; max min-slack), normalise each to its anchor range, minimise `λ·ĉ + (1−λ)·(−r̂)`. **Invariants:** λ=0 ≡ Chebyshev-centre objective; λ=1 ≡ pure-cost penalty solve; replaces the §2.2 floor when present; deterministic (anchors solved with the fixed seed).
- **Margin source.** `margin_for(constraint) -> Scalar` — v1: configurable default (relative-of-range with an absolute floor); δ: tolerance-scope-derived when available. Contract stable across source.

### §8.2 — Boundary-test sketch (faces solver + compiler)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Floor applies to Money objective** | scope: `param t:Length=auto`, `constraint t > 1mm`, `minimize <price·t>` | resolved `t ≥ 1mm + m` (off boundary); info diagnostic emitted |
| **Non-Money objective unchanged** | `objective_set_weighted.ri` (re-run verbatim) | `mass → ~1µm`, `stiffness → ~10m` — **no floor**, values unchanged |
| **Floor-infeasible** | tight box where no point has ≥`m` slack | distinct "infeasible under robustness floor" diagnostic (not bare infeasible) |
| **Tradeoff λ=1 (compiler+solver)** | `minimize cost_robustness_tradeoff(<money>, 1.0)` | resolves to the boundary (pure cost); no floor |
| **Tradeoff λ=0** | `… , 0.0)` | resolves to the robust centre (== Chebyshev-centre result) |
| **Tradeoff λ=0.5** | `… , 0.5)` | resolves strictly between the λ=0 and λ=1 points |
| **Bad override arg** | `cost_robustness_tradeoff(<non-money>, 2.0)` | named compile diagnostic(s): non-Money arg and λ∉[0,1]; no panic |

The integration-gate leaf (task β) names the "floor applies to a Money cost objective, value lands off the boundary" end-to-end `.ri` as its observable signal (closes G2).

---

## §8 — Decomposition plan

Labels α…ε; IDs at decompose time. Spine (C-as-integration-gate): α → β → γ. **No grammar prerequisite** (§4).

### Phase 1 — Robustness-floor foundation
- **Task α — Solver: synthesise a robustness floor (`slack_i ≥ m`) for `Money`-dimensioned objectives; configurable default margin; floor-infeasibility diagnostic. Investigate tolerance-scope as a margin source and record the finding.**
  - **Observable signal:** eval test — a `.ri` with `minimize <price·t>` + `constraint t > 1mm` resolves `t ≥ 1mm + m` (off the boundary), where pure cost-min would give `t == 1mm`; floor-infeasible fixture emits the distinct diagnostic. **Intermediate** (unlocks β), eval-observable. `grammar_confirmed=true`. Crates: reify-constraints, reify-eval.
  - **Prereqs:** none.

### Phase 2 — Vertical slice (the headline)
- **Task β — Integration: `Money`-objective robustness default wired end-to-end + the canonical cost-min example.**
  - **Observable signal:** `examples/continuous_cost_min.ri` (CI-run): a part with `param thickness:Length=auto`, a closed-form material cost, and a stress/clearance inequality; `reify eval` resolves `thickness` to the cost-minimising value that respects the robustness floor (off the boundary); info diagnostic present. **Leaf — integration gate** (§8.2 headline). `grammar_confirmed=true`. Crates: examples/, reify-eval.
  - **Prereqs:** α.

### Phase 3 — Tradeoff override
- **Task γ — Compiler + solver: `cost_robustness_tradeoff(cost_expr, λ)` special-form (recognition + typing + normalised two-anchor blend); arg diagnostics.**
  - **Observable signal:** `examples/cost_robustness_tradeoff.ri` (CI): `reify eval` shows λ=1 → boundary, λ=0 → robust centre, λ=0.5 → strictly between; `cost_robustness_tradeoff(<non-money>, 2.0)` emits named diagnostics, no panic. **Leaf.** `grammar_confirmed=true`. Crates: reify-compiler, reify-constraints.
  - **Prereqs:** β (floor + Money-objective detection in place), α.

### Phase 4 — Tolerance-tied margin (enhancement, gated on α's finding)
- **Task δ — Margin source: derive the robustness margin from the per-purpose tolerance scope when available (replacing the configurable default).**
  - **Observable signal:** eval test — changing a purpose's tolerance changes the resolved auto value's standoff from the boundary on a `.ri`. **Leaf.** If α reports the tolerance scope can't supply a per-constraint margin, this task is **dropped to a deferred follow-up** (configurable default remains). `grammar_confirmed=true`. Crates: reify-eval, reify-constraints.
  - **Prereqs:** α (the finding), β.

### Phase 5 — Companion docs
- **Task ε — Spec/docs: document cost objectives + the robustness default + `cost_robustness_tradeoff`; cross-link the M-WHOLE / PRD 2 / M-WASTE / M-UNITS successors.**
  - **Observable signal:** committed doc section; the four successor PRDs are cross-referenced. **Leaf** (docs). `grammar_confirmed=true`. Crates: docs/.
  - **Prereqs:** γ.

### Dependency view
```
α → β → γ → ε
α ───────→ δ        (δ gated on α's tolerance finding; may defer)
```

---

## §9 — Open (tactical) questions
1. **Default margin shape** — relative-of-range vs absolute vs `max(rel, abs_floor)`; the conservative default value. Decide at α.
2. **`Real = auto` default bounds** — confirm/why a bare `Real` auto objective can go infeasible where `Length` does not; document or fix. Tactical; surface at β.
3. **Diagnostic codes** — the `W_*`/info code for "robustness floor applied" and the floor-infeasible `E_*`. Decide at α.
4. **λ compile-time vs runtime** — v1 requires λ compile-time-known (two-anchor solve). A runtime/auto λ is future work. Confirm at γ.
5. **Anchor-solve cost** — `cost_robustness_tradeoff` does two extra solves; acceptable for v1 (opt-in path). Revisit if hot.

## §10 — Out of scope → named successors
- Subtree / whole-model cost-as-objective + cross-scope coupling → `whole-model-objective-coupling.md` (M-WHOLE).
- Discrete supplier/stock-size/count cost → `discrete-cost-minimisation.md` (PRD 2) over `ranked-solve-result.md` (F-result).
- Geometry-dependent material/waste cost (outer loop) → `material-waste-cost-minimisation.md` (M-WASTE) over `cache-input-cone-rekey.md` (F-cache).
- Multi-aspect (cost+mass+…) dimensional coherence → `multi-aspect-objective-units-coherence.md` (M-UNITS).

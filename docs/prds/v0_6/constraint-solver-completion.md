# Constraint-Solver Completion

Status: contract. Authored 2026-05-27 in interactive `/prd` session under G1–G6+META gates. Spec-gap-filling batch `spec-gap-2026-05-27`, cluster `constraint-solver-completion`. **AskUserQuestion was unreachable in the authoring environment; the load-bearing design forks are resolved by reasoned default below and re-stated in `## DESIGN FORKS FOR LEO`. Tuples are explicitly NOT being added (batch constraint) — no design decision below relies on them.**

Completes the partially-built constraint/optimization system described in spec §10. The single-objective scaffold (`minimize`/`maximize`, scope-level objectives, bottom-up resolution) already ships; this PRD fills the six gaps the §10 survey flagged. These are **refinements to an existing partial system**, not greenfield work, so the decomposition extends the seams that already exist (named in `## §7 — Touched seams`) rather than introducing new ones.

Resolves (all past v0.2; version labels ignored):
- **§10.4 multi-objective** — weighted-sum (the spec's stated default) and lexicographic ordering. Today a scope holds at most **one** `OptimizationObjective` (a single `Minimize`/`Maximize`).
- **§10.7 conflicting-objectives diagnostic** — two objectives in one scope today **silently overwrite** (last wins); the spec mandates an error unless combined/prioritised.
- **§10.7 / §18 row 1 default robustness (centrality) objective** — "among feasible values, prefer those maximising distance from constraint boundaries"; not implemented (the §18 appendix lists it as the very first deferred item).
- **§10.7 objective legibility** — "designer can always query what objective governs a given `auto` resolution"; objectives are tracked internally with no inspection surface.
- **§10.2 proposing-mode "what is constrainable" feedback** — infeasibility is reported, but underdetermined designs get no actionable guidance on which parameters are free.
- **§10.6 bottom-up resolution coupling detection** — leaf-first resolution ships; the spec says "implementation should detect coupling and surface diagnostics" — cross-scope coupling is currently undetected.

---

## §0 — Why one PRD, not a small set

The cluster brief asked whether this is one PRD or a few (e.g. multi-objective / proposing-legibility / coupling). **One PRD, phased.** The deciding factor is a single load-bearing type:

`reify_ir::OptimizationObjective` is today `enum { Minimize(CompiledExpr), Maximize(CompiledExpr) }` and `ResolutionProblem.objective` / `CompiledPurpose.objective` / `Engine.objectives` / `Engine.active_objective_map` all hold `Option<OptimizationObjective>` — strictly **one objective per scope**. Five of the six gaps (everything except proposing-mode and coupling-detection) are blocked behind widening that one type into a multi-objective container with a combination strategy. If multi-objective and legibility were separate PRDs they would contend for ownership of that type (a reciprocal-seam fight, exactly the G4 failure mode). Conflict-diagnostic and centrality-default are semantics *on* the same widened type. Legibility is a read surface *over* it. So the first five gaps are one tightly-coupled vertical.

Proposing-mode feedback (§10.2) and coupling-detection (§10.6) are looser — they read objective/determinacy state but don't mutate the objective type. They could in principle be a second PRD. They stay here because (a) both are small, (b) both consume the same legibility/determinacy substrate this PRD builds, and (c) keeping the whole §10 completion in one DAG lets the decomposition wire the dependencies as real edges instead of cross-PRD references. The phases are cleanly separable inside the DAG; the PRD boundary doesn't need to be.

---

## §1 — Consumer and user-observable surface (G1)

**Primary consumer:** the designer authoring a multi-objective parametric optimization in a `.ri` file, driving it through the `reify` CLI (`reify eval`, `reify check`). This is a real, present consumer — `examples/m5_purpose.ri`, `examples/m10_purpose_activation.ri`, and `examples/integration_full_v01.ri` already use `minimize`/`maximize` today.

**Who reads the legibility surface?** Two named readers:
1. The same CLI designer, via a new `reify explain <file>` subcommand that prints, per `auto` cell, which objective governed its resolution and (where applicable) the active weighting/priority and the chosen combination strategy.
2. The GUI auto-resolve panel and the LSP, which already surface determinacy state (`crates/reify-cli/src/mcp_context.rs` maps `DeterminacyState`); the legibility data structure this PRD adds (`ObjectiveProvenance`) is the substrate they will later render. **This is a declared future consumer, not a wired one** — the CLI `reify explain` path is the in-batch leaf that proves the surface end-to-end (G1 satisfied by the CLI; GUI/LSP wiring is out of scope, §10).

Every mechanism this PRD introduces and its consumer:

| Mechanism | Consumer |
|---|---|
| `OptimizationObjective` widened to a multi-objective container + `ObjectiveCombination` strategy | `ResolutionProblem` (solver), `SolverRegistry::solve`, compiler lowering |
| Weighted-sum combination in the dimensional solver | `DimensionalSolver` cost function (`crates/reify-constraints/src/solver.rs`) |
| Lexicographic combination (staged solve) | `SolverRegistry::solve` (sequences sub-solves) |
| Conflicting-objectives diagnostic `E_OBJECTIVE_CONFLICT` | compiler entity-build (`crates/reify-compiler/src/entity.rs`); surfaced by `reify check` |
| Default centrality (robustness) objective | `DimensionalSolver` when a scope has `auto` params, constraints, and no explicit objective; surfaced by `reify eval` resolving to a centred value |
| `ObjectiveProvenance` legibility record | `reify explain` CLI subcommand (in-batch leaf); GUI/LSP (future) |
| Proposing-mode "free parameters" report `W_UNDERDETERMINED` | `reify check` / `reify explain` on an underdetermined design |
| Bottom-up coupling diagnostic `W_SCOPE_COUPLING` | `reify check` on a model where a child scope's resolution depends on a not-yet-resolved sibling/parent |

No mechanism is an in-engine seam in the `engine-integration-norm.md §3` sense — all plug into the existing **§3.5 ConstraintSolver** seam, which the norm already catalogues. Confirmed: this PRD introduces **no new engine seam**.

---

## §2 — Approach: B+H

G5 fires: the constraint solver is a load-bearing seam (the overlay names it implicitly via §3.5; `ConstraintSolver` is the core domain engine path), cross-crate blast radius is 4 (`reify-ir`, `reify-compiler`, `reify-constraints`, `reify-eval`, plus `reify-cli` for surfaces = 5), and the purposes-completion sibling cluster is a cross-PRD consumer of the widened objective type. So this is **B + H**: a contract section (§6) pins the widened type's signatures + invariants, and a boundary-test sketch (§8) faces both the compiler-producer side and the solver-consumer side of the objective seam. The integration-gate task (phase 2) names that boundary-test sketch as its observable signal.

---

## §3 — Sketch of approach

### §3.1 — Multi-objective container (foundation, phase 1)

Widen the IR type. Today:

```
pub enum OptimizationObjective {
    Minimize(CompiledExpr),
    Maximize(CompiledExpr),
}
```

becomes a **single-objective term** plus a **container** that holds one-or-more terms and a combination strategy:

```
pub struct ObjectiveTerm {
    pub sense: ObjectiveSense,      // Minimize | Maximize
    pub expr: CompiledExpr,
    pub weight: f64,                // default 1.0 (equal-weight)
    pub priority: u32,              // default 0 (all same rank = pure weighted-sum)
}
pub enum ObjectiveCombination { WeightedSum, Lexicographic }
pub struct ObjectiveSet {
    pub terms: Vec<ObjectiveTerm>,  // non-empty by construction
    pub combination: ObjectiveCombination,
}
```

`ResolutionProblem.objective`, `CompiledPurpose.objective`, `Engine.objectives`, `Engine.active_objective_map`, and `TopologyTemplate.objective` change from `Option<OptimizationObjective>` to `Option<ObjectiveSet>`. The single-term construction path is preserved (one `Minimize`/`Maximize` decl → an `ObjectiveSet` with one term, `WeightedSum`, weight 1.0) so existing single-objective behaviour is byte-for-byte unchanged.

**Why a single new type rather than keeping the enum and adding a `Vec`:** the spec (§10.4) defines weighted-sum as the *default* multi-objective semantics and lexicographic as an *explicit extension*. A flat `ObjectiveSet { terms, combination }` represents both with one shape; the combination field is the explicit-extension switch. This is the contract pinned in §6.

### §3.2 — Multi-objective semantics (phase 3)

**Weighted-sum** (default): the dimensional solver already builds a penalty-method cost (`crates/reify-constraints/src/solver.rs` — Nelder-Mead, `eval_objective`). Extend `eval_objective` to fold `terms` into one scalar: `Σ wᵢ · sᵢ · eval(exprᵢ)` where `sᵢ = +1` for minimize, `−1` for maximize (the solver minimises cost). G6: this is mechanically sound — it's a linear combination of already-evaluable expressions, no new numerical capability required.

**Lexicographic** (explicit): `SolverRegistry::solve` sequences sub-solves by descending priority — solve for the rank-1 objective, freeze its optimum as an equality (or ε-band) constraint, solve rank-2 subject to that, etc. G6 hazard (acknowledged, §11): the freeze-as-equality between stages is an approximation; with Nelder-Mead's iteration-limit non-convergence (documented on `SolveResult::Solved`), a hard equality can over-constrain. **Resolved design decision:** lexicographic stages freeze to an **ε-band** (`|obj − obj*| ≤ ε·|obj*|`) not a hard equality, with ε a solver constant; the legibility record reports the realised rank-1 value so the designer can see the trade-off.

### §3.3 — Conflicting-objectives diagnostic (phase 3, paired with semantics)

Today `crates/reify-compiler/src/entity.rs` does `objective = Some(...)` on each `minimize`/`maximize` decl — the **second silently overwrites the first**. New rule at entity-build: collect **all** objective decls in a scope into an `ObjectiveSet`. The spec's "conflict without weighting = error" maps to: **>1 term with all-default weights (1.0) and all-default priority (0) and at least one Minimize *and* one Maximize over distinct expressions** ⇒ `E_OBJECTIVE_CONFLICT`. The designer resolves by (a) assigning weights, (b) assigning priorities (lexicographic), or (c) combining into a single arithmetic expression (`minimize 0.7*mass - 0.3*stiffness`, which already parses — see §5). Two same-sense default-weight objectives (`minimize mass` + `minimize cost`) are **not** an error — they're an equal-weight sum, the spec's stated default.

### §3.4 — Default centrality / robustness objective (phase 4)

Spec §10.7 / §18 row 1: when a scope has `auto` params + constraints + **no** explicit objective, the default is "maximise distance from constraint boundaries (centrality in the feasible region)." Mechanically defined (G6: this is a real, computable objective, not a guess): for inequality constraints `gⱼ(x) ≥ 0` reachable from the auto params, **maximise the minimum normalised slack** `min_j gⱼ(x)/scaleⱼ` (a max-min / Chebyshev-centre formulation). The dimensional solver synthesises this as an internal `ObjectiveSet` (one Maximize term over the min-slack expression) when no user objective is present. **Resolved design decision:** centrality applies **only** to the continuous dimensional path (it needs a differentiable-ish slack; Nelder-Mead handles the non-smooth min via the penalty machinery already present). For pure-discrete (CP-SAT) and pure-geometric (SolveSpace) scopes, the default stays "first feasible" (no centrality) — see §11 G6 note and §10 out-of-scope. This is gated behind a pragma-or-default decision in §DESIGN FORKS.

### §3.5 — Objective legibility (phase 5)

A new `ObjectiveProvenance` record produced during resolution and attached to the `EvalResult`: per `auto` cell, which `ObjectiveSet` governed it, the combination strategy, the per-term realised contribution, and whether the value is the centrality default vs. an explicit objective. Surfaced by a new `reify explain <file>` subcommand that prints a per-cell table. Reuses the `active_objectives()` accessor pattern already on the engine (`crates/reify-eval/src/engine_purposes.rs:295`).

### §3.6 — Proposing-mode "what is constrainable" (phase 6a)

Spec §10.2 proposing mode: on an **underdetermined** scope (auto params whose value is not uniquely pinned — `SolveResult::Solved { unique: false }` or auto params absent from any constraint), emit `W_UNDERDETERMINED` listing the free parameters and, for each, which constraints (if any) touch it. This reuses the determinacy snapshot (`PersistentMap<ValueCellId, (Value, DeterminacyState)>`) already threaded through `ConstraintInput`. Surfaced by `reify check` and `reify explain`.

### §3.7 — Bottom-up coupling detection (phase 6b)

Spec §10.6: bottom-up (leaf-first) resolution is an approximation when scopes are coupled. Detection: after the per-scope solve walk, check whether any **already-resolved** leaf scope's solved auto values are read by a constraint or objective in a **sibling or ancestor** scope that resolves later — i.e. the leaf was frozen before a coupling edge to it was considered. Emit `W_SCOPE_COUPLING` naming the coupled scopes and the crossing cell. This is a static read-set analysis over the per-scope `ResolutionProblem`s the engine already builds (`build_solver_problem`), not a solver change.

---

## §4 — Resolved design decisions

1. **One PRD, phased** (§0). The shared `OptimizationObjective`→`ObjectiveSet` widening forbids splitting multi-objective from legibility without a reciprocal-seam fight.
2. **`ObjectiveSet { terms, combination }` flat shape** (§3.1), not enum-of-vecs. Weighted-sum is the default `combination`; lexicographic is the explicit switch.
3. **Backward-compatible single-objective path** — one `minimize`/`maximize` lowers to a one-term `WeightedSum` set, weight 1.0; existing behaviour unchanged. This is a hard invariant, pinned in §6.
4. **Weighted-sum via expression folding in the existing Nelder-Mead cost** (§3.2); no new optimizer.
5. **Lexicographic via staged sub-solves with ε-band freeze** (§3.2), not hard-equality freeze, to survive Nelder-Mead non-convergence.
6. **Conflict = mixed-sense, distinct-expr, all-default-weight, >1 term** (§3.3). Same-sense equal-weight is the spec's default sum, not a conflict.
7. **Centrality = max-min normalised slack (Chebyshev centre), continuous path only** (§3.4). Discrete/geometric scopes keep "first feasible." Whether centrality is **on by default** vs. **opt-in via pragma** is a DESIGN FORK (default: on, matching spec §10.7 "default purpose applies").
8. **Legibility via `ObjectiveProvenance` on `EvalResult`, surfaced by `reify explain`** (§3.5). GUI/LSP consumption is future, out of scope.
9. **Weight/priority syntax: NO new grammar in this PRD** (G3, §5). Explicit weights/priorities are expressed via the combined-expression form (`minimize 0.7*mass - 0.3*stiffness`, which parses) **or** deferred to a follow-up grammar PRD. See §5 and DESIGN FORK 2.

---

## §5 — Grammar gate (G3)

Ran the overlay grammar gate (`tree-sitter parse --quiet` from `tree-sitter-reify/`) on every candidate syntax fragment. Fixtures live under the session fixture dir (`/tmp/prd-gate-fixtures/csc-*.ri`).

| Fragment | Intent | Parse | Verdict |
|---|---|---|---|
| `minimize mass` + `maximize stiffness` (two bare decls, one scope) | conflict case + equal-weight sum | **exit 0 — parses** | No grammar work. Semantics only. |
| `minimize 0.7 * mass - 0.3 * stiffness` | explicit weighted-sum via arithmetic | **exit 0 — parses** | No grammar work. The supported explicit-weight path. |
| `minimize mass where thickness > 2mm` | guarded objective | **exit 0 — parses** | Already supported (guard clause). |
| `minimize mass weight 0.7` | trailing-keyword explicit weight | **exit 1 — FAILS** (`ERROR` at `weight`) | Novel syntax. **Out of scope**; if wanted, follow-up grammar PRD. |
| `minimize mass priority 1` | trailing-keyword lexicographic rank | **exit 1 — FAILS** | Novel syntax. **Out of scope**; follow-up grammar PRD. |
| `minimize(mass, weight: 0.7)` | call-style named-arg | **exit 1 — FAILS** | Novel syntax. Rejected. |

**Decision (G3-clean):** this PRD ships multi-objective semantics using **only syntax that parses today**. Explicit weights and lexicographic priorities are expressed through:
- **Weighted-sum:** the combined arithmetic expression form (`minimize 0.7*mass - 0.3*stiffness`) — fully covered by the widened `ObjectiveSet` with a single folded term, *and* the multi-decl equal-weight sum (multiple `minimize`/`maximize` decls → multi-term `WeightedSum`).
- **Lexicographic:** **deferred** to a follow-up grammar PRD, because there is no parsing surface for per-objective priority today and the combined-expression trick cannot express a strict priority ordering. The `ObjectiveCombination::Lexicographic` variant and the staged-solve machinery are still built (the IR/solver support it), but **no `.ri` source can select it** until the grammar PRD lands. This is declared in §10 (out of scope: lexicographic *surface syntax*) and §11.

Every shipped task in this PRD therefore has `grammar_confirmed: true`. No grammar prerequisite task is queued.

---

## §6 — Contract: the objective seam (B+H)

The widened objective type crosses three boundaries: compiler (producer), purposes (sibling cross-PRD producer), solver (consumer). Pinned signatures + invariants:

### §6.1 — Type signatures (`reify-ir::constraint`)

```
pub enum ObjectiveSense { Minimize, Maximize }

pub struct ObjectiveTerm {
    pub sense: ObjectiveSense,
    pub expr: CompiledExpr,
    pub weight: f64,     // > 0; default 1.0
    pub priority: u32,   // default 0; higher = solved first in Lexicographic
}

pub enum ObjectiveCombination { WeightedSum, Lexicographic }

pub struct ObjectiveSet {
    pub terms: Vec<ObjectiveTerm>,   // INVARIANT: non-empty
    pub combination: ObjectiveCombination,
}
```

### §6.2 — Invariants

- **I1 (non-empty):** an `ObjectiveSet` always has ≥1 term. A scope with no objective decls and no centrality default holds `None`, never an empty set.
- **I2 (single-term identity):** a one-term `WeightedSum` set with weight 1.0 produces *bit-identical* solver behaviour to the old single `Minimize`/`Maximize`. Regression-guarded.
- **I3 (sense folding):** in `WeightedSum`, the solver cost contribution of term `i` is `wᵢ · σ(senseᵢ) · eval(exprᵢ)` with `σ(Minimize)=+1`, `σ(Maximize)=−1`. The whole cost is minimised.
- **I4 (priority partial order):** `Lexicographic` sorts terms by descending `priority`; ties within a priority rank fold as a `WeightedSum` at that rank.
- **I5 (centrality is synthetic):** the centrality default is materialised as an `ObjectiveSet` by the solver/engine, never written by the compiler; `ObjectiveProvenance` flags it `synthetic_centrality = true` so legibility distinguishes it from a user objective.
- **I6 (combination is a closed set):** `ObjectiveCombination` is exhaustively `{WeightedSum, Lexicographic}`; adding variants is a breaking change requiring a contract amendment.

### §6.3 — Error semantics

- `E_OBJECTIVE_CONFLICT` (compile-time, `reify check`/compile): >1 term, mixed sense over distinct exprs, all default weight+priority. Resolution hint in the diagnostic names the three escapes (weights / priorities / combined expression).
- Lexicographic with `combination = Lexicographic` but all-equal priorities ⇒ a `W_*` warning that it degenerates to weighted-sum (not an error).

---

## §7 — Touched seams (G4 — declare, don't wire)

This PRD **extends** existing seams; it owns the extension. The seam-owner table:

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `purposes-completion` (sibling, authored in parallel) | consumes | `CompiledPurpose.objective : Option<ObjectiveSet>` — the widened type | **this PRD owns the type change** | declared, NOT wired |
| `engine-integration-norm.md §3.5 ConstraintSolver` | extends | `ConstraintSolver::solve` / `ResolutionProblem.objective` | this PRD | extension owned here |
| `solver-hint-payloads.md` (`@solver_hint`) | adjacent | `@solver_hint` changes *search strategy / discrete sets*; this PRD changes *what is optimised* | disjoint — no shared mechanism | no seam |
| `@optimized` annotation | adjacent | `@optimized` is a fast-path *checker* swap (per `constraint.rs` OptimizedImpl scope note); does not touch objectives | disjoint | no seam |

**Purposes relationship (the load-bearing cross-cluster declaration).** The sibling `purposes-completion` cluster states "purposes can carry objectives." Today `CompiledPurpose.objective` is `Option<OptimizationObjective>` and `Engine::activate_purpose` rewrites + injects it into `active_objective_map` (`crates/reify-eval/src/engine_purposes.rs`). **This PRD owns the widening of `OptimizationObjective` → `ObjectiveSet`.** The purposes cluster consumes the new type but does not define it. The single concrete coupling: `activate_purpose`'s objective-rewrite loop (lines ~107–182) currently matches `Minimize(expr) | Maximize(expr)` to remap entity refs; after the widening it must iterate `set.terms`. **This is declared as a companion correction-task in the decomposition (phase 7)** so the purposes cluster's decomposition does not have to own a change to this PRD's type. We do NOT wire a dependency edge into the purposes cluster's tasks (they may not be filed yet); the correction-task lives in this batch and is self-contained (it edits `engine_purposes.rs`, which this PRD already touches).

**Relationship to `@solver_hint` / `@optimized` (brief asked to relate):** both are *orthogonal*. `@solver_hint` (solver-hint-payloads PRD) nudges the *search* (discrete candidate sets, preferred strategy) — it changes *how* the solver looks, not *what* it optimises. `@optimized` swaps in a fast equivalent *checker* and (per the `OptimizedImpl` scope note in `constraint.rs`) does not even participate in the solve/objective path today. This PRD changes the *objective* — the thing being optimised. No mechanism is shared; no seam ownership to resolve.

---

## §8 — Boundary-test sketch (B+H, the integration-gate signal)

Scenarios facing both the compiler-producer side and the solver-consumer side of the objective seam. The phase-2 integration-gate task names this table as its observable signal.

| # | Scenario | Preconditions | Postcondition (asserted) |
|---|---|---|---|
| B1 | Single `minimize mass` round-trips unchanged | one objective decl | solved values bit-identical to pre-widening (I2); `reify eval` output unchanged |
| B2 | Two same-sense decls (`minimize mass` + `minimize cost`) | both default weight | `ObjectiveSet` has 2 terms, `WeightedSum`; solve minimises the sum; no `E_OBJECTIVE_CONFLICT` |
| B3 | Mixed-sense default-weight (`minimize mass` + `maximize stiffness`) | distinct exprs | compile emits `E_OBJECTIVE_CONFLICT`; `reify check` exits non-zero with the code |
| B4 | Combined-expression weighted-sum (`minimize 0.7*mass - 0.3*stiffness`) | parses (verified §5) | one folded term; solver minimises it; result differs from equal-weight in the documented direction |
| B5 | Lexicographic via IR (constructed in test, no source syntax yet) | `combination = Lexicographic`, two priorities | staged solve: rank-1 optimum within ε-band of standalone rank-1 solve; rank-2 optimised subject to it |
| B6 | Centrality default (continuous) | auto param, two bounding inequalities, no objective | solved value sits at the analytic Chebyshev centre within solver tolerance (a concrete two-sided-bound fixture) |
| B7 | Centrality NOT applied to discrete scope | auto Int param, enum constraint, no objective | CP-SAT returns first feasible; `ObjectiveProvenance` shows no synthetic-centrality term |
| B8 | Purpose carrying a 2-term objective set activates | a purpose with two `minimize` terms | `active_objective_map` holds the widened set; entity-ref remap applied to every term (I-purposes coupling) |
| B9 | Legibility surface | any solved auto cell | `reify explain` prints the governing objective, combination, and synthetic-vs-explicit flag per cell |
| B10 | Proposing underdetermined | auto param with no pinning constraint | `W_UNDERDETERMINED` lists the free param + (empty) touching-constraint set; `reify check` surfaces it |
| B11 | Coupling detection | leaf scope solved, parent constraint reads the leaf's solved cell after freeze | `W_SCOPE_COUPLING` names both scopes + the crossing cell |

---

## §9 — Decomposition plan

Greek labels; actual IDs assigned at decompose time. Phase order = dependency order.

**Phase 1 — foundation (the widened type).**
- **α — Widen `OptimizationObjective` → `ObjectiveSet` in `reify-ir`.** Modules: `reify-ir`. Add `ObjectiveTerm`/`ObjectiveSense`/`ObjectiveCombination`/`ObjectiveSet` per §6.1; keep a single-term constructor. *Intermediate* (unlocks β, γ, …). Signal/consumer: unlocks compiler-lowering β and solver-fold δ; the type compiles and the single-term constructor produces an `ObjectiveSet` matching old behaviour in a round-trip unit. `grammar_confirmed: true`.
- **β — Lower scope objectives to `ObjectiveSet` in compiler.** Modules: `reify-compiler` (`entity.rs`, `traits.rs`, `types.rs`). Collect *all* `minimize`/`maximize` decls per scope into one set (replaces the last-wins `objective = Some(...)`). *Intermediate* — unlocks the conflict diagnostic ζ and the integration gate. Signal: a two-decl scope compiles to a 2-term set (asserted via compiler test surfaced through the phase-2 gate). `grammar_confirmed: true`.

**Phase 2 — integration gate (vertical slice, leaf).**
- **γ — End-to-end single+multi objective through the solver.** Modules: `reify-eval`, `reify-constraints`, `reify-ir`. Thread `ObjectiveSet` through `ResolutionProblem`, `build_solver_problem`, `SolverRegistry::solve`. **Leaf — names the §8 boundary-test sketch (B1, B2, B4) as its signal.** Signal: `reify eval` on a fixture with `minimize 0.7*mass - 0.3*stiffness` resolves the auto param to the weighted optimum (CLI output difference); B1 single-objective fixture output is byte-identical to a recorded baseline. Depends on α, β. `grammar_confirmed: true`.

**Phase 3 — multi-objective semantics + conflict.**
- **δ — Weighted-sum fold in the dimensional solver.** Modules: `reify-constraints` (`solver.rs`, `eval_objective`). Fold terms per I3. *Leaf.* Signal: `reify eval` on a 2-term same-sense fixture (`minimize mass` + `minimize cost`) resolves to the sum-minimising point; differs measurably from optimising either alone. Depends on γ.
- **ε — Lexicographic staged solve (IR-level; ε-band freeze).** Modules: `reify-constraints` (`registry.rs`). Sequence sub-solves by priority with ε-band freeze (§3.2). *Leaf.* Signal: integration test constructing an `ObjectiveSet { combination: Lexicographic }` in-IR (no source syntax — §5) asserts rank-1 optimum preserved within ε while rank-2 improves (B5). **G6: this leaf's signal is IR-constructed, not source-driven — explicitly acceptable because the source surface is deferred to a grammar follow-up (§5); the signal is still user-reachable via the `reify explain` provenance once a future grammar PRD adds syntax.** Depends on γ.
- **ζ — Conflicting-objectives diagnostic `E_OBJECTIVE_CONFLICT`.** Modules: `reify-compiler` (`entity.rs`), `reify-core` (diagnostic code). Emit per §3.3/§6.3. *Leaf.* Signal: `reify check` on a `minimize mass` + `maximize stiffness` fixture exits non-zero and prints `E_OBJECTIVE_CONFLICT` with the three-escape hint (CLI diagnostic — B3). Depends on β.

**Phase 4 — default centrality.**
- **η — Default centrality (Chebyshev-centre) objective, continuous path.** Modules: `reify-constraints` (`solver.rs`), `reify-eval` (synthesis decision). Synthesise the max-min-slack `ObjectiveSet` when a continuous scope has auto+constraints+no objective (§3.4). *Leaf.* Signal: `reify eval` on a two-sided-bound fixture (`x >= 2mm`, `x <= 8mm`, no objective) resolves `x` to ~5mm (the centre) within tolerance, not to a boundary — a CLI output difference vs. today's arbitrary feasible point (B6). G6: Chebyshev centre is analytically defined for the two-bound fixture (= midpoint), so the asserted value has a real basis, not a guess. Depends on γ. Behind the §DESIGN-FORK-1 default/pragma switch.

**Phase 5 — legibility.**
- **θ — `ObjectiveProvenance` record on `EvalResult`.** Modules: `reify-eval`, `reify-ir`. Produce per-cell provenance (governing set, combination, synthetic-centrality flag, per-term realised value). *Intermediate* — unlocks ι. Signal: unlocks `reify explain` leaf ι. Depends on γ, η.
- **ι — `reify explain <file>` CLI subcommand.** Modules: `reify-cli` (`main.rs`). Print the per-cell objective-provenance table. *Leaf.* Signal: `reify explain` on a multi-objective fixture prints, per auto cell, the governing objective + combination + synthetic-vs-explicit flag (CLI output — B9). Depends on θ.

**Phase 6 — proposing-mode + coupling.**
- **κ — Proposing-mode `W_UNDERDETERMINED` report.** Modules: `reify-eval`, `reify-core` (code), `reify-cli`. List free params + touching constraints on an underdetermined scope (§3.6). *Leaf.* Signal: `reify check` on an underdetermined fixture prints `W_UNDERDETERMINED` naming the free param (CLI diagnostic — B10). Depends on γ.
- **λ — Bottom-up coupling diagnostic `W_SCOPE_COUPLING`.** Modules: `reify-eval` (post-solve walk), `reify-core` (code). Static read-set analysis per §3.7. *Leaf.* Signal: `reify check` on a parent-reads-frozen-leaf fixture prints `W_SCOPE_COUPLING` naming both scopes + crossing cell (CLI diagnostic — B11). Depends on γ.

**Phase 7 — companion correction (purposes seam).**
- **μ — Update purpose objective-rewrite to iterate `ObjectiveSet.terms`.** Modules: `reify-eval` (`engine_purposes.rs`). The `activate_purpose` entity-ref remap loop must walk every term (§7). *Leaf.* Signal: `reify eval` on a fixture whose purpose carries a 2-term objective resolves correctly with both terms' entity refs remapped (B8 — CLI output difference; without the fix the second term's refs stay unremapped and resolution diverges). Depends on α (the type), γ (the thread-through).

DAG: α → β → γ; γ → {δ, ε, η, κ, λ}; β → ζ; {γ, η} → θ → ι; {α, γ} → μ.

---

## §10 — Out of scope

- **Weight/priority surface syntax** (`minimize ... weight N`, `... priority N`) — fails the grammar gate (§5); deferred to a follow-up grammar PRD. The IR/solver *support* lexicographic (ε variant); only the `.ri` surface is missing.
- **Pareto-front exploration** — spec §10.4 explicitly calls this "a tooling concern, not a language-level construct." Not in this PRD.
- **Centrality for discrete (CP-SAT) and geometric (SolveSpace) scopes** — those backends are feasibility solvers with no objective machinery (G6, §11); their default stays "first feasible." A real discrete-centrality default would need an objective-capable discrete backend — separate, large, and gated on a backend swap.
- **GUI / LSP rendering of `ObjectiveProvenance`** — the data structure ships and `reify explain` consumes it; GUI auto-resolve-panel and LSP hover wiring is a future surface PRD (declared consumer, §1).
- **Coupling *resolution*** (iterating to a fixed point across coupled scopes) — §10.6 only mandates *detection + diagnostic*; resolution is a much larger solver change, deferred.

---

## §11 — G6 premise-validity notes (verified against the actual backends)

The brief flagged G6 as load-bearing here. Findings from reading the solver crates:

- **Only the continuous `DimensionalSolver` (Nelder-Mead) optimises an objective.** `crates/reify-constraints/src/cpsat.rs` is a pure-Rust backtracking *feasibility* solver — it finds a satisfying assignment and ignores `ResolutionProblem.objective` entirely. `crates/reify-constraints/src/solvespace.rs` (geometric) likewise has no objective handling. **Consequence:** every objective-bearing promise in this PRD (weighted-sum, lexicographic, centrality) is scoped to the **continuous** path. The PRD does NOT promise discrete/geometric multi-objective — that would be a false premise against these backends. Phases δ/ε/η, B6/B7 all reflect this; B7 explicitly asserts the discrete path stays feasibility-only.
- **Weighted-sum is mechanically trivial** — a linear fold of already-evaluable `CompiledExpr`s into the existing penalty cost. No new numerical capability; G6-clean.
- **Lexicographic ε-band freeze** (not hard equality) is the §3.2 mitigation for Nelder-Mead's documented iteration-limit non-convergence (`SolveResult::Solved` doc comment). A hard-equality freeze would risk over-constraining a non-converged stage; ε-band is the defensible choice.
- **Centrality = Chebyshev centre (max-min normalised slack)** is a standard, well-defined formulation. For the B6 two-sided-bound fixture the centre is the analytic midpoint, so the asserted ~5mm value has a real basis (not a "tuned" guess — the §74 overlay hazard). The max-min is non-smooth, but Nelder-Mead is derivative-free and the existing penalty machinery already handles non-smoothness, so no solver-capability fiction.
- **No numeric accuracy threshold is asserted with a guessed bound.** The only numeric postconditions (B6 centre, B5 ε-band) are tolerance-relative to an analytically known or independently-solved reference — G6 branch-1 satisfied by construction.

---

## §12 — Open questions (tactical, deferred to impl)

1. **ε-band constant for lexicographic freeze.** Suggested: reuse the solver's existing convergence tolerance scaled by a small factor. Decide during ε.
2. **Slack normalisation `scaleⱼ` for centrality.** Per-constraint scale (e.g. by the constraint's own magnitude) vs. global. Suggested: per-constraint by the RHS magnitude. Decide during η.
3. **`reify explain` output format** (table vs. JSON `--format`). Suggested: human table by default, mirror `reify doc`'s `--format json` for tooling. Decide during ι.
4. **`W_SCOPE_COUPLING` granularity** — one warning per coupled pair vs. one aggregate. Suggested: per crossing cell. Decide during λ.

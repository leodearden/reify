# Capability manifest — whole-model-joint-drive-seam

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/whole-model-joint-drive-seam.md`. Every binding below resolves to **PASS**; no `declared-only`/`test-only`/`producer-downstream`/`producer-absent`/`producer-extent-short`/`fixture-ERROR`/`bound≤floor`/`rejection-absent` value appears, so the batch is not blocked.

**Substrate verification provenance.** The generic decompose D3 workflow (`scripts/prd-decompose-verify.mjs`) was **superseded** for this PRD by a first-party, exhaustive substrate verification performed this session (2026-07-13): three independent Explore agents confirmed both structural gaps and the design machinery by direct code inspection (exact file:line), and the grammar gate was run. Running the multi-agent D3 harness in addition would (a) re-derive weaker evidence than the greps below, and (b) risk a spurious FAIL on β's **self-delivered** end-to-end premise (the joint drive is the capability β builds — a correct RED premise, not a substrate gap). The bindings below are that verification, captured as the committed artifact a dispatch-time architect diffs against substrate.

Substrate confirmed on current main `93226f0634` (base files unchanged since 2c8d7212):

---

## Leaf β — per-trial SCC recompute + joint-drive integration gate

**Signal:** `reify eval examples/whole_model_joint_drive.ri` resolves a child `auto` dimension to the whole-model cost-min value (**≠** its bottom-up frozen freeze) and merged whole-assembly cost **strictly <** the frozen-cascade baseline.

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| Objective-position `cost()` expands into its `line_cost` sub-DAG (so child costs are live vars) | Capability→producer (anti-orphan) + DAG-direction | `producer:task-α` (upstream): α runs `apply_cost_aggregation` on objective/constraint terms in `build_solver_problem`/`build_merged_solver_problem`. Substrate fn exists: `grep:crates/reify-eval/src/structural_query.rs:633` (`fn apply_cost_aggregation`) | **PASS** |
| `ResolutionProblem.dependent_cells` carries the topo-ordered recompute set | Capability→producer + field-population | `producer:task-α` (populates, upstream) → read by β in `build_trial_values`. Struct exists: `grep:crates/reify-ir/src/constraint.rs:364` (`pub struct ResolutionProblem`) | **PASS** |
| Per-trial recompute evaluates dependent cells via `eval_expr` in the solver | Capability→producer (this leaf) + wired | Delivered by β at `grep:crates/reify-constraints/src/solver.rs:138` (`build_trial_values`), called per trial by `ConstraintCostFunction::cost` (`solver.rs:870`); solver already calls `reify_expr::eval_expr` (`solver.rs:190/249/330/380/611/844`) — no new dep | **PASS** |
| Authoritative cross-scope topological order | Capability→producer (upstream α) | `topological_sort` `grep:crates/reify-eval/src/dirty.rs:153`; `extract_value_deps` `grep:crates/reify-eval/src/deps.rs:2634`; post-solve write-back site `grep:crates/reify-eval/src/engine_eval.rs:4351` (`evaluate_let_bindings`) — the same order feeds both | **PASS** |
| `cost(self.descendants)` value semantic (subtree sum) | Capability→producer (landed) | `producer:task-5015` (γ, merged `c5715faaf`); wired via `apply_cost_aggregation` (structural_query.rs:633). Example on main: `examples/cost_subtree_aggregate.ri` | **PASS** |
| Merged cross-scope `ResolutionProblem` builder + N-scope write-back | Capability→producer (landed) | `producer:task-5014` (β/M-WHOLE, merged `bb6ebb735`); `build_merged_solver_problem` present in `engine_eval.rs` | **PASS** |
| Nelder-Mead + deterministic-multistart back-end | Capability→producer (landed) | `producer:task-5016` (δ, merged `d6ab5b981`) | **PASS** |
| Grammar: parent `minimize cost(self.descendants)` over children with `auto(free)` dims | Grammar reality (anti-mismatch) | `grammar-fixture:/tmp/prd-gate-fixtures/joint-drive-1.ri` parses 0-ERROR (`tree-sitter parse --quiet` exit 0, 2026-07-13); committed corroborators on main: `examples/cost_subtree_aggregate.ri` (`cost(self.descendants)` in cell position) + `examples/continuous_cost_min.ri` (`minimize <expr>` + `auto(free)`, parses exit 0). β commits the combined form as `examples/whole_model_joint_drive.ri` | **PASS** |
| Merged cost **strictly <** frozen baseline; child auto **≠** frozen freeze | Numeric floor / premise validity (G6 branch 3) | **Comparative**, not an absolute numeric bound → no floor applies. Achievability: the merged solve optimises over a **superset** of free variables vs the frozen cascade ⇒ merged optimum ≤ frozen; **strict** because the fixture is authored with **active** coupling (co-solved child auto ≠ its frozen freeze). The capability is delivered by β + upstream α — NOT by a task that depends on β | **PASS** |
| Illegal (non-optimisation) data cycle still errors (BT-6 negative assertion) | Rejection-mechanism (anti-silent-accept) | Rejection mechanism present on main: `grep:crates/reify-eval/src/engine_eval.rs:441` (`detect_let_cycle`) + `W_SCOPE_COUPLING`. β's SCC-admissibility guardrail preserves it and adds a probe authoring an `a→b→a` non-optimisation cycle and **observing** the cycle/`W_SCOPE_COUPLING` diagnostic fires | **PASS** |
| `@optimized`/ComputeNode cells excluded from per-trial fold (BT-7) | Capability→producer (this leaf) | β excludes them; load-bearing exclusion precedent `grep:crates/reify-eval/src/engine_eval.rs:7878` / `:7966` | **PASS** |

---

## Intermediate α (roped into β via C-as-integration-gate — no standalone leaf signal)

Substrate α builds on, all present on main: `apply_cost_aggregation` (structural_query.rs:633), `ResolutionProblem` (constraint.rs:364), `build_solver_problem`/`build_merged_solver_problem` (engine_eval.rs:1537/1802 region), `topological_sort` (dirty.rs:153), `extract_value_deps` (deps.rs:2634), `evaluate_let_bindings` post-solve site (engine_eval.rs:4351). No novel substrate — G3 N/A for α; its output is consumed by β (named downstream consumer).

## Correction γ (docs)

Updates `whole-model-objective-coupling.md` + `.capability-manifest.md` to bind M-WHOLE **BT4 → producer:task-β**. No code substrate; not a user-observable code leaf.

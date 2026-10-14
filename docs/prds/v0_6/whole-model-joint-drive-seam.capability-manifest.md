# Capability manifest — whole-model-joint-drive-seam

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/whole-model-joint-drive-seam.md`. Every binding below resolves to **PASS**; no `declared-only`/`test-only`/`producer-downstream`/`producer-absent`/`producer-extent-short`/`fixture-ERROR`/`bound≤floor`/`rejection-absent` value appears, so the batch is not blocked.

**Substrate verification provenance.** The generic decompose D3 workflow (`scripts/prd-decompose-verify.mjs`) was **superseded** for this PRD by a first-party, exhaustive substrate verification performed this session (2026-07-13): three independent Explore agents confirmed both structural gaps and the design machinery by direct code inspection (exact file:line), and the grammar gate was run. Running the multi-agent D3 harness in addition would (a) re-derive weaker evidence than the greps below, and (b) risk a spurious FAIL on β's **self-delivered** end-to-end premise (the joint drive is the capability β builds — a correct RED premise, not a substrate gap). The bindings below are that verification, captured as the committed artifact a dispatch-time architect diffs against substrate.

Substrate originally confirmed (2026-07-13) on main `93226f0634` (base files unchanged since 2c8d7212).

**Refreshed 2026-07-28 (task 5190) against main `bd10b6d0e1e2dd32b09065f2304bd029f04b6506`.**
The 2026-07-13 manifest above **predates Amendment A1** (`cba46b8794`, 2026-07-22, esc-5189-2),
so as authored it carried **no** coverage of Gap C, design decision 7, δ = task **5334**, or
boundary tests **BT-8 / BT-9 / BT-10** — the entire cluster-formation half of the seam. This
refresh (a) adds the missing δ = 5334 section below — δ is an **intermediate**, not a leaf: it
carries no standalone user-observable signal and is roped into β, exactly as α is — and
(b) re-anchors every `grep:<file>:<line>`
citation that had drifted since 2026-07-13. Both provenance points are kept rather than
overwritten, so the refresh reads as an amendment: the 2026-07-13 line records what the
first-party three-Explore-agent verification actually asserted, and this line records what a
reader can re-run today. Drifted anchors re-derived in this refresh:
`build_trial_values` solver.rs:138 → **:169**; `ConstraintCostFunction::cost` solver.rs:870 →
**:1011**; `evaluate_let_bindings` engine_eval.rs:4351 → **:9593**; `detect_let_cycle`
engine_eval.rs:441 → **:443**; `build_solver_problem`/`build_merged_solver_problem`
engine_eval.rs:1537/1802 → **:2024/:2254**; the `@optimized` exclusion precedent
engine_eval.rs:7878/:7966 → **:1731**.

---

## Leaf β — per-trial SCC recompute + joint-drive integration gate

**Signal:** `reify eval examples/whole_model_joint_drive.ri` resolves a child `auto` dimension to the whole-model cost-min value (**≠** its bottom-up frozen freeze) and merged whole-assembly cost **strictly <** the frozen-cascade baseline.

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| Objective-position `cost()` expands into its `line_cost` sub-DAG (so child costs are live vars) | Capability→producer (anti-orphan) + DAG-direction | `producer:task-5188` (α, upstream, landed): runs `apply_cost_aggregation` on objective/constraint terms in `build_solver_problem` (`grep:crates/reify-eval/src/engine_eval.rs:2024`) / `build_merged_solver_problem` (`grep:crates/reify-eval/src/engine_eval.rs:2254`). Substrate fn exists: `grep:crates/reify-eval/src/structural_query.rs:633` (`fn apply_cost_aggregation`) | **PASS** |
| `ResolutionProblem.dependent_cells` carries the topo-ordered recompute set | Capability→producer + field-population | `producer:task-5188` (α, populates, upstream) → read by β in `build_trial_values`. Struct exists: `grep:crates/reify-ir/src/constraint.rs:364` (`pub struct ResolutionProblem`); α's concrete builder: `grep:crates/reify-eval/src/engine_eval.rs:1611` (`fn build_dependent_cells`) | **PASS** |
| Per-trial recompute evaluates dependent cells via `eval_expr` in the solver | Capability→producer (this leaf) + wired | Delivered by `producer:task-5189` (β) at `grep:crates/reify-constraints/src/solver.rs:169` (`fn build_trial_values`), called per trial from `ConstraintCostFunction::cost` (`grep:crates/reify-constraints/src/solver.rs:1011`, whose `build_trial_values` call is at `solver.rs:1022`); solver already calls `reify_expr::eval_expr` — no new dep | **PASS** |
| Authoritative cross-scope topological order | Capability→producer (upstream α = 5188) | `topological_sort` `grep:crates/reify-eval/src/dirty.rs:153`; `extract_value_deps` `grep:crates/reify-eval/src/deps.rs:2634`; post-solve write-back site `grep:crates/reify-eval/src/engine_eval.rs:9593` (`fn evaluate_let_bindings`) — the same order feeds both | **PASS** |
| `cost(self.descendants)` value semantic (subtree sum) | Capability→producer (landed) | `producer:task-5015` (γ, merged `c5715faaf`); wired via `apply_cost_aggregation` (structural_query.rs:633). Example on main: `examples/cost_subtree_aggregate.ri` | **PASS** |
| Merged cross-scope `ResolutionProblem` builder + N-scope write-back | Capability→producer (landed) | `producer:task-5014` (β/M-WHOLE, merged `bb6ebb735`); `build_merged_solver_problem` present in `engine_eval.rs` | **PASS** |
| Nelder-Mead + deterministic-multistart back-end | Capability→producer (landed) | `producer:task-5016` (δ, merged `d6ab5b981`) | **PASS** |
| Grammar: parent `minimize cost(self.descendants)` over children with `auto(free)` dims | Grammar reality (anti-mismatch) | `grammar-fixture:examples/whole_model_joint_drive.ri` — the combined form, now **committed** on main (landed by β at `e1bab107ab`; tracked, so the binding is re-runnable by any later reader). Supersedes the 2026-07-13 citation of an ephemeral scratch fixture under the run-local `prd-gate-fixtures` directory, which was never durable evidence. Committed corroborators on main: `examples/cost_subtree_aggregate.ri` (`cost(self.descendants)` in cell position) + `examples/continuous_cost_min.ri` (`minimize <expr>` + `auto(free)`) | **PASS** |
| Merged cost **strictly <** frozen baseline; child auto **≠** frozen freeze | Numeric floor / premise validity (G6 branch 3) | **Comparative**, not an absolute numeric bound → no floor applies. Achievability: the merged solve optimises over a **superset** of free variables vs the frozen cascade ⇒ merged optimum ≤ frozen; **strict** because the fixture is authored with **active** coupling (co-solved child auto ≠ its frozen freeze). The capability is delivered by `producer:task-5189` (β) + upstream `producer:task-5188` (α) + `producer:task-5334` (δ) — NOT by a task that depends on β | **PASS** |
| Illegal (non-optimisation) data cycle still errors (BT-6 negative assertion) | Rejection-mechanism (anti-silent-accept) | Rejection mechanism present on main: `grep:crates/reify-eval/src/engine_eval.rs:443` (`fn detect_let_cycle`) + `W_SCOPE_COUPLING`. β's SCC-admissibility guardrail preserves it and adds a probe authoring an `a→b→a` non-optimisation cycle and **observing** the cycle/`W_SCOPE_COUPLING` diagnostic fires | **PASS** |
| `@optimized`/ComputeNode cells excluded from per-trial fold (BT-7) | Capability→producer (this leaf) | `producer:task-5189` (β) excludes them; load-bearing exclusion precedent `grep:crates/reify-eval/src/engine_eval.rs:1731` (the `is_optimized_userfn_cell` guard), whose rationale — including why ComputeNode-produced cells without a foldable `default_expr` never enter — is documented at `engine_eval.rs:1536-1544`. Predicate itself: `grep:crates/reify-eval/src/engine_eval.rs:1507` (`fn is_optimized_userfn_cell`) | **PASS** |

---

## Intermediate α = task 5188 (roped into β via C-as-integration-gate — no standalone leaf signal)

Substrate α builds on, all present on main and re-anchored 2026-07-28: `apply_cost_aggregation` (`structural_query.rs:633`), `ResolutionProblem` (`constraint.rs:364`), `build_solver_problem` (`engine_eval.rs:2024`) / `build_merged_solver_problem` (`engine_eval.rs:2254`), `topological_sort` (`dirty.rs:153`), `extract_value_deps` (`deps.rs:2634`), `evaluate_let_bindings` post-solve site (`engine_eval.rs:9593`). No novel substrate — G3 N/A for α; its output is consumed by β = task 5189 (named downstream consumer). α is **landed**; its own novel artefact is `build_dependent_cells` (`engine_eval.rs:1611`), which populates `ResolutionProblem.dependent_cells`.

## Intermediate δ = task 5334 — Gap C: cluster formation for derived-cost coupling (Amendment A1)

**Signal:** intermediate — no standalone user-observable signal. δ makes `compute_clusters` actually yield the one `MergedSolve` cluster that β's BT-5 leaf **presupposes**: without Gap C's seed no cluster forms at all, and BT-5 is structurally unreachable (PRD §7). Its output is consumed by β = task 5189 (named downstream consumer), so it is not an orphan.

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| **Design decision 7 / C1** — objective reads feeding cluster formation reflect the **expanded** objective | Capability→producer (this leaf) + wired | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:121` (`pub(crate) struct ClusterFormationCtx`), threaded as `Option<&ClusterFormationCtx>` into `compute_clusters`. Its own doc comment self-cites the amendment (`resolve_order.rs:105-106`: "JOINT-DRIVE δ, Gap C, task #5334, PRD … §13 Amendment A1"). Callers passing `None` keep the pre-#5334 direct-auto seed **byte-for-byte** — the executable INV-2 fence (`resolve_order.rs:1032`) | **PASS** |
| **Design decision 7 / C2** — transitive auto-reaching union seed | Capability→producer (this leaf) + wired | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:742` (`fn union_via_transitive_auto_owners`), invoked from `compute_clusters` (`grep:crates/reify-eval/src/resolve_order.rs:996`) at the "Transitive auto-reaching seed (JOINT-DRIVE δ, task #5334, C2)" branch (`resolve_order.rs:1045`). Walks expanded objective reads through derived `Let` cells via the `extract_value_deps` closure (`deps.rs:2634`) down to auto owners | **PASS** |
| **`@optimized` stop condition** — design decision 5 preserved, not silently widened | Capability→producer (this leaf) + wired | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:808` calls `crate::engine_eval::is_optimized_userfn_cell` (`grep:crates/reify-eval/src/engine_eval.rs:1507`). The walk **stops** at an `@optimized` cell rather than unioning through it | **PASS** |
| **Instance-path ↔ template-keyed id bridging** | Capability→producer (this leaf) + wired | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:952` (`pub(crate) fn normalize_cell_id`), applied at every walk hop; instance-path fallback rationale at `resolve_order.rs:759`, bounds argument at `:838`. Unbridgeable ids simply do not cluster — the pre-#5334 behaviour (`resolve_order.rs:857`) | **PASS** |
| **Cold + warm call-site parity** — a warm re-eval forms the same cluster set | Capability→producer (this leaf) + wired | `producer:task-5334`: COLD `grep:crates/reify-eval/src/engine_eval.rs:4853` (`ClusterFormationCtx { … }`) → `:4863` `resolve_order(&module.templates, Some(&cluster_ctx))`; WARM `grep:crates/reify-eval/src/engine_eval.rs:7496` → `:7503` `resolve_order_ordering_and_clusters(…, Some(&cluster_ctx))`. Both construct the ctx, so neither path silently keeps the `None` seed | **PASS** |
| **BT-8 (positive)** — cluster forms for derived-cost coupling | Capability→producer (this leaf) + wired | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:1804` (`bt8_pre_expanded_derived_cost_forms_single_merged_cluster`) + `grep:crates/reify-eval/src/resolve_order.rs:1888` (`bt8_cost_self_descendants_forms_single_merged_cluster`); integration-level cold-path fence `grep:crates/reify-eval/tests/harness_engine/joint_drive_cluster_formation.rs:131` (`derived_cost_coupling_forms_cluster_on_cold_path`). Gap C banner: `resolve_order.rs:1785` | **PASS** |
| **BT-9 (negative)** — `@optimized` stops the walk, **no** cluster forms | Rejection-mechanism (anti-silent-accept) | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:1986` (`bt9_optimized_line_cost_forms_no_cluster`). Rejection is **OBSERVED to fire** — the test asserts the absence of a cluster on a fixture that is identical to BT-8's except the child `line_cost` is `@optimized`, so a silently-widened walk fails it | **PASS** |
| **BT-10 (negative)** — constraint reads still never union (`scope_coupling` A–G / resolve_order INV-2) | Rejection-mechanism (anti-silent-accept) | `producer:task-5334`: `grep:crates/reify-eval/src/resolve_order.rs:2067` (`bt10_constraint_only_transitive_read_forms_no_cluster`); invariant-fence banner `resolve_order.rs:2033`. The `compute_clusters` seed comment re-states that CONSTRAINT-position transitive reads are deliberately **not** unioned. Rejection **OBSERVED to fire** | **PASS** |
| **Dim cap + over-cap disposition unchanged by δ** | Landed substrate (untouched by this leaf) | `grep:crates/reify-eval/src/resolve_order.rs:72` (`WHOLE_MODEL_CLUSTER_DIM_CAP`), `grep:crates/reify-eval/src/resolve_order.rs:85` (`ClusterDisposition::ApproximatedFallback`), applied at `resolve_order.rs:1125-1126`. δ changes only *which cells seed a cluster*, never the cap or the degrade path — so M-WHOLE α's `W_COUPLING_APPROXIMATED` contract is preserved | **PASS** |

**G6.** δ asserts **no** absolute numeric bound and **no** field-population claim: every assertion above is a *set-equality* on the formed cluster set (BT-8: exactly one `MergedSolve` cluster `{parent, child}`; BT-9/BT-10: the empty cluster set). So no numeric-floor branch fires and there is no guessed threshold to defend. The two negative rows are rejection-mechanism bindings whose rejection is observed, not merely present.

**DAG-direction.** β = 5189 `depends_on` 5334 — a real edge — and 5334 is landed (`db64320f66`, ancestor of main `bd10b6d0e1e2dd32b09065f2304bd029f04b6506`). δ is therefore **upstream** of every leaf that observes it; no `producer-downstream` binding appears in this section.

## Correction γ (task 5190, docs)

Two-part companion reconciliation. No code substrate; **not** a user-observable code leaf.

1. **Binds M-WHOLE BT4 → `producer:task-5189`.** `whole-model-objective-coupling.md` §6 (BT3/BT4 rows) + §8 (new cross-PRD row for this PRD) + §9 (ε bullet), and `whole-model-objective-coupling.capability-manifest.md` §ε (BT3/BT4 rows, the G6-branch-3 paragraph, header, Summary). BT4 was the only novel-capability row in that manifest justified by an **achievability argument alone**, with no `producer:task-…` binding — the gap §2 of this PRD flags. The binding itself — task ids, landed SHAs, DAG direction, and why it is written as a numeric id rather than a bare Greek label — is stated **once**, in that manifest's header + BT4 row, and is deliberately **not** restated here.
2. **Refreshes THIS manifest post-Amendment-A1.** Adds the **Intermediate δ = task 5334** section above (Gap C / design decision 7 / BT-8/-9/-10), which the 2026-07-13 authoring predates, and re-anchors this document's `grep:` evidence against main `bd10b6d0e1e2dd32b09065f2304bd029f04b6506` (see the header provenance for the drift list).

**Boundary-test numbering — disambiguation (resolved by task #5764).** This manifest binds **the PRD §7 table's** numbering, in which **BT-10 = "Constraint reads still never union"** (δ; `crates/reify-eval/src/resolve_order.rs:2067`), unchanged. Task 5189 had independently numbered two further **β-local** boundary tests that collided with §7's labels:

- a β-local **BT-10** — `cost_robustness_tradeoff` as a THIRD, unconverted scoring site (`crates/reify-constraints/tests/joint_drive_per_trial_recompute.rs:264`); and
- a **BT-11** — instance-path alias rescoping (`crates/reify-eval/tests/joint_drive_expansion_boundary.rs`), recorded in PRD §12 Q4 prose but never entered into the §7 table.

Follow-up task **#5764** (filed from ticket `tkt_0RRT9Q9R4JJ6F106CNHHEW49K7`) picked option (a): the two β-local tests are **renumbered** to **BT-11** (tradeoff scoring) and **BT-12** (per-sub overrides) — comment banners in both test files updated, test fn names unchanged — and both now have §7 table rows alongside δ's unchanged BT-10. `docs/prds/v0_6/whole-model-joint-drive-seam.md` §7/§12 and this manifest were the surfaces touched; `docs/prds/v0_6/whole-model-joint-drive-seam.md` was **not** edited by task 5190 itself.

---

## Summary

| Leaf | Signal | Asserted-capability verdicts | Blocks batch? |
|---|---|---|---|
| α (5188) | intermediate — no standalone signal; roped into β | landed substrate + `build_dependent_cells`; G3 N/A | no |
| δ (5334) | intermediate — cluster formation Gap C (Amendment A1); roped into β | 9 PASS | no |
| β (5189) | e2e leaf **BT-5**: `reify eval examples/whole_model_joint_drive.ri` — child `auto` ≠ frozen freeze, merged cost **strictly <** frozen baseline | 11 PASS | no |
| γ (5190) | docs correction — no code substrate, not a user-observable code leaf | N/A (bookkeeping) | no |

**No FAIL bindings.** No `declared-only` · `test-only` · `producer-absent` · `producer-extent-short` · `producer-downstream` · `fixture-ERROR` · `bound≤floor` · `rejection-absent` value appears anywhere above, so the batch is not blocked. Every `producer:task-…` binding is upstream of the leaf that observes it (α = 5188 and δ = 5334 are both real `depends_on` edges of β = 5189; all three are landed and are ancestors of the refresh SHA).

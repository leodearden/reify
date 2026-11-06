# PRD — auto-reach relation (reify-eval): one answer to "which constraint or objective reaches which auto"

**Status:** authored 2026-10-18 (gap-fill stream `auto-reach-relation`, record `plans/gap-fill-deliberation-reify-20261016.md` §K3-D2, ruling B). Decomposed 2026-10-18: α #9368, β1 #9369, β2 #9370, β3 #9371, γ #9372, δ #9374 (decision milestone), ω #9373 (close); #6307 and #7069 absorbed into β1, #6649 into β2 (cancelled with `x_subsumed_by`); gate #9171 ← #9369, #9370, #9371, #9372. v0_6. Approach **B + H** (§5 contract, §6 boundary-test sketch): the stream changes what the ConstraintSolver seam (§3.5 of `engine-integration-norm.md`) admits and has four consumers in three PRDs' territory.
**Invariant established:** **INV-EVAL-7** (new `docs/invariants.md` row, owner this stream; clauses AR-1..AR-4 in §5.7). Every leaf cites it and carries its enforcement in its done criteria (INV-META-1).
**Finding keys closed:** fk-a5a65ab8428c (high; hotspot-survey-reify-20261015, eval-cell-commit.1) and fk-a5237570b374 (medium; eval-cell-commit.3). Gate: #9171 for the high leaves; never #8612.
**Code anchors** verified against main `baff3ed38b` (2026-10-17). `git diff 04be9302ea..main` over `crates/reify-eval/src/{engine_eval,resolve_order,engine_fixpoint,engine_edit,deps,detectors}.rs` is empty, so the record's anchors (taken at `0e60fa138d`, re-verified at `04be9302ea`) still hold. Main moves fast — cite-by-symbol; re-locate lines at implementation time. Line numbers appear only in the dated §10 record.

---

## 1. Consumer + user-observable surface (G1, G2)

**Consumers.**
- **Every `reify check` / `reify eval` / LSP diagnostics run.** Which constraints enter a scope's `ResolutionProblem`, whether `W_UNDERDETERMINED` fires for an auto, whether `E_EVAL_UNRESOLVED` declines a constraint, and the verdict a declined constraint gets.
- **`reify build` / tessellate.** The post-geometry constraint re-check (`Engine::check_constraints_post_geometry`) skips the declined set that `engine_fixpoint::constraints_reaching_auto` computes today.
- **GUI parameter previews** (`EngineSession::preview_parameter` → `Engine::edit_check` → `Engine::edit_param`). After solver-driver-parity β #6690 their problem is built by the one builder; this stream gives that builder its admission query.
- **In-engine consumers of the relation** (the four the record names, plus two projections of the same relation, §4 D9): the single problem builder's admission (#6690), `detect_underdetermined` (a `PostPassDetector` in `detectors.rs` after S4 κ #8868), `resolve_order::compute_clusters`, the constraint-side decline gate (`engine_fixpoint::constraints_reaching_auto`); `build_dependent_cells` and `objective_auto_reach`.

**In-engine seam catalogue (overlay G1 sub-check).** No new seam. The relation feeds §3.5 ConstraintSolver (problem admission) and the existing post-pass registry; it changes *which* constraints the seam receives, not the seam.

**What users see** (measured with the debug binary built from main on 2026-10-16; fixtures committed under `tests/prd-gate/fixtures/auto_reach_*.ri`; outputs in §10):
1. **Cross-sub geometry-backed constraint** (`auto_reach_geo_loop_b.ri`, #6649): today `reify check` prints `VIOLATED GeoLoopB#constraint[0]`, exit 1, and no geometry-backed refusal — a false Violated on undef operands (INV-SF-4 breach). After: `error: unresolved constraint: constraint GeoLoopB#constraint[0] transitively depends on auto parameter(s) through geometry-backed inputs`, no `VIOLATED` (the declined constraint is reported unresolved with its cause, never Violated — #6649's Indeterminate-with-cause pin, INV-SF-4), and `W_UNDERDETERMINED` for `GeoLoopB.leaf.w` naming the blocked constraint.
2. **Own-scope geometry-backed constraint** (`auto_reach_geo_loop_a.ri`): today `reify eval` runs a doomed solve and prints `error: constraints could not be satisfied (max absolute residual: 1.00e0)` and `note: GeoLoopA.w is undef (because: solve failed: infeasible)` next to the refusal. After: both lines are gone; the undef cause names the geometry block; `W_UNDERDETERMINED` names the blocked constraint.
3. **Connector autos** (`auto_reach_conn_pinned.ri`, `auto_reach_conn_concrete.ri`, #6307/#7069): today `W_UNDERDETERMINED: auto parameter 'E.__connector_0.gain' … not touched by any constraint` fires while the same run prints `E.__connector_0.gain = 0.01 m` — warned but solved. After: no warning. `auto_reach_conn_free.ri` keeps both of its (correct) warnings.
4. **Constraint in a parent on a template-keyed child auto** (`auto_reach_cross_scope_child_auto.ri`, `auto_reach_tk_child_let.ri`): today nothing solves `Leaf.k`, no `W_UNDERDETERMINED` fires, and eval says `Leaf.k is undef (because: Leaf.k awaiting solve)` — a false cause. After β1: `W_UNDERDETERMINED` for `Leaf.k` naming `P#constraint[0]` as not driving it (cross-scope); the constraint's own verdict is unchanged (it is not declined). Making such a constraint drive the auto is R3's "later via whole-model-objective-coupling", tracked by the δ decision milestone.
5. **Message honesty** (`auto_reach_two_siblings.ri`, `auto_reach_constraint_inside_child.ri`): verdicts unchanged; `(touching constraints: none)` — false today for `Sib.b1.bore` and `Own.b.bore` — becomes the list of touching constraints and why each cannot drive the auto.
6. **Edit path** (GUI previews, with #6690): a constraint that reaches an auto through a `let`, or through an instance-path spelling, is admitted on `edit_param` as it is on cold eval (today the edit filter tests direct raw reads only).
7. **`@optimized` through a let** (`let c = optfn(a); constraint c < X`): today admitted with `c` frozen during the solve (joint-drive-seam decision 5), a doomed solve. After: declined with a block message naming `c`; β1 first proves the shape with a fixture (§11 OQ-1).

**Leaf signals (G2, per leaf in §9).** User-visible leaves name a `reify check` / `reify eval` difference on a committed fixture. α and β3 are behaviour-preserving intermediates whose consumer is named (β1, γ); γ is the integration gate (C-as-integration-gate) over all eighteen fixtures, including the edit column.

## 2. Problem (re-verified at `baff3ed38b`)

1. **Six derivations of one relation, six edge sets** (heuristic 11 SPOT; fk-a5a65ab8428c). "Constraint or objective term C transitively reads auto A" is computed by:
   - **W1** `filter_constraints_reading_autos` over `CellReadIndex::cells_reaching` (engine_eval.rs) — constraints of the owning scope only; follows value-cell `default_expr` reads, through `@optimized` cells; no realization edges; additive raw + normalised spelling.
   - **W2** `detect_underdetermined` + `auto_is_pinned_through_a_reader` — global constraints and objective terms; forward `read_closure`, then a reverse probe gated by the stage-(g) single-instance alias oracle and a declaring-template guard.
   - **W3** `resolve_order::union_via_transitive_auto_owners` over its own `build_non_auto_cell_map` (a second copy of `CellReadIndex`'s cell map) — objective terms only; STOPS at a bare `@optimized` call cell; as-is spelling first, normalised fallback.
   - **W4** `engine_fixpoint::constraints_reaching_auto` / `realizations_reaching_auto` — the graph trace map; realization→realization edges only; raw ids; documented "does NOT chase value-cell→value-cell read chains"; a constraint whose geometry argument is a `CrossSubGeometryRef` gets empty `realization_reads` (the #6649 escape).
   - **W5 / W6** `Engine::edit_param` / `Engine::edit_source` resolution sub-passes — direct raw reads only (`trace.reads.iter().any(|r| auto_ids.contains(r))`), groups keyed by raw entity string. (W6 dies with `edit_source`, S4 δ #8862; W5's builder is unified by #6690.)
   Eight further reach-like walks exist (§4 D9 places each). The census (§10) tabulates nine shapes × six walks; the walks disagree on six of the nine.
2. **The disagreements are live defects, not style.** The #5467 history is four fixes and three escalations of exactly W1 vs W2 (02913857af "a loud, correct warning had become a silent wrong answer"; f603a9e717 "Layers 1 and 4 disagreed"; d3b5c320da; 7be73738b1). Today, on main: the #6649 false Violated (W1 admits, W4 misses); the doomed geometry solve (W1 admits what W4 declines); the connector warned-but-solved (the builder's connector pin vs W2); the cross-scope auto that nothing solves and nothing reports (W2's global closure clears it, W1 admits nothing for it); and edit-path admission that differs from cold (W5 vs W1) in both directions (heuristics 10, 11).
3. **`CellReadIndex` keeps its two directions in sync by hand** (fk-a5237570b374; heuristics 10, 12). Forward (`read_closure`, a lazy walk re-running `extract_value_deps` per visit) and reverse (`cells_reaching`'s `OnceCell` map from a separate sweep) each restate the raw-plus-`normalize_cell_id` bridge — `read_closure`'s `push`, the reverse keying, the reverse seeding — and two more restatements sit in `build_dependent_cells` and `auto_is_pinned_through_a_reader`. Its doc carries the consistency argument in prose ("Making only ONE side additive is worse than making neither"). `normalize_cell_id` is a meaningful-string parser over `strip_collection_indices`.
4. **Temporal coupling on the unfold budgets** (heuristic 7). The index is shared through a caller-owned `OnceCell<CellReadIndex>`; whichever of `build_solver_problem`, `build_merged_solver_problem` or `detect_underdetermined` reaches `get_or_init` first fixes the budgets; one `debug_assert_eq!` in `detect_underdetermined` records that "the losing caller's budgets are silently discarded".
5. **Geometry-backed reach is undrivable by design, but only one walk knows.** No production path re-realizes geometry per solver trial (`docs/prds/v0_6/geometry-algebra-solver-unification.md` names per-trial re-realization out of scope; `check_constraints_post_geometry` is a one-shot verification with a decline set). W1 nevertheless admits such constraints because a geometry `let` carries a `default_expr`.
6. **The "frozen compute cell" rule is spelled twice.** Joint-drive-seam decision 5 (`docs/prds/v0_6/whole-model-joint-drive-seam.md` D5: `@optimized` cells are excluded from the per-trial fold) is honoured by `build_dependent_cells` and by W3, through `is_optimized_userfn_cell`, which matches a *bare* `@optimized` call only; W1 and W2 ignore it.

## 3. Pre-conditions and substrate (G3, G6)

| Capability the leaves assume | State at `baff3ed38b` | Resolution |
|---|---|---|
| `deps::DependencyGraph` (one forward walk; `reverse()` its inversion) | absent | prerequisite: S3 δ #8691 (α ← #8691) |
| `reify_core::InstancePath` and `structure_scope()` | absent | prerequisite: S3 ι #8704 (α ← #8704; ruled R6) |
| `solver_problem.rs` holding the builders, `CellReadIndex`, `build_single_instance_alias_paths`, the pinned-connector writers | absent (all in `engine_eval.rs`) | prerequisite: S4 ν #8860 (α ← #8860) |
| `detect_underdetermined` as a `PostPassDetector` in `detectors.rs`; `ColdArtifacts` on `PostPassState` | `detectors.rs` holds only the PSD detector; `ColdArtifacts` absent | prerequisite: S4 κ #8868 (β1 ← #8868; κ itself follows S4 φ #9292, the `replay_last_eval` leaf) |
| one engine-side `ResolutionProblem` builder (cold and edit) | two builders + the edit block | prerequisite: solver-driver-parity β #6690 (β1 ← #6690) |
| `EvaluationState` (snapshot, reverse index, trace map) | exists (`lib.rs`) | consumed; gains two fields (§5.5) |
| `ClusterFormationCtx` | exists (`resolve_order.rs`) | consumed; gains one field (β3) |
| `UndefCause` (`reify-ir` `value.rs`), `DiagnosticCode::{Underdetermined, EvalUnresolved}` | exist | consumed; `UndefCause` gains one variant (β1) |
| `check_constraints_post_geometry` decline set | exists (`engine_constraints.rs`) | consumed (β2) |
| INV-ARCH-1 `module_scc_ratchet` | exists (S3 α #8673, done) | consumed: two new modules, no new SCC |
| `tests/infra/adoption-ratchet.manifest` + `adoption_ratchet.py` | exist | consumed: this stream's ratchet rows (§5.8) |

**Grammar (G3 vector 1).** No novel syntax. The eighteen `tests/prd-gate/fixtures/auto_reach_*.ri` fixtures parse with 0 ERROR nodes under the isolated-cache `tree-sitter parse --quiet` (negative control rejected), and `reify check` evaluates each (§10). `grammar_confirmed: true` on every leaf.

**G6.** No numeric bounds. End-to-end premises are traced to each leaf's own dependency set (β2's refusal needs β1's relation; γ's equality needs β1–β3). Rejection premises are backed by mechanisms that already fire on main: `W_UNDERDETERMINED` fires on `auto_reach_conn_free.ri`, `E_EVAL_UNRESOLVED` on `auto_reach_geo_loop_a.ri`. The one premise not yet observed — the cold-path `@optimized`-let doomed solve and the non-bare wrapper `2.0 * optfn(x)` — is β1's first step (§11 OQ-1), not a signal any leaf asserts before proving it.

## 4. Resolved design decisions

### Leo's rulings (verbatim)

From the record, `plans/gap-fill-deliberation-reify-20261016.md` §K3-D2 (human, 2026-10-16):
> B. New stream `auto-reach-relation`, sequenced after S3 δ #8691, S4 ν #8860 and κ #8868 so those moves keep their behaviour-preserving oracles: α `DependencyGraph::template_reads()` projection keyed on `InstancePath::structure_scope` with alias edges (raw spelling → canonical node, added once at construction) and the unfold budgets as constructor arguments (S3 seam); β `auto_reach.rs` `AutoReach { by_auto: Vec<Reacher { source, via: ReadPath, drivable }> }` built once per eval below the engine SCC from one declared `ReachEdges` policy (through-@optimized and realization-chain choices ruled at authoring — they are where the six walks disagree), consumed by the problem builder's admission (inside #6690's single builder), `detect_underdetermined` (in κ's detectors.rs), `compute_clusters` and `constraints_reaching_auto`; γ a cross-check over υ #8877's differential fixtures pinning "W_UNDERDETERMINED fires iff the builder admitted no constraint for that auto". Absorbs #6649 (its Indeterminate pin carried) and re-verifies #7069 (absorbed or cancelled as stale). #6690 receives a cross-reference only (critic C5): its let-mediated reach predicate is the one site the stream later replaces.

Authoring rulings, this session (human, 2026-10-18; question, then the option chosen, quoted):
- **R1 — W_UNDERDETERMINED semantics with undrivable reachers.** Chosen: "II: no drivable reacher (Recommended)" — "Literal to your ruling: W fires iff the builder admitted nothing for the auto, and the message lists the blocked reachers and why (e.g. 'GeoLoopB#constraint[0] — blocked: reaches it only through geometry cell Leaf.body'). Keeps #6649's carried pin. Cost: geo_loop_a/geo_loop_b gain an auto-side W they lack today, alongside the constraint-side E_EVAL_UNRESOLVED."
- **R2 — `@optimized` reached through a let.** Chosen: "Undrivable (Recommended)" — "Reacher recorded with Undrivable(FrozenComputeCell(c)): not admitted, constraint declined with a block message, W names the block. Same predicate as build_dependent_cells' exclusion, widened from 'is a bare @optimized call' to 'contains one' (β1 must prove the wrapper shape is reachable with a fixture first). Clusters unchanged (bt9 stays green). An inline @optimized call IN the constraint is unaffected: it is dispatched per trial."
- **R3 — a parent constraint on a template-keyed child auto.** Chosen (Leo's own wording): "Undrivable now, drivable later via whole-model-objective-coupling".
- **R4 — #6307 and #7069.** Chosen: "Absorb both into β1 (Recommended)" — "Connector delegation becomes a ReadPath hop on AutoReach; the connector correspondence moves to one home used by both the pin writers and the relation. Port task/6307's 775-line e2e (four reproducers, guards, nine-row verdict table) as β1 acceptance first, then cancel #6307 and #7069 with x_subsumed_by β1. The branch's engine_eval.rs changes are NOT merged. Its two documented limits (let over unpinned auto; nested connector) flip to correct answers."
- **R5 — lifetime.** Chosen: "Once per transaction (Recommended)" — "Built at two points by one free fn: in Engine::eval where ClusterFormationCtx is built today, and in edit_param right before its resolution sub-pass, both from the retained DependencyGraph + a ScopeMap. Stored in EvaluationState so Engine::build's E_EVAL_UNRESOLVED gate reads it. The builder takes &AutoReach as a parameter; S4 κ's detector reads it via ColdArtifacts (replacing CellReadIndex there); compute_clusters gets it via ClusterFormationCtx. ScopeMap retained in S1's EvalArtifacts — a cross-reference line to S1, not a dependency edge."
- **R6 — G4 ownership and the ι edge.** Chosen: "Yes to both (Recommended)" — this stream owns AutoReach + ReachEdges, `TemplateReads`/`ScopeMap`, and the cluster-formation reach query (compute_clusters' scheduling and caps stay where they are); α ← #8704.
- **R7 — invariant registration.** Chosen: "New INV-EVAL-7 row (Recommended)" — one `docs/invariants.md` row, owner this stream; β1 adds it as `partial`, β2 extends it, γ flips it to `enforced(type+test)`.
- **R8 — leaves and gating.** Chosen: "As proposed (Recommended)" — α (medium) ← #8691 #8704 #8860; β1 (high) ← α #6690 #8868, absorbs #6307/#7069; β2 (high) ← β1, absorbs #6649; β3 (high) ← β1; γ (high) ← β1 β2 β3; δ (medium) ← γ; ω (medium) ← all; gate #9171 ← β1 β2 β3 γ.
- **R9 — δ's shape (after the critic pass).** Asked: δ's design is open (the parent constraint reads a graph-minted `P.l.k` node nothing writes; clusters today need an objective; resolve_order's bt10 fence, WMOC INV-2/BT-10 and joint-drive-seam decision 7 all say "constraint reads never union"). Chosen: "Pending decision milestone (Recommended)" — "δ becomes a dep-gated [MILESTONE] decision gate (execution_class='decision' → born-at-L2, no agent) depending on γ: when γ lands it escalates to you with the three design questions above, the two fixtures (tk_child_let works via template-keyed reads; cross_scope_child_auto needs the P.l.k rebinding) and bt10/INV-2 as the fence to re-rule. Your ruling then files the implementation leaf. No OPEN design sits on a dispatchable leaf; R3's 'later' stays literal and tracked."
- **R10 — R5's retention home.** Asked: R5's option text named S1's `EvalArtifacts`, which does not exist on main. Chosen: "EvaluationState now (Recommended)" — "Use today's EvaluationState; S1 moves the fields if its partition says so (cross-reference line, no dependency edge). No wait on S1."

### Lead decisions (this session; each within the rulings above)

- **D1 — One edge policy, one orthogonal Drivability axis** (R1, R2; heuristic 3). `ReachEdges` follows every value-cell read (including through `@optimized` and geometry cells, across instance spellings, through connector delegation). Whether the solver can drive the auto along that path is a separate fact, `Drivability`, computed once per reacher from the path by one function. The six walks never disagreed about whether a read exists; they disagreed about what to do with a cell the solver cannot refold. Consumers differ only by a filter on `drivable`, never by a second policy.
- **D2 — Realization edges are not a separate class.** The geometry-valued value cell is the node; a path that crosses one is `Undrivable(GeometryCell(witness))`. W4's realization→realization walk is a subset of value-cell reads (a boolean `let asm = union(a, b)` is a geometry cell reading geometry cells), and the value-cell view also covers W4's documented blind spot and the `CrossSubGeometryRef` escape (#6649) by construction.
- **D3 — "Keyed on `InstancePath::structure_scope`" means keyed on the canonical node `ScopeMap::canonical(structure_scope(id))`.** ι's `structure_scope()` strips index segments only; mapping a plain sub's instance path (`Holder.b`) to its structure (`Bearing`) needs the instance-path map, and `EvaluationGraph::from_templates` copies a plain sub's child `default_expr` unrescoped (`Holder.b.fit` reads `Bearing.bore`), so the bridge is needed on the graph too. This is the ruling's own "raw spelling → canonical node", read precisely; it is not a re-ruling.
- **D4 — α's shape: a method on `DependencyGraph` taking the scope data** (resolves seams C1–C3). `DependencyGraph::template_reads(&self, scopes: &ScopeMap) -> TemplateReads`. `from_graph_and_fields` is untouched; the unfold budgets are constructor arguments of `ScopeMap` (the projection's input), which is the ruling's "budgets as constructor arguments" at the right type. Alias edges live only inside `TemplateReads`: `forward`, `reverse()`, S3's inversion law ("nothing else is in `reverse()`") and B5 are untouched. `template_reads` iterates already-emitted forward edges, never `EvaluationGraph` node maps, so S3 δ's "exactly one function iterates `EvaluationGraph` node maps to emit read edges" row stays at one.
- **D5 — `ScopeMap` is the one canonicalisation home** (`crates/reify-eval/src/instance_scope.rs`, new). It absorbs `build_instance_path_structure_map`, the map half of `normalize_cell_id` (its string half is ι's `structure_scope()`), `build_single_instance_alias_paths` (the stage-(g) oracle) and the connector correspondence now inlined in `connector_pin_if_determined` / `is_strict_connector_instance_auto`. The pin writers and the relation call the same `ScopeMap::connector_delegate`, so "assign" and "warn" cannot drift (R4).
- **D6 — Module placement** (S4 D13; INV-ARCH-1; resolves seams C8). `instance_scope.rs` imports only the reify_core / reify_ir / reify_compiler crates — never `deps` or any other `crate::` module — and `deps.rs` imports `instance_scope` for the `ScopeMap` type (`template_reads` names it), so the pair forms no SCC. `auto_reach.rs` imports `deps` and `instance_scope` and the same crates. Neither imports `resolve_order`, `engine_fixpoint`, `solver_problem`, `detectors`, `undef_tracer` or the engine residual SCC, and neither names `Engine`; consumers import them downward. Importing `deps` is safe only because the stream follows S3 β/δ, which take `deps` out of the baselined 24-module SCC (today `deps.rs` imports `geometry_ops`, an engine-SCC member). Seed expansion (`expand_solver_position_expr`, which needs the trait registry and values) stays with the callers, which hand `(ReachSource, &CompiledExpr)` pairs in.
- **D7 — The frozen-compute predicate is containment, with one home.** `contains_optimized_call(expr, functions)` replaces `is_optimized_userfn_cell` and is called by both `build_dependent_cells`' fold exclusion and `ReachEdges::drivability` (R2). β1 pins the wrapper shape with a fixture before relying on it; if the wrapper is unreachable in practice, β1 records why and keeps the bare-call match.
- **D8 — Two classes of block; only the refusal class declines a constraint** (critic B1). A *refusal-class* block — `GeometryCell`, `FrozenComputeCell` — means no solve this run can evaluate the constraint against the auto: the constraint is declined whole (today's W4 rule, generalised from geometry to frozen compute), reported `E_EVAL_UNRESOLVED`, and skipped by the post-geometry check. A *non-driving-class* block — `MultiInstanceAlias`, `ChildScopeConstraint`, `CrossScopeConstraint`, `UndeterminedDelegate` — means only that this reacher does not drive this auto; the constraint is otherwise ordinary (admitted wherever it has a drivable reacher, checked as today). So `Later.y > Leaf.k` stays admitted for `Later.y` and `Leaf.k` gets the warning; `Sib#constraint[0]`, `Bearing2#constraint[0]` and `P#constraint[0]` keep today's verdicts.
- **D9 — Scope of consumers** (resolves seams C10a). In scope as projections of the one relation: the four named consumers; `build_dependent_cells` (the fourth consumer of `CellReadIndex` today, and the fold that *makes* a path drivable, so it must read the same relation); `objective_auto_reach` (E_OBJECTIVE_UNCONSUMED's re-derivation). Out of scope, with reasons, in §7.
- **D10 — γ stands alone; it does not widen υ** (resolves seams C7; G1 rule). υ #8877 does not exist on main, has no underdetermined row, and compares across paths only what is not eval-only. γ is its own e2e module over the eighteen committed fixtures with an eval column and an edit column; υ may adopt the rows when it lands.
- **D11 — Ratchet vehicle: `tests/infra/adoption-ratchet.manifest`** (exists on main) rather than a new member on S9's unbuilt `rust_construct_ratchet` engine. Rows in §5.8.
- **D12 — Edit rebuild cost.** Rebuilding AutoReach per `edit_param` is O(cells + reads), comparable to today's `CellReadIndex::build` plus one closure. β1's commit message records a measurement on `examples/whole_model_joint_drive.ri`; retain-and-invalidate (the R5 alternative) is the fallback, breadcrumbed with that trigger.
- **D13 — Drivability across scopes is source-relative and problem-relative** (critic M2). The `cross_scope_*` arm applies to `ReachSource::Constraint` only: an objective-term reacher across scopes is `Drivable` (whether the scopes are co-solved is `compute_clusters`' decision, downstream — bt8 keeps forming its cluster). A `CrossScopeConstraint` reacher counts as drivable inside a merged-cluster problem whose scope set holds both the constraint's scope and the auto's owner, which keeps today's merged-cluster admission (the merged builder admits each member's constraints against the cluster-wide autos). The relation stores the block; `admitted_constraints` and the detector resolve co-solving from the scope set they are given (the detector reads the resolve-order clusters from `ColdArtifacts`, S4 D7).
- **D14 — The auto universe is the templates' auto value cells** (critic M3): every `kind.is_auto()` `ValueCellDecl` of every template (regular, instance-path minted, connector-instance), recorded by `ScopeMap::from_templates` as `ScopeMap::autos()` so the edit construction point, which has no templates in scope, reads the same universe (fold-verify N1). A graph node minted under a plain sub for a template-keyed auto (`P.l.k` for `Leaf.k`) is an alias spelling of that auto (`TemplateReads::aliases_of`), never an auto of its own — so it is never warned about.
- **D15 — A path is instance-consistent** (critic m10). A reacher exists for auto A only if every rescoping on its path resolves to A's own instance (`Sib.b1.fit` reaches `Sib.b1.bore`, never `Sib.b2.bore`). The alias law's spelling sets (§5.2) are an over-approximation used to find candidate paths; they are not the reach rule.
- **D16 — Fixture registration falls on the first compiled reader, which is α** (critic M5; fold-verify N2). α's alias-law test (B1, in-file in `deps.rs`/`instance_scope.rs`) compiles all eighteen `auto_reach_*.ri` fixtures, so α registers all eighteen in `_RUST_COUPLED_RI_FIXTURES` in its own diff; later leaves read them by path (β1's ported connector e2e uses the committed `auto_reach_conn_*.ri` files, not inline copies — one home, heuristic 11). Once registered, re-baselining one of these fixtures escalates the pre-commit to the full workspace suite — such an edit goes through a task branch, never a docs-landing commit.
- **D17 — Absorbed tasks are cancelled at filing, after their content is ported into β1/β2's records** (R4; brief's absorption procedure; critic m7). R4's "port first, then cancel" is satisfied before the cancel by the committed fixtures, the manifest's verdict table and the dated details appended to β1/β2; porting the test *code* onto the relation is β1's first step. The durable content is committed with this PRD: all eleven #6307 sources as `auto_reach_conn_*.ri` fixtures and its verdict table in the manifest, and #6649's two shapes as `auto_reach_geo_loop_{a,b}.ri`. The full #6307 test file stays readable at `task/6307` tip `42795f734b`. None of the three has a dependent (§10).

## 5. Contract (H)

### 5.1 `ScopeMap` (`crates/reify-eval/src/instance_scope.rs`, α)

```rust
pub(crate) struct UnfoldBudget { pub depth: usize, pub nodes: usize }
pub(crate) struct ScopeMap { /* private */ }
impl ScopeMap {
    pub(crate) fn from_templates(templates: &[TopologyTemplate], budget: UnfoldBudget) -> ScopeMap;
    pub(crate) fn budget(&self) -> UnfoldBudget;
    /// The canonical (template-keyed) node for any spelling; `None` for an id that is already canonical.
    pub(crate) fn canonical(&self, id: &ValueCellId) -> Option<ValueCellId>;
    /// The single non-collection instance path of `structure`, if exactly one exists (stage-(g) oracle).
    pub(crate) fn single_instance(&self, structure: &str) -> Option<&InstancePath>;
    /// For a connector-instance auto (`E.__connector_0.gain`), its connector type's cell (`ConnAuto.gain`).
    pub(crate) fn connector_delegate(&self, id: &ValueCellId) -> Option<ValueCellId>;
    /// The auto universe (D14): every template's `kind.is_auto()` value cell, with its owning scope and `auto(free)` flag.
    pub(crate) fn autos(&self) -> &[AutoCell];
}
pub(crate) struct AutoCell { pub id: ValueCellId, pub owner: String, pub free: bool }
```
Invariants: built by `Engine::eval` from the templates (the only point where they are in scope), retained on `EvaluationState`, and passed by reference — `edit_param` reuses it, which is why it carries the auto universe (no `OnceCell`, no consumer-chosen budget — heuristic 7); `canonical` is many-to-one in the canonicalising direction only; entity-string parsing goes through `InstancePath` (ι's ratchet count stays 0).

### 5.2 `TemplateReads` (`deps.rs`, α)

```rust
impl DependencyGraph {
    pub fn template_reads(&self, scopes: &ScopeMap) -> TemplateReads;
}
pub(crate) struct TemplateReads { /* private: canonical forward + reverse, alias table */ }
impl TemplateReads {
    pub(crate) fn reads_of(&self, canonical: &ValueCellId) -> &[ValueCellId];
    pub(crate) fn readers_of(&self, canonical: &ValueCellId) -> &[ValueCellId];
    pub(crate) fn aliases_of(&self, canonical: &ValueCellId) -> &[ValueCellId];
    pub(crate) fn rescope(&self, template_keyed: &ValueCellId, instance: &InstancePath) -> ValueCellId;
}
```
**Alias law** (α's oracle; B5 does not cover the projection): every alias edge targets a canonical node; for every cell, `aliases_of(c) ∪ {c}` equals the spelling set today's `read_closure` holds for it; `readers_of` is the transpose of `reads_of` by construction (one edge list, two indexes — "reverse == transpose(forward)") and a `debug_assert` checks it at construction; `single_instance` agrees with `build_single_instance_alias_paths` on every existing fixture.

### 5.3 `AutoReach` (`crates/reify-eval/src/auto_reach.rs`, β1)

```rust
pub(crate) struct AutoReach {
    by_auto: Vec<Reacher>,                               // the ruled carrier, sorted by (auto, source); immutable after build
    // tactical indexes over by_auto (per-auto slice, per-source transpose) — private
}
pub(crate) struct Reacher { pub auto: ValueCellId, pub source: ReachSource, pub via: ReadPath, pub drivable: Drivability }
pub(crate) enum ReachSource {
    Constraint(ConstraintNodeId),
    ObjectiveTerm { scope: String, term: usize },        // index into ObjectiveSet.terms
    EvaluatedDefault(ValueCellId),                       // a connector delegate's concrete default or let (R4)
}
pub(crate) struct ReadPath {
    pub scope: ScopeRelation,                            // Own | CrossScope { source_scope } | ChildScope
    pub hops: u32,                                       // 0 = direct read
    pub alias: Option<AliasRescoping>,                   // child-side let crossed, and the instance it was rescoped to
    pub delegate: Option<ValueCellId>,                   // connector delegation hop
    pub first_block: Option<Block>,                      // the first crossing the solver cannot drive through
}
pub(crate) enum Block {
    GeometryCell(ValueCellId), FrozenComputeCell(ValueCellId),
    MultiInstanceAlias { structure: String, instances: usize },
    ChildScopeConstraint, CrossScopeConstraint { source_scope: String },
    UndeterminedDelegate(ValueCellId),
}
pub(crate) enum BlockClass { Refusal, NonDriving }      // D8: GeometryCell, FrozenComputeCell are Refusal; the rest NonDriving
impl Block { pub(crate) fn class(&self) -> BlockClass; }
pub(crate) enum Drivability { Drivable, Undrivable(Block) }
impl AutoReach {
    pub(crate) fn build(reads: &TemplateReads, scopes: &ScopeMap, seeds: &[(ReachSource, &CompiledExpr)],
                        policy: &ReachEdges) -> AutoReach;   // the auto universe is scopes.autos() (D14)
    pub(crate) fn reachers_of(&self, auto: &ValueCellId) -> &[Reacher];
    pub(crate) fn reachers_from(&self, source: &ReachSource) -> impl Iterator<Item = &Reacher>;
    pub(crate) fn admitted_constraints<'c>(&self, problem: &ProblemScopes,
        candidates: impl Iterator<Item = &'c CompiledConstraint>) -> Vec<(ConstraintNodeId, &'c CompiledExpr)>; // AR-1
    pub(crate) fn declined_constraints(&self) -> BTreeSet<ConstraintNodeId>;                                   // AR-2: refusal-class only
    pub(crate) fn undriven_strict_autos(&self, co_solved: &dyn Fn(&str, &str) -> bool)
        -> impl Iterator<Item = (&ValueCellId, &[Reacher])>;                                                 // AR-3
    // ProblemScopes = the scope names of one ResolutionProblem (one scope, or a merged cluster's members) and their regular autos.
    pub(crate) fn auto_owners_reached_by(&self, term: &ObjectiveTermRef, owner_of: impl Fn(&ValueCellId) -> Option<usize>) -> BTreeSet<usize>; // clusters: objective sources only, drivable only
    pub(crate) fn dependent_cells_for(&self, scope_autos: &HashSet<ValueCellId>, sources: &[ReachSource]) -> Vec<(ValueCellId, CompiledExpr)>;
}
```
A new edge kind is one `ReadPath` field or one `Block` arm, decided in `ReachEdges` — never a new walk. Container types beyond `by_auto: Vec<Reacher>` are tactical.

### 5.4 `ReachEdges` and drivability (β1; R1, R2, R3)

```rust
pub(crate) struct ReachEdges {
    pub geometry_cells: Crossing,           // Follow + Block (D2)
    pub optimized_cells: Crossing,          // Follow + Block, containment predicate (R2, D7)
    pub child_side_alias: AliasRule,        // SingleInstanceOnly
    pub cross_scope_constraints: Crossing,  // Follow + Block (R3); constraint sources only; lifted when co-solved (D13)
    pub child_scope_constraints: Crossing,  // Follow + Block
    pub connector_delegation: bool,         // Follow (R4)
}
pub(crate) const RULED: ReachEdges = …;
impl ReachEdges { pub(crate) fn drivability(&self, via: &ReadPath) -> Drivability; }
```
`drivable` is a pure function of `via` and `RULED`, computed in `drivability` and nowhere else (heuristic 10, one home), stored on the reacher (the ruled field), and `debug_assert`-re-derived where consumed (heuristic 10, redundantly). Tests build policy variants to pin each arm's effect. Two refinements bind every consumer: the block's class (D8) decides decline vs non-driving, and a `CrossScopeConstraint` block is lifted for a problem that co-solves both scopes (D13). The `cross_scope_constraints` arm applies to constraint sources only; objective-term reachers across scopes are drivable (D13). Paths are instance-consistent (D15).

Per-shape answers under `RULED` (the §10 fixtures): direct read of an own or instance-path auto — drivable; through parent-side lets — drivable; through a child-side let with a single instance path — drivable (stage-(g) alias); the same with two instances (`auto_reach_two_siblings.ri`) — `Undrivable(MultiInstanceAlias)`; a constraint inside the declaring child (`auto_reach_constraint_inside_child.ri`) — `Undrivable(ChildScopeConstraint)`; a parent constraint on a template-keyed child auto (`auto_reach_cross_scope_child_auto.ri`, `auto_reach_tk_child_let.ri`) — `Undrivable(CrossScopeConstraint)`, non-driving class (R3; the δ milestone decides "later"); through a geometry cell (`auto_reach_geo_loop_a.ri`, `auto_reach_geo_loop_b.ri`) — `Undrivable(GeometryCell)`; through an `@optimized`-containing let — `Undrivable(FrozenComputeCell)`; a connector-instance auto whose delegate is pinned by a constraint or a determinable default (`auto_reach_conn_pinned.ri`, `auto_reach_conn_concrete.ri`) — drivable through the delegate; a delegate whose own reachers are all undrivable — `Undrivable(UndeterminedDelegate)`. Objective terms are sources like constraints.

### 5.5 Lifetime and construction points (R5; resolves seams C5, C6)

- **One free fn** `auto_reach::build_auto_reach` builds the relation at two points, with any path difference as a parameter (SDP I3, B15's spirit): (1) `Engine::eval`, where `ClusterFormationCtx` is built today (after the field, Param/Let, Auto pre-seed, sub-component, structural-query and self-datum phases), before `resolve_order`; (2) `Engine::edit_param`, after its re-elaboration and immediately before its resolution sub-pass, from the retained graph.
- **Retention (R5, R10).** `EvaluationState` (`lib.rs`) gains `auto_reach: AutoReach` and `scope_map: ScopeMap`. `Engine::eval` sets both; `edit_param` rebuilds `auto_reach` and keeps `scope_map`, which is built from templates alone (structure, not values — collection subs contribute their index-stripped path), so a parameter edit never stales it. `Engine::build`'s unified pass reads `state.auto_reach.declined_constraints()` where it reads `trace_map` today. S1's engine-state partition classifies the two fields (cross-reference line, no edge).
- **Routes.** The single builder takes `&AutoReach` as a parameter. `detect_underdetermined`'s `run` reads `ColdArtifacts.auto_reach` (κ's `CellReadIndex` member is replaced; `cold` stays `Some` on `Eval` only, so `W_UNDERDETERMINED` stays eval-only as S4 D7 declares). `compute_clusters` reads it through a `ClusterFormationCtx` field. The decline gate is a call on the retained instance.
- **Edit seeds.** The edit construction point takes constraints from the retained graph (already expanded) and objective terms from `self.objectives`, expanded with the retained trait registry; if a structural-query objective cannot be expanded on edit, β1 records the asymmetry (edit does no clustering today, so this is not a regression).

### 5.6 Consumers after the stream

| Consumer | Query | Leaf |
|---|---|---|
| single `ResolutionProblem` builder (cold, merged cluster, edit) | `admitted_constraints(problem, candidates)` | β1 |
| connector pin writers (`connector_pin_if_determined`, `is_strict_connector_instance_auto`) | `ScopeMap::connector_delegate` (the correspondence); the pin is part of "the builder" for AR-3 | α (correspondence), β1 |
| `detect_underdetermined` (`PostPassDetector`) | `undriven_strict_autos(co_solved)` with co-solving from `ColdArtifacts`' resolve-order clusters; message lists the reachers and their blocks | β1 |
| `build_dependent_cells` | `dependent_cells_for(scope_autos, admitted sources)` | β1 |
| `objective_auto_reach` (E_OBJECTIVE_UNCONSUMED) | `reachers_from(ObjectiveTerm)` drivable | β1 |
| undef-cause writer (today `record_failed_autos`) | `UndefCause::ReachNotDrivable { constraint, block }` for an auto whose only reachers are undrivable | β1 |
| `engine_fixpoint` decline gate / `E_EVAL_UNRESOLVED` | `declined_constraints()` | β2 |
| `check_constraints_post_geometry` decline set | `declined_constraints()` | β2 |
| `resolve_order::compute_clusters` | `auto_owners_reached_by(&ObjectiveTermRef, …)`, drivable only; the parameter type admits objective terms only (constraint reads never union, scope-coupling INV-2) | β3 |

### 5.7 INV-EVAL-7 (R7) — clauses

**INV-EVAL-7:** *Auto reach is one relation: constraint admission, the decline gate and W_UNDERDETERMINED are projections of one `AutoReach` per transaction.* Clauses:
- **AR-1 (admission).** Constraint C is in problem P's `ResolutionProblem` (P = one scope, or a merged cluster) ⇔ C is one of P's scopes' constraints ∧ C has a reacher (A, C) that is drivable for P (drivable, or a `CrossScopeConstraint` reacher whose two scopes are both in P — D13) with A among P's regular autos ∧ ¬declined(C).
- **AR-2 (decline).** declined(C) ⇔ some reacher of C carries a refusal-class block (`GeometryCell`, `FrozenComputeCell` — D8) ⇔ `E_EVAL_UNRESOLVED`(C); the post-geometry check skips exactly the declined set. A non-driving-class block never declines.
- **AR-3 (auto side, R1).** A reacher of auto A is *effective* iff it is drivable for the problem that owns A (D13) and its source is not declined (AR-2). For a strict (non-`auto(free)`) auto A: `W_UNDERDETERMINED`(A) ⇔ A has no effective reacher (constraint, objective term, or connector delegate) ⇔ the builder admits no constraint that reaches A, the problem's objective reaches A through no drivable path, and A is not pinned from its delegate (critic M1: the connector pin writer is part of "the builder" in R1's sentence). A constraint that drivably reaches A but is declined through another reacher therefore leaves A warned — consistent with AR-1, which does not admit it. The message lists A's reachers and each one's block.
- **AR-4 (one source).** All three are queries on the same `AutoReach` instance of the transaction.
Enforcement: **type** (one `AutoReach` constructor; the consumers' only input is `&AutoReach`; `drivable` computed in one fn), **test** (γ's e2e module asserts AR-1 ⇔ AR-3 and AR-2 ⇔ E on all eighteen fixtures, eval and edit), **debug_assert** (the detector re-derives AR-3 from the builder's admitted ∪ pinned set on `Eval`). Status: β1 `partial: enforced(type+test) for admission and W_UNDERDETERMINED, proposed for the decline gate`; β2 extends; γ `enforced(type+test)`. Written in S24's pointer grammar once S24 ξ #8821 has landed, inline in the Enforcement cell before that.

### 5.8 Ratchet rows (`tests/infra/adoption-ratchet.manifest`; D11; INV candidate 5)

Each row is added by the leaf that brings its count to the ceiling, in the same diff, and named in that leaf's done criteria. The engine (`tests/infra/adoption_ratchet.py`) excludes only lines whose first non-blank character is `#`; `//` and `///` comment lines COUNT, and `#[cfg(test)]` module bodies count. So a ceiling-0 row means no production line, doc comment or test in its glob may spell the retired symbol — a pointer to this PRD replaces any prose that would name it. No row here needs alternation; a row that ever does writes a bare pipe in the manifest (an escaped pipe is a literal under Python `re` and makes the row vacuous). One glob per row.
| pattern-id | construct (ERE) | path | ceiling | leaf |
|---|---|---|---|---|
| `auto-reach-normalize-cell-id` | `normalize_cell_id[(]` | `crates/reify-eval/src/*.rs` | 0 | α |
| `auto-reach-engine-name-scope` | `Engine` (any substring — the module names no engine type at all) | `crates/reify-eval/src/instance_scope.rs` | 0 | α |
| `auto-reach-cell-read-index` | `CellReadIndex` | `crates/reify-eval/src/*.rs` | 0 | β1 |
| `auto-reach-pinned-through-reader` | `auto_is_pinned_through_a_reader` | `crates/reify-eval/src/*.rs` | 0 | β1 |
| `auto-reach-engine-name` | `Engine` (as above) | `crates/reify-eval/src/auto_reach.rs` | 0 | β1 |
| `auto-reach-extract-value-deps` | `extract_value_deps[(]` | `crates/reify-eval/src/*.rs` | the whole-glob count at β1's landing (test modules included), each survivor named in the row's owner column | β1 (β3 lowers) |
| `auto-reach-realizations-reaching` | `realizations_reaching_auto` | `crates/reify-eval/src/*.rs` | 0 | β2 |
| `auto-reach-non-auto-cell-map` | `build_non_auto_cell_map` | `crates/reify-eval/src/*.rs` | 0 | β3 |
| `auto-reach-union-via-owners` | `union_via_transitive_auto_owners` | `crates/reify-eval/src/*.rs` | 0 | β3 |
S4's E11 row (`\bEngine\b` in its new modules, on S9's not-yet-landed `rust_construct_ratchet`) and the two `Engine` rows here are one rule on two vehicles — a recorded heuristic-11 debt; whichever stream lands second folds them (cross-reference line to S4).
The `extract_value_deps(` row is the K3-D2 framing's "exactly one function iterates value-cell reads to emit reach edges", expressed as a count of call sites that S3 δ's own `deps.rs`-scoped row does not cover.

### 5.9 Same-diff obligations on every leaf that moves or deletes code

S4 D13's list applies: PDIAG baseline regeneration and PTODO fingerprint re-keying for moved or deleted diagnostics/TODOs; after S24 ο #8822, `plinecite-baseline-gen --rename <old>=<new>` for a moved file under PLINECITE scope; after S24 ξ #8821, a `docs/invariants.md` Pointer naming a moved or renamed symbol is updated. `python3 scripts/assert-module-scc.py check` is green with no `GREW` after every leaf.

### 5.10 G7 — design-invariants walk (`docs/legibility/design-invariants.md`)

Walked every leaf against INV-SF-1..7, INV-PD-1/2 and INV-ARCH-1. No waivers.
- **INV-SF-1 `undef-has-provenance`** — β1 replaces the misattributed `solve failed: infeasible` and the false `… awaiting solve` with `UndefCause::ReachNotDrivable { constraint, block }` for an auto whose only reachers are undrivable (resolution, not waiver).
- **INV-SF-3 `declared-intent-consumed-or-diagnosed`** — every constraint that touches an auto it cannot drive is now named, either in `W_UNDERDETERMINED`'s reacher list (non-driving class) or as `E_EVAL_UNRESOLVED` (refusal class). The constraint inside a declaring child (`auto_reach_constraint_inside_child.ri`) is diagnosed through the auto's warning; re-projecting it onto the instance is out of scope (§7).
- **INV-SF-4 `indeterminate-attributable-transient`** — β2 removes the false `VIOLATED` on `auto_reach_geo_loop_b.ri`. A geometry-declined constraint is unresolved in every run of that model; its diagnosis is the `E_EVAL_UNRESOLVED` error (exit 1), which is the attributable cause the invariant requires — the same posture the own-scope case has on main.
- **INV-SF-6 `diagnostics-carry-codes`** — no new code. `W_UNDERDETERMINED` keeps `DiagnosticCode::Underdetermined` (the reacher list is message text under it). The `FrozenComputeCell` decline reuses `DiagnosticCode::EvalUnresolved`; its message names the block ("… through a frozen @optimized cell `c`" beside today's "… through geometry-backed inputs"). β1 states the exact text (§11 OQ-3).
- **INV-SF-5 / umbrella `nothing-vacuous-and-unowned`** — OQ-2 (collection-sub autos) is owned by α: if the compiler mints no such autos, α records the rows as vacuous-today in its commit and γ's module asserts the fixture set it covers, so the vacuity is owned, not silent.
- **INV-SF-2, INV-SF-7, INV-PD-1/2** — not engaged: no new `error:` path without a non-zero exit (E_EVAL_UNRESOLVED already exits 1), no grammar, no declared result fields.
- **INV-ARCH-1** — two new modules, no new SCC (D6); `python3 scripts/assert-module-scc.py check` green with no `GREW` is in every code leaf's done criteria.

## 6. Boundary-test sketch (two-way)

| # | Side | Scenario | Preconditions | Postconditions |
|---|---|---|---|---|
| B1 | producer (α) | alias law (in-file test modules of `deps.rs` / `instance_scope.rs`) | existing deps fixtures + the eighteen `auto_reach_*` fixtures | every alias edge targets a canonical node; spelling sets equal today's `read_closure`; `readers_of` = transpose(`reads_of`); `single_instance` = `build_single_instance_alias_paths` |
| B2 | producer (β1) | drivability is a function of the path | `ReachEdges` variants built in the test, one arm flipped each | each flip changes exactly the shapes §5.4 lists for that arm |
| B3 | consumer: builder (β1) | AR-1 on cold and edit | each fixture; `eval` then an `edit_param` on an unrelated param; observed through a recording `ConstraintSolver` (SDP B9's shape — `AutoReach` is `pub(crate)`, so the test reads the built problem, never the relation) | the built `ResolutionProblem.constraints` are equal across the two paths (SDP B9 extended) |
| B3b | consumer: builder (β1) | non-driving vs refusal | `tests/underdetermined.rs`' `Later.y > Leaf.k` shape; merged-cluster suites (`tests/merged_cluster_solve.rs`) | `Later#constraint` admitted for `Later.y`; `W_UNDERDETERMINED` for `Leaf.k` only; a parent constraint on a child auto inside an objective-formed merged cluster still admitted (D13) |
| B4 | consumer: detector (β1) | AR-3 | each fixture | the set of autos with `W_UNDERDETERMINED` equals the strict autos with no admitted constraint, no drivable objective reacher and no delegate pin; each message lists the reachers and blocks |
| B5 | consumer: connector (β1) | delegation | `auto_reach_conn_{pinned,concrete,free}.ri` + the ported #6307 nine-row table | pinned/concrete: no W and `E.__connector_0.gain = 0.01 m`; free: two W; the branch's rows 8 and 9 give the relation's answers |
| B6 | consumer: gate (β2) | AR-2 | `auto_reach_geo_loop_{a,b}.ri` | `E_EVAL_UNRESOLVED` on both; no `VIOLATED`; no `constraints could not be satisfied`; the undef cause is `ReachNotDrivable` |
| B7 | consumer: clusters (β3) | clusters unchanged | resolve_order's bt8/bt9/bt10/δ-cluster suites; `examples/whole_model_joint_drive.ri`, `examples/cost_subtree_aggregate.ri` | cluster sets and full `reify eval` output byte-identical to before β3 |
| B8 | integration (γ) | one relation end to end | all eighteen fixtures, eval and edit columns | B3, B4 and B6 hold together on every fixture; a seeded single-site regression (one consumer re-deriving reach) reds the module |

## 7. Out of scope

- `resolve_order::build_read_dag` / `emit_cycle_coupling_diagnostics`: one-hop direct reads for scope ordering and `W_SCOPE_COUPLING`, deliberately non-transitive (scope-coupling INV-2), byte-pinned by the scope_coupling suites, and computed before values exist. In principle the `hops == 0` projection of AutoReach; breadcrumb: if INV-2 is re-ruled, `build_read_dag` becomes `AutoReach` filtered to `via.hops == 0`.
- reify-constraints `dependent_reads.rs` / `decompose.rs`: solver-side, over `ResolutionProblem.dependent_cells`, in another crate; `ResolutionProblem` is the boundary. Noted for the hotspot ledger: `dependent_reads::collect_value_refs` (ValueRef only) differs from `CompiledExpr::collect_value_refs` (also `CrossSubGeometryRef`).
- reify-compiler `inert_objective` (`E_OBJECTIVE_INERT`): the compile-time complement over templates, no instances, no values.
- `scope_qualifies_for_centrality`, `EvaluationGraph::is_auto_cell` / `deactivate_if_not_auto`: not reach relations.
- Making a cross-scope constraint drive a template-keyed child auto (R3 "later"): decided by the δ milestone after γ. Until then resolve_order's `bt10_constraint_only_transitive_read_forms_no_cluster` fence, whole-model-objective-coupling INV-2/BT-10 and whole-model-joint-drive-seam decision 7 ("constraint reads never union") stand unchanged.
- Making `MultiInstanceAlias` and `ChildScopeConstraint` reachers drivable (per-instance rescoping; re-projecting a child's constraints onto the alias path). Each is one `Drivability` arm flip later; breadcrumb trigger: a user model that needs it.
- Per-trial geometry re-realization (geometry-algebra PRD's successor); a typed `ValueCellId` carrying an `InstancePath` (S3 OQ-2, post-S1).
- The edit path's per-entity-string grouping (W5's `entity_groups`) is #6690's to replace; listed so the manifest does not double-count it.

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| S3 dependency-invalidation-stratum (δ #8691, ι #8704) | consumes | `DependencyGraph` (forward + `reverse()`), `InstancePath::structure_scope` | S3 | α ← #8691, #8704 |
| this stream, on S3's type | produces | `DependencyGraph::template_reads(&ScopeMap) -> TemplateReads`; alias law | auto-reach-relation | α (cross-reference line to S3) |
| S4 cell-eval-transaction-closure (ν #8860, κ #8868, φ #9292, υ #8877) | consumes | `solver_problem.rs` (ν); `PostPassDetector` + `ColdArtifacts` (κ; `ColdArtifacts.auto_reach` replaces `CellReadIndex`) ; D13 import rule | S4 | α ← #8860; β1 ← #8868 (κ ← φ); υ not a prerequisite (D10) |
| solver-driver-parity β #6690 | consumes | the single builder takes `&AutoReach`; admission = `admitted_constraints` | #6690 builds the builder; this stream swaps its one reach call | β1 ← #6690; cross-reference only on #6690 (record, critic C5) |
| S1 engine-state-partition | adjacent | `EvaluationState` gains `auto_reach`, `scope_map` | S1 classifies the fields | cross-reference line, no edge |
| whole-model-objective-coupling / whole-model-joint-drive-seam | adjacent; R3's "later" | cluster formation reads the relation (β3, behaviour-preserving); whether a cross-scope constraint may drive a child auto through a merged cluster — re-ruling WMOC INV-2/BT-10 and joint-drive-seam D7 — is the δ decision milestone's question | Leo (δ milestone), then the implementation leaf his ruling files | δ ← γ; cross-reference line |
| S24 (#8821, #8822) | adjacent | INV-EVAL-7 row in the pointer grammar; PLINECITE `--rename` | S24 owns format | same-diff obligations (§5.9) |
| INV-ARCH-1 (S3 α #8673, done) | consumes | two new modules, no new SCC | S3 owns tool | every leaf |

**Sequencing note (seams C9).** #6690 has no edge to ν or κ, so when β1 starts the single builder may live in `engine_eval.rs` or in `solver_problem.rs`; β1's files cover both, and β1 follows both through α ← ν and β1 ← #6690.

**Cross-reference lines (returned to the gap-fill lead; one editor per document):**
- S3 (#8691 owner): "auto-reach-relation α adds `DependencyGraph::template_reads(&ScopeMap) -> TemplateReads` in deps.rs, and deps.rs imports the new `instance_scope` module; alias edges live only in `TemplateReads`, so §5.2's inversion law and B5 are untouched."
- S4 (cell-eval-transaction-closure D7, κ #8868): "after auto-reach-relation β1, `ColdArtifacts` carries `&AutoReach` in place of `CellReadIndex`, and `detectors.rs` imports `crate::auto_reach::AutoReach` instead of `solver_problem::CellReadIndex`; κ stays a pure move. S4's E11 row and auto-reach's two `Engine` adoption-ratchet rows are one rule on two vehicles; whichever lands second folds them."
- S1 (engine-state-partition §5.6): "`EvaluationState` gains `auto_reach: AutoReach` (rebuilt by `eval` and `edit_param`) and `scope_map: ScopeMap` (built by `eval` from templates alone; kept across edits); classify both in the Engine-state partition."
- #6690: "the single builder takes `&AutoReach` as a parameter in β1; build toward this answer on edit_param: a constraint whose only reach into a scope's autos is cross-scope (parent constraint on a template-keyed child auto) is NOT admitted unless both scopes are co-solved in one merged cluster — the edit path's raw-match admission of it today is the divergence R3 resolves, so do not preserve it."
- whole-model-objective-coupling / joint-drive-seam: "R3 (2026-10-18) rules cross-scope constraints on template-keyed child autos undrivable now and drivable later 'via whole-model-objective-coupling'; the δ milestone (auto-reach-relation) brings the INV-2/BT-10/D7 re-rule to Leo after γ."

**New program G4 rows (returned to the gap-fill lead, R6):** `AutoReach relation + ReachEdges policy (crates/reify-eval/src/auto_reach.rs) | auto-reach-relation | single builder (#6690), S4 detectors.rs, resolve_order::compute_clusters, engine_fixpoint decline gate, check_constraints_post_geometry`; `DependencyGraph::template_reads() / TemplateReads + ScopeMap (instance_scope.rs) | auto-reach-relation (on S3's type) | AutoReach, the pinned-connector writers`; `resolve_order cluster-formation reach query | auto-reach-relation | —`.

## 9. Decomposition plan

Every leaf: `metadata.x_finding_run = "hotspot-survey-reify-20261015"`, `metadata.source = "hotspot-survey"`, `metadata.x_gapfill_ruling = "plans/gap-fill-deliberation-reify-20261016.md §K3-D2"`, `metadata.x_finding_key` = the keys it contributes to, cites INV-EVAL-7, `grammar_confirmed: true`. Every code leaf's done criteria carry §5.9 and `python3 scripts/assert-module-scc.py check` green with no `GREW`. No leaf is a prerequisite of #8691, #8860, #8704, #6689, #8863 or any task in #8612's transitive closure (recomputed live before wiring). G7: no waivers (§5.10).

- **α — `ScopeMap` + `DependencyGraph::template_reads` (#9368)** (medium; fk-a5237570b374). New `instance_scope.rs` (§5.1) and `TemplateReads` (§5.2); moves `build_instance_path_structure_map`, `normalize_cell_id`'s map half, `build_single_instance_alias_paths` and the connector correspondence into `ScopeMap` (the pin writers call `ScopeMap::connector_delegate`); repoints `CellReadIndex`'s three bridge sites, `build_dependent_cells`' rescoping and `resolve_order`'s as-is-then-normalised fallback to it; `CellReadIndex` takes `&ScopeMap`, and its budgets, the caller-owned `OnceCell` budget coupling and the `debug_assert` go (problem items 3, 4). Decides OQ-2. B1's alias-law tests (in-file in `deps.rs` / `instance_scope.rs`) compile all eighteen `auto_reach_*.ri` fixtures, so α registers all eighteen in `_RUST_COUPLED_RI_FIXTURES` (D16). Behaviour-preserving. Files: `deps.rs`, `instance_scope.rs`, `resolve_order.rs`, `solver_problem.rs`, `engine_eval.rs`, `detectors.rs`, `lib.rs`, `scripts/verify.sh`, `tests/infra/adoption-ratchet.manifest`. Prereqs: #8691, #8704, #8860. Intermediate (consumers β1, β3). Signal: B1 green; `reify check tests/prd-gate/fixtures/auto_reach_two_siblings.ri` still emits exactly two `W_UNDERDETERMINED` and `reify eval tests/prd-gate/fixtures/auto_reach_child_side_let.ri` still prints `Holder.b.bore = 0.01 m`; rows `auto-reach-normalize-cell-id` and `auto-reach-engine-name-scope` at 0; the module-SCC check green with `instance_scope` in no SCC.
- **β1 — `AutoReach`; admission and `W_UNDERDETERMINED` switch together (#9369)** (high; fk-a5a65ab8428c, fk-a5237570b374; absorbs #6307, #7069). `auto_reach.rs` (§5.3, §5.4), both construction points and retention (§5.5); the builder (cold, merged, edit), the detector, the connector pin writers, `build_dependent_cells`, `objective_auto_reach` and the undef-cause writer as queries (§5.6); `contains_optimized_call` after its OQ-1 fixture (D7); `UndefCause::ReachNotDrivable`; deletes `CellReadIndex`, `filter_constraints_reading_autos`, `auto_is_pinned_through_a_reader` and rewrites their three in-file test modules against public seams (Tests stance). First step, the #6307 port (D17): its e2e as `tests/harness_engine/connector_param_underdetermined_e2e.rs` reading the committed `auto_reach_conn_*.ri` fixtures by path (registered by α, D16), with the branch's `underdetermined_support.rs` helpers (`eval_through_reify_check`, the hoisted `assert_underdetermined_ids_exactly`) and the matching `instance_path_underdetermined_e2e.rs` edit; `harness_auto_resolution/auto_binding_sites_remaining_resolution.rs`'s `example_auto_binding_sites_ri_all_four_resolve` baseline goes from `["AllFourSites.__connector_0.gain"]` to empty; the stale path comment in `examples/auto_binding_sites.ri` is fixed. B2 lives in-file in `auto_reach.rs`; B3, B3b and B4 in a new `tests/harness_engine/auto_reach_admission.rs` through the engine API and a recording `ConstraintSolver`; `tests/underdetermined.rs`' `no_underdetermined_for_cross_scope_read_auto_param` is corrected to expect exactly one warning, for `Leaf.k` (C15). Adds the INV-EVAL-7 row (`partial`). Files: `auto_reach.rs`, `solver_problem.rs`, `detectors.rs`, `engine_eval.rs`, `engine_edit.rs`, `undef_tracer.rs`, `lib.rs`, `reify-ir/src/value.rs`, `reify-cli/src/main.rs`, `tests/underdetermined.rs`, `tests/harness_engine.rs`, `tests/harness_engine/auto_reach_admission.rs`, `tests/harness_engine/connector_param_underdetermined_e2e.rs`, `tests/harness_engine/underdetermined_support.rs`, `tests/harness_engine/instance_path_underdetermined_e2e.rs`, `tests/harness_auto_resolution/auto_binding_sites_remaining_resolution.rs`, `examples/auto_binding_sites.ri`, `docs/invariants.md`, `tests/infra/adoption-ratchet.manifest`. Prereqs: α, #6690, #8868. Signal: `reify check tests/prd-gate/fixtures/auto_reach_conn_pinned.ri` and `…/auto_reach_conn_concrete.ri` print no `W_UNDERDETERMINED` (`…/auto_reach_conn_free.ri` still prints two); `reify check …/auto_reach_cross_scope_child_auto.ri` prints `W_UNDERDETERMINED` for `Leaf.k` naming `P#constraint[0]` and keeps today's `INDETERMINATE P#constraint[0]` verdict; `reify eval …/auto_reach_geo_loop_a.ri` no longer prints `constraints could not be satisfied`.
- **β2 — the decline gate from the relation (#9370)** (high; fk-a5a65ab8428c; absorbs #6649). `constraints_reaching_auto` and the post-geometry decline set become `declined_constraints()` on the retained instance (AR-2, refusal class only); `realizations_reaching_auto` and `collect_constraint_realization_reads` deleted with the "Known limitation" paragraph; INV-EVAL-7 row extended. #6649's two pins are its acceptance rows: the cross-sub constraint is reported unresolved with its cause and never Violated; `W_UNDERDETERMINED` is correct for the ctor-arg auto. Its e2e reads `auto_reach_geo_loop_{a,b}.ri` by path (registered by α, D16). Files: `engine_fixpoint.rs`, `engine_build.rs`, `engine_constraints.rs`, `deps.rs`, `auto_reach.rs`, `tests/harness_engine/cross_sub_geometry_auto_refusal_e2e.rs`, `tests/harness_engine.rs`, `tests/harness_cache/unified_dag_boundary_cases.rs`, `tests/harness_cache/unified_dag_geometry_executors.rs`, `docs/invariants.md`, `tests/infra/adoption-ratchet.manifest`. Prereqs: β1. Signal: `reify check tests/prd-gate/fixtures/auto_reach_geo_loop_b.ri` prints `error: unresolved constraint: constraint GeoLoopB#constraint[0] transitively depends on auto parameter(s) through geometry-backed inputs` and no `VIOLATED`.
- **β3 — `compute_clusters` on the relation (#9371)** (high; fk-a5a65ab8428c, fk-a5237570b374). `union_via_transitive_auto_owners` and `build_non_auto_cell_map` replaced by `auto_owners_reached_by` over drivable objective reachers (cross-scope objective reachers are drivable, D13) through a `ClusterFormationCtx` field; lowers `auto-reach-extract-value-deps`. Behaviour-preserving: bt8 and the δ-cluster suites still form their clusters, bt9 still forms none (its path is `Undrivable(FrozenComputeCell)`), bt10 still forms none (constraint sources never seed). Files: `resolve_order.rs`, `engine_eval.rs`, `auto_reach.rs`, `tests/infra/adoption-ratchet.manifest`. Prereqs: β1. Intermediate (consumer γ). Signal: B7 — the resolve_order cluster suites unchanged, and `reify eval examples/whole_model_joint_drive.ri` / `examples/cost_subtree_aggregate.ri` byte-identical to before.
- **γ — cross-check e2e + the author-facing doc (#9372)** (high; fk-a5a65ab8428c). The integration gate (B8): one `harness_engine` module over the eighteen `tests/prd-gate/fixtures/auto_reach_*.ri` fixtures (registered by α), observing the built problems through a recording `ConstraintSolver`, asserting AR-1 ⇔ AR-3 and AR-2 ⇔ `E_EVAL_UNRESOLVED` on eval, and the eval column = the edit column for admission. Docs-truth gate: `crates/reify-mcp/src/tools/chunks/parameters.md` gains "Which constraints drive an `auto`" (drivable shapes, the blocked shapes and the diagnostic each produces), each claim checked against the fixtures; discoverability acceptance: an author asking "why is my auto not solved" or searching `W_UNDERDETERMINED` finds that section from the parameters chunk. No exemplar-corpus or reify-design index entry: the stream changes diagnostics, not authoring idioms (recorded waiver of overlay docs-truth items 2–3, reason stated). Flips INV-EVAL-7 to `enforced(type+test)`. Files: `tests/harness_engine/auto_reach_cross_check.rs`, `tests/harness_engine.rs`, `crates/reify-mcp/src/tools/chunks/parameters.md`, `docs/invariants.md`. Prereqs: β1, β2, β3. Signal: the module green on main and red against a seeded single-site regression during authoring; the chunk section present and accurate.
- **δ — [MILESTONE] decide: cross-scope constraints drive a child's auto (R3 "later via whole-model-objective-coupling") (#9374)** (medium; R9). A dep-gated decision gate, not an implementation leaf: `execution_class = "decision"` (a born-at-L2 escalation to Leo on dispatch, no agent). DO NOT IMPLEMENT. When γ lands, it brings Leo: (a) the read-binding problem — `auto_reach_cross_scope_child_auto.ri`'s constraint reads a graph-minted `P.l.k` node that nothing writes, so a merged solve needs read-rebinding to the canonical auto and a write-back (`auto_reach_tk_child_let.ri` reads `Leaf.k` template-keyed and needs neither); (b) clusters need an objective today (the objective-less fast path in `compute_clusters`), so an objective-less merged solve must be defined; (c) the fences to re-rule — resolve_order's `bt10_constraint_only_transitive_read_forms_no_cluster`, whole-model-objective-coupling INV-2/BT-10, whole-model-joint-drive-seam decision 7; (d) the single-instance restriction (with two or more instances a template-keyed auto cannot honour per-instance constraints); (e) the docs-truth obligations a new idiom would carry (exemplar + index line). On Leo's ruling, the implementation leaf is filed against this PRD (no `prd_task_label`), wired `ω ← it`, and δ closes. Files: none. Prereqs: γ. Signal: the escalation reaches Leo carrying (a)–(e) and the two fixtures' outputs at that day's main; δ closes on his ruling (an implementation leaf filed, or the question closed with its reason).
- **ω — PRD close (#9373)** (medium). Stamps this PRD and its `.capability-manifest.md` terminal (overlay "PRD terminal status": token, landed leaf ids, AS-AUTHORED paragraph, LIVE/AS-AUTHORED map — §5 and the INV-EVAL-7 row stay LIVE); verifies every register row of the manifest against a CLI run; confirms the `x_subsumed_by` stamps on #6307, #7069, #6649. Files: `docs/prds/v0_6/auto-reach-relation.md`, `docs/prds/v0_6/auto-reach-relation.capability-manifest.md`. Prereqs: α, β1, β2, β3, γ, δ (and the implementation leaf δ's ruling files, if any). Signal: the committed header.

**DAG:** (task ids in each row's title) #8691, #8704, #8860 → α; α, #6690, #8868 → β1; β1 → β2; β1 → β3; β1, β2, β3 → γ; γ → δ; all → ω. Gate #9171 ← β1, β2, β3, γ. Acyclic.

**Gate-test drift-guard registration (overlay rule).** New integration test files are modules of the existing `harness_engine` binary (no new `[[test]]` target, no nextest entry). No new `tests/infra` member: the ratchet rows go in the existing `adoption-ratchet.manifest`. No leaf asserts a wall-clock bound. α, the first compiled reader of the eighteen committed fixtures, registers all of them in `_RUST_COUPLED_RI_FIXTURES` in its own diff (D16; PG-DRIFT keeps the list honest).

## 10. Measurement record (dated — main `baff3ed38b`, 2026-10-17/18; debug binary built 2026-10-16 from about `6a9168e5d1`)

- Census: `crates/reify-eval/src/` walks and their pins as listed in §2; `CellReadIndex` 415 lines with doc; `auto_is_pinned_through_a_reader` 155 with doc; `union_via_transitive_auto_owners` 94; `realizations_reaching_auto` 67; `build_non_auto_cell_map` 23. Test modules exercising `CellReadIndex` directly: `cell_read_index_tests` (409 lines), `transitive_constraint_admission_tests` (379), `dependent_cells_admissibility_tests` (418).
- Fixture runs (`target/debug/reify check|eval`; diagnostics only, `W_MODULE_DECL_MISSING` absent because every fixture declares its module):
  - `auto_reach_geo_loop_a.ri` — check: `INDETERMINATE …`, `error: unresolved constraint: constraint GeoLoopA#constraint[0] transitively depends on auto parameter(s) through geometry-backed inputs`, exit 1. eval adds `error: constraints could not be satisfied (max absolute residual: 1.00e0)` and `note: GeoLoopA.w is undef (because: solve failed: infeasible)`.
  - `auto_reach_geo_loop_b.ri` — check: `VIOLATED GeoLoopB#constraint[0]`, `error: constraint GeoLoopB#constraint[0] violated`, exit 1; no refusal; no `W_UNDERDETERMINED`. eval adds `constraints could not be satisfied` and `GeoLoopB.leaf.w is undef (because: solve failed: infeasible)`.
  - `auto_reach_conn_pinned.ri`, `auto_reach_conn_concrete.ri` — `W_UNDERDETERMINED: auto parameter 'E.__connector_0.gain' …`, exit 0; eval prints `E.__connector_0.gain = 0.01 m`.
  - `auto_reach_conn_free.ri` — two `W_UNDERDETERMINED` (`ConnFree.gain`, `E.__connector_0.gain`), both undef "awaiting solve".
  - `auto_reach_cross_scope_child_auto.ri` — check: `INDETERMINATE P#constraint[0]: undefined inputs: P.l.k`, exit 0, no `W_UNDERDETERMINED`; eval: `note: Leaf.k is undef (because: Leaf.k awaiting solve)`.
  - `auto_reach_tk_child_let.ri` — same class: no `W_UNDERDETERMINED`; `Leaf.k`, `Leaf.d`, `P.l.d` undef "awaiting solve".
  - `auto_reach_child_side_let.ri` — `Holder.b.bore = 0.01 m`, no warning.
  - `auto_reach_two_siblings.ri` — two `W_UNDERDETERMINED` (`Sib.b1.bore`, `Sib.b2.bore`) with `(touching constraints: none)`, both undef.
  - `auto_reach_constraint_inside_child.ri` — `OK Bearing2#constraint[0]`; `W_UNDERDETERMINED` for `Own.b.bore` with `(touching constraints: none)`; undef.
  - The eight further #6307 shapes (`auto_reach_conn_{two_sites,let_defined,constrained_free_type,regular_sub_auto,no_default,free_connect_param,let_unpinned_root,nested}.ri`, extracted from `task/6307` tip `42795f734b`): the manifest's "#6307 verdict table" records each one's warnings and resolved value on main.
- #7069 re-verification: `connector_instance_auto_is_pinned_by_its_connector_type` was never on main (`git log -S` finds it only in d53c4a8025 and a09359f65f, both on `task/6307`, neither an ancestor of main); #6307 is pending with `task/6307` 9 commits ahead (tip 42795f734b, 2026-09-01).
- #6649 re-verification: its false Violated and missing refusal reproduce; its "wrong W_UNDERDETERMINED naming GeoLoopB.w" does not (no `W_UNDERDETERMINED` in either command).
- Dependents of #6307, #6649, #7069: none (paginated `get_tasks`, 9,301 examined = total).
- #8612's transitive closure recomputed live: 245 non-terminal tasks; #9171, #6690, #8868, #8877, #6649, #7069 outside it; #8691, #8704, #8860, #6689, #8863 inside (depended on only).

## 11. Open questions (tactical)

1. **OQ-1 — the `@optimized`-let shapes.** Before relying on R2's correction, β1 authors a fixture with a registered trampoline (`let c = optfn(a); constraint c < X`) and the wrapper `let c = 2.0 * optfn(a)`, runs them through `reify eval`, and records the observed doomed solve. If the wrapper never reaches the fold in practice, β1 keeps the bare-call predicate and says why in its commit message. Decide during β1.
2. **OQ-2 — collection-sub autos.** Whether the compiler mints an auto cell for a collection sub's named argument is unverified (no `.ri` shape found). If it does, §5.4's rules apply (direct reads drivable; child-side lets `Undrivable(MultiInstanceAlias)`); if not, the rows are vacuous today and γ says so. Decide during α (it owns `ScopeMap`'s collection handling).
3. **OQ-3 — message grammar.** Exact text of `W_UNDERDETERMINED`'s reacher list (`touching constraints: P#constraint[0] — does not drive it: cross-scope constraint`), keeping the existing prefix so `contains(...)` pins keep matching; and the block-specific `E_EVAL_UNRESOLVED` text for `FrozenComputeCell` (same code, §5.10). Decide during β1.
4. **OQ-4 — tactical containers inside `AutoReach`** (per-auto slices, per-source transpose). Decide during β1.

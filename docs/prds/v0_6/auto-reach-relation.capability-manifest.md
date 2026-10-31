# Capability manifest — auto-reach-relation

PRD: `docs/prds/v0_6/auto-reach-relation.md`. Machine-readable twin: `docs/prds/v0_6/auto-reach-relation.capability-manifest.yaml` (task ids stamped by `commit_planning` on 2026-10-18: α #9368, β1 #9369, β2 #9370, β3 #9371, γ #9372, δ #9374, ω #9373). Evidence measured at main `baff3ed38b` (2026-10-17/18) with the debug binary built 2026-10-16 (about `6a9168e5d1`). Cite-by-symbol; any line number below is part of this dated record.

**D3 decompose-verify disposition** (`scripts/prd-decompose-verify.mjs`, run 2026-10-18 as a scratch copy with `REIFY_BIN` pinned to the 2026-10-16 debug build — the default `target/release/reify` dates from 2026-09-01 — and sonnet adversaries; 16 agents, 1.08M tokens). Leaves passed: α, β1, β2, β3, each with its today-state premises; γ (a test module and a doc chunk), δ (a decision gate) and ω (a docs stamp) are outside the probe vectors' reach and were not passed. Results: α VERIFIED (4/4 probes executed), β3 VERIFIED (3/3). β1 and β2 returned BLOCKS on harness mis-bindings only: their probes bound `W_UNDERDETERMINED` and `error:` lines to stdout although reify prints diagnostics on stderr and verdict lines (`VIOLATED …`, `INDETERMINATE …`) on stdout, and one probe passed a list where the `stdout_contains` match key takes a string (a `TypeError` in `prd-capability-check.py::match_predicate`). The captured outputs in those very records confirm every premise (the connector warnings on stderr; `VIOLATED GeoLoopB#constraint[0]` and no refusal on `auto_reach_geo_loop_b.ri`). Nothing was falsified. Note for the leaves' own tests and probes: bind diagnostics to stderr and verdict lines to stdout.

Every leaf's G3 premises bind to a filed upstream task (DAG-direction: producer upstream) or to a symbol on main. No leaf asserts a numeric bound (G6 branches 1–2 not engaged). Rejection premises (G6 branch 4) are bound to mechanisms observed firing on main.

## Per-leaf bindings

### α — `ScopeMap` + `DependencyGraph::template_reads`
| Capability | Evidence | Verdict |
|---|---|---|
| `DependencyGraph` with `forward` / `reverse()` exists to hang `template_reads` on | producer: S3 δ #8691 (pending), upstream of α | PASS (producer upstream) |
| `InstancePath::structure_scope()` for the canonical key | producer: S3 ι #8704 (pending), upstream of α | PASS (producer upstream) |
| `CellReadIndex`, `build_single_instance_alias_paths` and the pinned-connector writers live in `solver_problem.rs` | producer: S4 ν #8860 (pending), upstream of α | PASS (producer upstream) |
| the three bridge sites, `build_dependent_cells`' rescoping and `resolve_order`'s fallback exist to repoint | `CellReadIndex::read_closure` push, `cells_reaching` keying and seeding; `build_dependent_cells` trace normalisation; `union_via_transitive_auto_owners` as-is-then-normalised (read at `baff3ed38b`) | PASS |
| behaviour-preserving oracle | `auto_reach_two_siblings.ri` (two `W_UNDERDETERMINED`) and `auto_reach_child_side_let.ri` (`Holder.b.bore = 0.01 m`) observed on main; the `cell_read_index_tests`, `instance_path_underdetermined_e2e` and resolve_order cluster suites exist | PASS |
| fixture registration vehicle (α is the first compiled reader of all eighteen, D16) | `_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`; PG-DRIFT in `tests/infra/test_verify_scope.sh` | PASS |
| delivered | `crates/reify-eval/src/instance_scope.rs` exists; `fn template_reads` in `deps.rs`; `auto_reach_geo_loop_b` registered in `scripts/verify.sh` | check (yaml) |

### β1 — `AutoReach`; admission and `W_UNDERDETERMINED` switch together
| Capability | Evidence | Verdict |
|---|---|---|
| one engine-side builder to take `&AutoReach` | producer: solver-driver-parity β #6690 (pending, high), upstream of β1 | PASS (producer upstream) |
| `detect_underdetermined` as a `PostPassDetector`, `ColdArtifacts` on `PostPassState` | producer: S4 κ #8868 (pending), upstream of β1 (κ ← φ #9292) | PASS (producer upstream) |
| `EvaluationState` to retain `auto_reach` + `scope_map` | `lib.rs::EvaluationState` (exists) | PASS |
| connector pin mechanism the delegation arm mirrors | `connector_pin_if_determined`, `is_strict_connector_instance_auto` (engine_eval.rs, exist); `auto_reach_conn_pinned.ri` / `auto_reach_conn_concrete.ri` print `E.__connector_0.gain = 0.01 m` | PASS |
| rejection premise: `W_UNDERDETERMINED` fires (the conn-free guard keeps two) | `auto_reach_conn_free.ri`: two `W_UNDERDETERMINED` observed | PASS |
| false-positive premise: the connector warning exists to remove | `auto_reach_conn_pinned.ri`, `auto_reach_conn_concrete.ri`: `W_UNDERDETERMINED` for `E.__connector_0.gain` observed | PASS |
| silent cross-scope premise | `auto_reach_cross_scope_child_auto.ri`: no `W_UNDERDETERMINED`, `Leaf.k … awaiting solve` observed | PASS |
| doomed-solve premise | `auto_reach_geo_loop_a.ri` eval: `constraints could not be satisfied (max absolute residual: 1.00e0)` observed | PASS |
| `@optimized`-let doomed solve on the cold path | not yet observed (no fixture with a registered trampoline run); β1's first step (PRD §11 OQ-1) | OPEN (decided in β1) |
| `UndefCause` to extend | `reify-ir` `value.rs::UndefCause`; CLI `format_undef_cause` exhaustive | PASS |
| #6307's port files exist on the branch | `task/6307`: `connector_param_underdetermined_e2e.rs`, `underdetermined_support.rs`, `instance_path_underdetermined_e2e.rs`, `auto_binding_sites_remaining_resolution.rs`, `examples/auto_binding_sites.ri` (`git diff --stat main...task/6307`) | PASS |
| #6307's acceptance content survives the branch | all eleven #6307 sources committed as `tests/prd-gate/fixtures/auto_reach_conn_*.ri`; full test file at `task/6307` tip `42795f734b`, `crates/reify-eval/tests/harness_engine/connector_param_underdetermined_e2e.rs` | PASS |
| delivered | `crates/reify-eval/src/auto_reach.rs` exists; `INV-EVAL-7` row in `docs/invariants.md`; `ReachNotDrivable` in `reify-ir/src/value.rs` | check (yaml) |

### β2 — the decline gate from the relation
| Capability | Evidence | Verdict |
|---|---|---|
| the relation and `EvaluationState.auto_reach` | producer: β1, upstream | PASS (producer upstream) |
| decline consumers exist | `engine_fixpoint::constraints_reaching_auto` / `unresolved_diagnostics`; `Engine::check_constraints_post_geometry` decline set; `engine_build` post-geometry re-check | PASS |
| rejection premise: `E_EVAL_UNRESOLVED` fires | `auto_reach_geo_loop_a.ri` check: `error: unresolved constraint: … through geometry-backed inputs` observed | PASS |
| false-Violated premise (INV-SF-4) | `auto_reach_geo_loop_b.ri` check: `VIOLATED GeoLoopB#constraint[0]`, exit 1 observed | PASS |
| delivered | `declined_constraints(` called in `engine_fixpoint.rs` | check (yaml) |

### β3 — `compute_clusters` on the relation
| Capability | Evidence | Verdict |
|---|---|---|
| the relation | producer: β1, upstream | PASS (producer upstream) |
| `ClusterFormationCtx` to carry it | `resolve_order.rs::ClusterFormationCtx` (exists) | PASS |
| behaviour-preserving oracle | bt8/bt9/bt10/δ-cluster unit suites in `resolve_order.rs`; `examples/whole_model_joint_drive.ri`, `examples/cost_subtree_aggregate.ri` evaluate (exit 0) on main | PASS |
| delivered | `auto_owners_reached_by` in `resolve_order.rs` | check (yaml) |

### γ — cross-check e2e + the author-facing doc
| Capability | Evidence | Verdict |
|---|---|---|
| all three consumers on the relation | producers: β1, β2, β3, upstream | PASS (producer upstream) |
| fixtures | the eighteen `tests/prd-gate/fixtures/auto_reach_*.ri` (committed with this PRD; each parses with 0 ERROR nodes, isolated cache, negative control rejected; registered by α) | PASS |
| doc chunk to extend | `crates/reify-mcp/src/tools/chunks/parameters.md` (exists; no section on which constraints drive an auto) | PASS |
| delivered | `crates/reify-eval/tests/harness_engine/auto_reach_cross_check.rs` exists; the chunk section; INV-EVAL-7 `enforced` | check (yaml) |

### δ — [MILESTONE] decide: cross-scope constraints drive a child's auto (R3, R9)
A decision gate (`execution_class = "decision"`), not an implementation leaf: no capability is asserted, no delivered check gates ω on it. Its escalation carries:
| Question | Evidence | Verdict |
|---|---|---|
| read-binding for an instance-path read of a template-keyed auto (`P.l.k`) | `graph.rs` plain-sub elaboration mints `P.l.k` (Auto kind, no `default_expr`); `auto_reach_cross_scope_child_auto.ri` check: `INDETERMINATE P#constraint[0]: undefined inputs: P.l.k` | OPEN (Leo's ruling) |
| objective-less merged solve | `compute_clusters`' objective-less fast path (clusters are seeded only when some template has an objective); `build_merged_solver_problem`'s objective fold | OPEN (Leo's ruling) |
| the fences | `bt10_constraint_only_transitive_read_forms_no_cluster` (resolve_order.rs); whole-model-objective-coupling INV-2/BT-10; whole-model-joint-drive-seam decision 7 | OPEN (Leo's ruling) |
| single-instance restriction; docs-truth for a new idiom | `ScopeMap::single_instance`; overlay docs-truth items 2–4 | OPEN (Leo's ruling) |

### ω — PRD close
| Capability | Evidence | Verdict |
|---|---|---|
| every other leaf landed or cancelled | producers: α, β1, β2, β3, γ, δ, upstream | PASS (producer upstream) |
| delivered | `**Status:** **SHIPPED` in the PRD header | check (yaml) |

## Behaviour-change register (corrections; R1–R4)

Each row: fixture or shape → today (observed, §10 of the PRD) → after → class → the pin it flips (if any) → owning leaf. "Correction" rows cite the invariant or task the old answer breaks.

| # | Shape | Today | After | Class | Pin today | Leaf |
|---|---|---|---|---|---|---|
| C1 | `auto_reach_geo_loop_b.ri` (cross-sub geometry) | check: `VIOLATED`, exit 1, no refusal; eval: `constraints could not be satisfied`, undef "solve failed: infeasible" | `E_EVAL_UNRESOLVED` (unresolved with its cause, never Violated); no solve; `W_UNDERDETERMINED` for `GeoLoopB.leaf.w` naming the block | correction (INV-SF-4; #6649) | none | β1 (admission, W), β2 (gate) |
| C2 | `auto_reach_geo_loop_a.ri` (own-scope geometry) | refusal + doomed solve: `constraints could not be satisfied`, undef "solve failed: infeasible" | refusal only; undef cause `ReachNotDrivable`; `W_UNDERDETERMINED` naming the block | correction (INV-SF-1 misattribution) + change (R1: new W) | none | β1 |
| C3 | `auto_reach_conn_pinned.ri`, `auto_reach_conn_concrete.ri`, `auto_reach_conn_two_sites.ri`, `auto_reach_conn_let_defined.ri`, `auto_reach_conn_constrained_free_type.ri` | `W_UNDERDETERMINED` on each `__connector_N.gain` although it resolves | no warning | correction (#6307; #7069) | `example_auto_binding_sites_ri_all_four_resolve`'s `assert_underdetermined_ids_exactly` baseline `["AllFourSites.__connector_0.gain"]` → empty (harness_auto_resolution/auto_binding_sites_remaining_resolution.rs) — flips | β1 |
| C4 | `auto_reach_cross_scope_child_auto.ri`, `auto_reach_tk_child_let.ri` | nothing solves `Leaf.k`; no `W_UNDERDETERMINED`; undef "awaiting solve" | `W_UNDERDETERMINED` for `Leaf.k` naming `P#constraint[0]` (cross-scope, non-driving); the constraint's verdict unchanged (`INDETERMINATE`); whether it later drives the auto is the δ milestone's question | correction (INV-SF-1, INV-SF-3) | — (see C15 for the test that flips) | β1 |
| C5 | `auto_reach_two_siblings.ri`, `auto_reach_constraint_inside_child.ri` | `W_UNDERDETERMINED … (touching constraints: none)` — false for `Sib.b1.bore` and `Own.b.bore` | same verdict; the message names the touching constraint and its block | correction (heuristic 1; message honesty) | `two_sibling_instances_sharing_a_child_side_let_are_flagged_independently`, `an_auto_pinned_only_by_a_constraint_inside_its_declaring_child_is_flagged` — `contains(...)`, do not flip | β1 |
| C6 | `auto_reach_conn_let_unpinned_root.ri` | `W_UNDERDETERMINED` on `E.__connector_0.gain` and `ConnLet2.base` (the #6307 branch would have unflagged the first via its static proxy) | both warnings stay; the instance's names its undetermined delegate | correction vs the branch's documented limit; unchanged vs main | branch guard (ported) | β1 |
| C7 | `auto_reach_conn_nested.ri` | three `W_UNDERDETERMINED` (`ConnOuter.gain`, `ConnOuter.__connector_0.gain`, `E.__connector_0.gain`) | `ConnOuter.__connector_0.gain`'s warning goes (its delegate `ConnInner.gain` is pinned); the other two stay (true) | correction (#6307) | branch guard (ported) | β1 |
| C8 | let chain or instance-path spelling on the edit path | not admitted on `edit_param` (direct raw reads only) | admitted, as on cold eval | correction (SDP §2.3 / B9), jointly with #6690 | none | β1 |
| C9 | parent constraint on a template-keyed child auto, edit path | admitted into the child's group by raw name match | not admitted (same as cold; R3 — whether it later drives the auto is the δ milestone's question) | change (R3) | none | β1 |
| C10 | `let c = optfn(a); constraint c < X` (cold) | admitted with `c` frozen (doomed solve) | declined: `E_EVAL_UNRESOLVED` (same code, message names the frozen cell `c`); `W_UNDERDETERMINED` names it | change (R2), premise proven in β1 first | none | β1 |
| C11 | `let c = 2.0 * optfn(a)` wrapper | refolded through registry-less `eval_expr` by `build_dependent_cells` and the solver | treated as frozen (containment predicate) | correction if reachable (β1 decides, PRD §11 OQ-1) | `dependent_cells_excludes_optimized_userfunctioncall_cell` — unchanged | β1 |
| C12 | realization reads a `let` that reads an auto (W4's documented blind spot) | no `E_EVAL_UNRESOLVED`; the let frozen during the solve | declined + `E_EVAL_UNRESOLVED`; the "Known limitation" paragraph deleted | change (one relation) | none | β2 |
| C13 | objective-only reach through a bare `@optimized` let | silent; no cluster (bt9) | `W_UNDERDETERMINED` names the frozen-compute block; still no cluster | change (R1 + R2; consistent with joint-drive-seam D5) | `bt9_optimized_line_cost_forms_no_cluster` — unchanged | β1 |
| C15 | `Later.y > Leaf.k` (no sub relation; `Later` owns auto `y`) | admitted for `Later.y`; no warning for `Leaf.k` (W2's global closure clears it); nothing solves `Leaf.k` | still admitted for `Later.y` (drivable Own reacher; the CrossScope reacher is non-driving and does not decline); `W_UNDERDETERMINED` for `Leaf.k` only | correction (R3; INV-SF-1, INV-SF-3) | `no_underdetermined_for_cross_scope_read_auto_param` (tests/underdetermined.rs) asserts no warning — flips to exactly one, for `Leaf.k` | β1 |
| C16 | parent constraint on a child auto inside an objective-formed merged cluster | admitted (merged builder admits members' constraints against the cluster-wide autos) | admitted (CrossScope reacher lifted when co-solved, D13) | unchanged | none located for this exact shape; β1 adds one in B3b (`tests/harness_engine/auto_reach_admission.rs`) | β1 |
| C14 | `E_EVAL_UNRESOLVED` surface | computed on `Engine::build`'s unified pass only | also computable at eval from the retained relation | change (widening of the surface; message text unchanged) | the three `unified_dag_*` / `unified_pass_*` pins — unchanged | β2 |

Unchanged shapes (pinned, must stay; bt10 stays a no-cluster fence until the δ milestone's ruling): `auto_reach_child_side_let.ri` (`Holder.b.bore = 0.01 m`, no warning); `auto_reach_conn_free.ri` (two warnings); `auto_reach_conn_regular_sub_auto.ri` (`P.bolt.length` warned, undef); `auto_reach_conn_no_default.ri` (warned, undef); `auto_reach_conn_free_connect_param.ri` (no warning: `auto(free)` is never flagged); objective-only reach (`no_underdetermined_for_objective_only_auto_param`); cluster formation (bt8, bt9, bt10, δ-cluster suites).

## #6307 verdict table, carried (R4)

The eleven #6307 sources (task/6307 tip `42795f734b`, `connector_param_underdetermined_e2e.rs`) as committed fixtures, main today vs the relation:
| Fixture | #6307 role | main (2026-10-18) | After β1 |
|---|---|---|---|
| `auto_reach_conn_pinned.ri` | reproducer (own constraint) | W on instance; resolves 10 mm | no W; resolves |
| `auto_reach_conn_two_sites.ri` | reproducer (two connect sites) | W on both instances; both resolve | no W; both resolve |
| `auto_reach_conn_concrete.ri` | reproducer (concrete default) | W; resolves 10 mm | no W; resolves |
| `auto_reach_conn_let_defined.ri` | reproducer (let-defined) | W; resolves 8 mm | no W; resolves |
| `auto_reach_conn_free.ri` | guard (unconstrained type) | two W | two W |
| `auto_reach_conn_regular_sub_auto.ri` | guard (regular sub auto) | W; undef | W; undef |
| `auto_reach_conn_no_default.ri` | guard (no defining expression) | W; undef | W (`touching constraints: none`) |
| `auto_reach_conn_free_connect_param.ri` | guard (`auto(free)` connect param) | no W | no W |
| `auto_reach_conn_constrained_free_type.ri` | guard (free type cell pinned by a constraint) | W; resolves 10 mm | no W; resolves |
| `auto_reach_conn_let_unpinned_root.ri` | limit (branch: unflagged by static proxy) | W on instance and `ConnLet2.base` | both W; instance names its undetermined delegate |
| `auto_reach_conn_nested.ri` | limit (branch: nested keeps its warning) | three W | `ConnOuter.__connector_0.gain` unflagged; two W stay |

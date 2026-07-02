# PRD: Uniform dependency handling in the eval engine — no-stale-Undef invariant + value-cell graph completion

**Milestone:** v0_6 · **Status:** active (ratified 2026-07-02, design session with Leo) · **Shape:** B+H (contract + boundary tests)
**Supersedes (mechanism, not tasks):** the R3d/R3e/R3f compensation stack in `docs/prds/v0_6/value-eval-geometry-addressing.md` Rung 3 (tasks #4900/#4907/#4946 — all remain done/landed as history; this PRD deletes the *code* they added where it becomes redundant).
**Successor (deferred):** `docs/prds/v0_6/eval-unified-schedule-executor.md` (stage (b) stub + [MILESTONE] task).
Code citations are at HEAD `85b6b88e07`; prefer the stable function names over line numbers.

## 1. Goal

Make symbolic geometry/selector resolution in pure eval correct **by construction** instead of by compensation:

- **(c)** a **no-stale-Undef invariant** — a checker that makes the silent-Undef failure class loud in any debug test, instead of surfacing three tasks later in one release-gated e2e;
- **(a)** **value-cell graph completion** — geometry `let`s get first-class value cells (internal-only; no language-surface change), so the eval walk's topological order sees the true dependency edges and the in-walk mint covers every case uniformly.

End state: the R3e consumer re-eval sweeps, the R3f post-walk-mint re-eval hook, and both whole-module post-walk mint passes are **deleted**; the full regression set (R3E trio, R3f let-backed member, printer capstone `printer_print_envelope_eval_e2e`) stays green without them; the invariant is green across the debug gate.

## 2. Background — the compensation stack and its root cause

Eval accreted three compensating mechanisms for symbolic geometry/selector resolution timing, each discovered as a separate production miss on the same release-gated test (task #4655's capstone):

- **R3d (#4900, done):** dependency-ordered in-walk mint — Param arm and Let arm of `evaluate_params_and_lets_unified` (`engine_eval.rs:6121-6169`, `:6551-6579`) mint symbolic handles/selectors at cell-visit time.
- **R3e (#4907, done):** same-pass consumer re-eval after in-walk mint — `re_eval_consumers_of_in_walk_mints` (`engine_eval.rs:6662-6747`), graph twin `..._from_graph` (`:6759-6837`), cache-ful write-back via `reeval_cone_cell` (`:4666-4695`), keyed on the `minted_in_walk` trigger set, wired at three call-sites (eval `:6620`, eval_cached `:5299`, `engine_edit.rs:1063`), with an **@optimized exclusion** (`:6723-6729`) because `reeval_cone_cell` evaluates via plain `eval_expr` with no compute-dispatch registry.
- **R3f (#4946, pending — the bridge; lands before this PRD's batch):** the post-walk whole-module mints (`mint_symbolic_geometry_handles_into_values`, `engine_build.rs:8499-8544`; `mint_symbolic_topology_selectors_into_values`, `geometry_ops.rs:5133-5165`) return their flipped cell set and feed the same re-eval machinery.

**Root cause (proven, file:line):** the walk-ordering graph is incomplete. Geometry `let`s lower to a `RealizationDecl` with **no value cell** (`entity.rs` Let arm: `if is_geometry_let(...) { continue; }`, ~:1991-1995; scope-registration comment ~:1325). `build_combined_param_let_graph` (`engine_eval.rs:434-545`) nodes only param/let value cells, and Kahn's in-degree count in `topological_sort` (`dirty.rs:189`) **silently ignores edges whose target is not a node**. So for `let loc = faces_by_normal(loc_box, …)` over geometry-let `loc_box`, the edge `loc → loc_box` does not exist in the ordering; consumers can schedule before the mint; strict, diagnostic-free Undef propagation (`reify-expr/src/lib.rs:285-288`, `:1590-1592`) turns the miss into a silent wrong answer.

**The writer inventory (2026-07-02 deep dive, three parallel agent streams):** of every place a cell value is written outside the main walk (solver write-backs, guard passes, edit-path waves, compute results…), the two post-walk mints are the **only** writers that feed no consumer re-eval. Everything else already re-evaluates its cone.

**Latent misses the stack cannot close:**
- **Miss #4 (@optimized consumer):** the R3e @optimized exclusion means the sweep can never rescue an @optimized consumer (e.g. `displacement_at(result, let_backed_loc)`) that read a stale Undef. Same silent failure mode, waiting for someone to write that shape.
- **Miss #5 (edit_param):** `edit_param` has **no post-walk mint backstop at all** — a comment at `engine_edit.rs:964-966` claims an "idempotent post-eval backstop mint pass" that does not exist in the body at HEAD.

**Key structural facts that shape the fix:**
1. **The dependency information already exists post-eval.** `EvaluationState { snapshot, reverse_index, trace_map }` (`lib.rs:385-392`) retains per-cell `DependencyTrace { reads, realization_reads }` (`deps.rs:23-32`) — *including* the geometry-cell→realization edges the ordering graph drops. That makes (c) feasible from retained state.
2. **The double-representation precedent already ships.** A solid-typed param with a geometry default lowers to BOTH a `Type::Geometry` value cell (`entity.rs:1902-1988`) AND a `RealizationDecl`, linked by `RealizationNodeData.geometry_cell` (`graph.rs:420-426`), coherent under the mint's refuse-to-clobber guard (`engine_build.rs:8435-8440`) and GHR-β identity (`Value::GeometryHandle` / `GeometryHandleRef.kernel_handle: Option<GeometryHandleId>`, `reify-ir/src/value.rs`; `PartialEq`/`content_hash` exclude `kernel_handle`, so symbolic ≡ realized — landed by R2a #4652). This is exactly why #4907's param-backed fixture passes while the let-backed printer scenario fails: the *only* structural difference is the missing value cell.
3. **Selector lets already get value cells** (`entity.rs:1331-1341`, `:2150-2159`). Geometry lets are the one holdout.

## 3. Ratified direction (design session 2026-07-02)

Staged **(c) → (a) → (b)**, with (b) deferred:

- (c) and (a) are authored and decomposed **here**.
- (b) — eval consumes the unified-DAG schedule (`run_unified_pass`, `engine_fixpoint.rs`) via an eval-side symbolic-mint executor, retiring `build_combined_param_let_graph` and the eval/eval_cached walk duplication — is the ratified **endgame**, captured as a forward-stub PRD (`eval-unified-schedule-executor.md`) plus a [MILESTONE] task. Rationale for deferral: (a) rewrites the substrate (b) would be designed against; (b) must sequence against unified-dag Stage-5 (#4727); (b) deserves its own G5 design pass at flip time.
- Constraint (a) inherits from (b): **do not deepen investment in `build_combined_param_let_graph` / the walk scheduler** beyond what (a) needs — (b) deletes it.

**Geometry-let value cells stay internal** (ratified): no dot-access change, no export/doc surface change, no new language semantics. The cells exist for ordering and uniformity. Geometry-valued cells in snapshots are already precedented by solid params, so no new user-visible snapshot shape *class* is introduced.

## 4. Sketch of approach

### Stage (c) — no-stale-Undef invariant (task α)

A checker `check_no_stale_undef` in `reify-eval` (module placement tactical): given post-eval retained state (values + `trace_map` + graph), report every **violation**: a non-auto value cell with a `default_expr`, currently `Undef`, whose static deps (trace `reads` ∪ `realization_reads`) are **all resolved** (present and non-Undef), and which is not excluded. Exclusions (§6 contract): @optimized cells, auto cells, guard-inactive members, missing-producer reads. Runs strictly **post-everything** (after solver, after mints — i.e. against the state `eval`/`eval_cached` is about to return).

Shipped as a **test-harness invariant** first: a debug-gate suite runs it across the eval fixture corpus + examples; a **seeded-violation self-test** proves the checker actually fires (anti-silent-accept for the checker itself). Promotion to an always-on `debug_assert` is deferred (tactical, §11).

### Stage (a) — graph completion (tasks β, γ, δ, ε)

- **β (defensive pre-fix, lands before the lowering flip):** the #4726 @optimized `value_inputs` filter (`engine_eval.rs:6315-6338`) keys on graph-presence (`snapshot.graph.value_cells.contains_key`). Once geometry lets have cells, that filter would classify them as `value_inputs` and `compute_cache_key` (`compute_cache_key.rs:96-118`) would double-count them (geometry flows via `realization_inputs`). Flip the filter to a `cell_type != Geometry` exclusion. Safe standalone: behavior-neutral while geometry lets have no cells.
- **γ (the lowering flip):** `entity.rs` Let arm emits a `ValueCellDecl` for geometry lets (kind `Let`, `Type::Geometry`, `default_expr` = the geometry ctor expr) **alongside** the existing `RealizationDecl` — the exact shape solid-geometry params already have. The `geometry_cell` link fires by the existing name-match. Downstream: `build_combined_param_let_graph` and the trace map now see the node and the true edges; the topo walk orders producers before consumers; the existing R3d in-walk mint arms stamp the cell at its topo slot. Verify: no double-hydrate on the build path (`HydrateCell` steps, `engine_build.rs:5559-5620`), snapshot export sanity, differential corpus (task 4359 suites) green. **No feature flag** — the batch is dependency-sequenced, the full `--scope all --profile both` gate guards the flip, and the invariant (α) is the regression net; revert is the fallback.
- **δ (the deletions + docs, capstone):** delete `re_eval_consumers_of_in_walk_mints` + `..._from_graph` + their three call-sites + the `minted_in_walk` bookkeeping + the @optimized exclusion (dead with the sweeps); delete the R3f post-walk-mint→re-eval hook (#4946's addition); delete both whole-module post-walk mints and their six call-sites (eval `:4478/:4491`, eval_cached `:5591/:5601`, edit_source `engine_edit.rs:3695/:3705`). **Keep:** the R3d in-walk mint arms (they ARE the uniform mint mechanism now) and `reeval_cone_cell` (still consumed by the guarded-member downstream cone, `engine_eval.rs:4061`). Bundled docs: §9 addendum to `docs/design/symbolic-eval-nested-selector-resolution.md` + supersession note in `value-eval-geometry-addressing.md` §8.
- **ε (edit-path closure):** extend the invariant to the edit path (graph-native, mirroring the `_from_graph` pattern — the edit path has no `TopologyTemplate`); add an edit-path regression for the let-backed scenario (edit a param upstream of a geometry let consumed by a selector consumer; assert re-resolution); remove/correct the stale backstop comment at `engine_edit.rs:964-966`. Miss #5 is closed structurally by γ (geometry-let cells ride the dirty cone and mint in-walk at `engine_edit.rs:969`); ε proves it.

**Miss #4 closure:** with true ordering, an @optimized consumer runs *after* the mint on the main walk and goes through the live compute dispatch (`engine_eval.rs:6281-6501`) with a resolved selector. No re-eval-path dispatch is needed — the exclusion is deleted along with the sweeps.

**Cache/migration note:** new value cells change snapshot shape and cache identity once (a migration event, not an incompatibility). GHR-β keeps handle *values* stable; β keeps compute keys stable. The differential corpus and full gate guard the flip.

## 5. Pre-conditions for activating

| Prerequisite | Why | State at authoring |
|---|---|---|
| #4946 (R3f bridge) landed | α's invariant must be green over the corpus; the R3f class must be closed at α time. Also serializes engine_eval.rs churn. | pending, high priority — **no edge in this PRD may block it** |
| #4655 (R3c printer capstone) landed | δ's signal cites `printer_print_envelope_eval_e2e` on main | pending, blocked on #4946 — **no edge in this PRD may block it** |
| R2a #4652 (`kernel_handle: Option`) | symbolic handle representation | done (merge `f944fdc48a`), verified at HEAD |
| Solid-param cell+realization precedent | γ's lowering shape | on main, verified at HEAD |

All edges from this batch point **at** #4946/#4655 (this batch depends on them), never the reverse.

## 6. Contract (B+H)

### 6.1 No-stale-Undef invariant semantics

**Violation** ≔ a value cell `c` such that ALL of:
1. `c` is not an auto cell (`is_auto_cell`, `graph.rs:769`);
2. `c` has a `default_expr`;
3. `values[c]` is `Undef` (or absent) in the final state;
4. every static dep `d ∈ trace(c).reads ∪ trace(c).realization_reads` is **resolved**: present in the final values/realization state and non-Undef. A dep that is itself Undef, or a read whose producer is absent from the graph entirely (missing-producer), makes `c` **exempt**, not violating;
5. `c` is not excluded: NOT an @optimized `UserFunctionCall` (`find_matching_compiled_function(...).optimized_target.is_some()` — the same predicate R3e uses today), NOT a guard-inactive member.

**Timing:** the checker runs against the state the entry point is about to return — strictly after solver write-backs and (until δ lands) after the post-walk mints. It is read-only.

**Checker self-test obligation:** the suite must include a seeded violation proving the checker fires (a fabricated stale-Undef state → non-empty violation report). A checker that cannot be observed to fire is itself a silent-accept.

**Lifecycle:** α ships it for `eval`/`eval_cached`; ε extends it graph-natively to `edit_param`/`edit_source`. After δ, the invariant is the standing guard that replaces the deleted compensations' implicit coverage.

### 6.2 Cell/realization coherence contract (the double representation)

For every named geometry let (and, as today, solid-typed geometry param):
1. **Two artifacts, one name:** a `Type::Geometry` value cell AND a `RealizationDecl`, linked by `geometry_cell` (name-match, `graph.rs:420-426`).
2. **Value-cell content:** symbolic handle (`kernel_handle: None`) when minted by eval; realized handle (`Some`) when the build path hydrates. The mint **never clobbers** a realized handle (existing guard, `engine_build.rs:8435-8440` / `:8476-8482`).
3. **Identity:** GHR-β — `PartialEq`/`content_hash` exclude `kernel_handle`; symbolic and realized handles for the same realization + upstream state compare equal. Cache keys must not distinguish them.
4. **Compute-input partition:** a geometry-typed cell is NEVER a compute `value_input`; geometry flows via `realization_inputs` (`build_compute_realization_inputs`). Enforced by β's `cell_type`-based filter — graph-presence is no longer a valid proxy.
5. **Edit invalidation:** a geometry cell whose backing realization vanished must not survive an edit (existing mechanism `engine_edit.rs:2715-2739`, extended to let-backed cells in γ).
6. **Ordering:** the value-cell graph must contain a node for every named geometry producer, so every `consumer → producer` read is a counted edge in `topological_sort`. (This is the clause the pre-γ engine violates.)

## 7. Boundary-test sketch (B+H)

| # | Facing | Scenario | Preconditions | Postconditions |
|---|---|---|---|---|
| 1 | compiler→graph | geometry let lowers to the pair | `let b = box(...)` in a structure-def template | `value_cells` contains `b` (Type Geometry, kind Let, with `default_expr`); a `RealizationDecl` named `b` exists; `geometry_cell` links them |
| 2 | compiler→walk | true edge counted | `let s = faces_by_normal(b, +Z, 1deg)` over geometry-let `b` | ordering graph has node `b`; `s`'s in-degree counts `s → b`; topo places `b` before `s` |
| 3 | eval | first-pass resolution, no sweeps | the R3f let-backed fixture (geometry-let-backed selector target + consumer) | consumer sees the resolved selector on the FIRST pass; passes with R3e/R3f machinery deleted |
| 4 | eval (@optimized, miss #4) | `displacement_at(result, let_backed_loc)`-shaped @optimized consumer | let-backed selector feeding an @optimized call | consumer dispatches through the compute registry with a resolved selector; result non-Undef |
| 5 | edit (miss #5) | incremental re-mint | GUI-path `edit_param` on a param upstream of a geometry let with a selector consumer | consumer re-resolves within the dirty cone; edit-path invariant green |
| 6 | invariant | seeded violation | fabricated stale-Undef state | checker reports the violation (self-test; anti-silent-accept) |
| 7 | cache | identity stability across the flip | same module through `eval_cached` before/after the batch | handle values compare equal (GHR-β); compute cache keys exclude geometry-typed `value_inputs` |

Rows 3+4 constitute δ's observable gate; row 5 is ε's; row 6 is α's; rows 1/2/7 are γ's differential checks.

## 8. Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `value-eval-geometry-addressing.md` | supersedes its Rung-3 compensation *code* (R3d mint arms survive; R3e/R3f machinery deleted); consumes its R3c capstone as witness | the deletions in δ + §8 supersession note in that PRD | **this PRD** (δ bundles the prose update) | queued (δ) |
| `eval-unified-schedule-executor.md` (stub) | successor; this PRD constrains itself to not deepen the walk scheduler | [MILESTONE] design-session trigger | **stub PRD** (milestone task filed with this batch) | stub committed |
| `engine-unified-build-dag.md` | context only — (b) will extend its driver; Stage-5 legacy deletion is #4727 | none in this PRD | n/a | n/a |
| `fea-load-support-selector-migration.md` (#4370) | downstream beneficiary (correct ordering for @optimized consumers) | none owned here (#4370 already depends on #4900) | n/a | n/a |

Task #4905 (R3d mint-path polish: DRY the 5-site mint block, drain diagnostics) is **adjacent, not a dependency** — it touches the surviving mint arms, not the deleted sweeps. Whichever lands second rebases; no edge.

No cross-repo seams.

## 9. Decomposition plan

All tasks: `grammar_confirmed=true` (no novel syntax anywhere; fixtures reuse existing parsed shapes). Modules per task listed narrow.

- **α — no-stale-Undef invariant checker + debug-gate harness (eval/eval_cached).** Modules: `crates/reify-eval` (new invariant module + tests). Deps: **#4946** (out-of-batch). Signal (leaf-grade even though γ consumes it): new debug-gate suite runs the checker across the eval fixture corpus + `examples/`, green; seeded-violation self-test proves the checker fires. Consumer: γ/δ (regression net) + the standing debug gate.
- **β — flip the #4726 compute `value_inputs` filter to `cell_type`-based exclusion.** Modules: `crates/reify-eval/src/engine_eval.rs` (+ a compute-cache-key regression). Deps: **#4946** (serializes engine_eval churn). Intermediate; unlocks γ. Observable component: existing @optimized suites green; new regression asserts a geometry-typed `ValueRef` is excluded from the value bucket even when present in the graph.
- **γ — geometry lets emit first-class value cells (the lowering flip).** Modules: `crates/reify-compiler/src/entity.rs`, `crates/reify-eval` (graph/deps/build touch-points), differential suites. Deps: α, β. Intermediate; unlocks δ, ε. Observable component: full `--scope all` suite + differential corpus green; boundary rows 1/2/7.
- **δ — delete the compensation stack; capstone regression gate + docs.** Modules: `crates/reify-eval/src/{engine_eval,engine_edit,engine_build,geometry_ops}.rs`, `docs/design/symbolic-eval-nested-selector-resolution.md`, `docs/prds/v0_6/value-eval-geometry-addressing.md`. Deps: γ, **#4655** (out-of-batch; the capstone e2e must exist on main). **Leaf.** Signal: R3E trio + R3f let-backed member + `printer_print_envelope_eval_e2e` (release-gated, `--profile both`) all green with `re_eval_consumers_of_in_walk_mints*`, `minted_in_walk`, and both post-walk mints **absent from the tree** (grep-absence); invariant suite green.
- **ε — edit-path closure: invariant extension + let-backed edit regression + stale-comment fix.** Modules: `crates/reify-eval/src/engine_edit.rs` + invariant module + tests. Deps: γ, δ (file-conflict serialization on engine_edit.rs). **Leaf.** Signal: new edit-path let-backed regression green in the debug gate; edit-path invariant suite green; the false backstop comment at `engine_edit.rs:964` gone.
- **μ — [MILESTONE] stage-(b) design session** (filed under the stub PRD, same batch). Deps: δ, ε. Signal: milestone semantics — flip condition met → a `/prd` design session for `eval-unified-schedule-executor.md` is triggered (see stub PRD).

DAG: `#4946 → {α, β} → γ → {δ, ε} → μ`, with `#4655 → δ`. Nothing points at #4946 or #4655 from upstream.

## 10. Out of scope

- **Stage (b)** — the unified-schedule eval executor (stub PRD + milestone; design at flip time).
- **Language surface for geometry-let cells** — visibility/dot-access/export semantics stay unchanged (ratified internal-only). A future language PRD owns any surface change.
- **Threading compute dispatch into `reeval_cone_cell`** — mooted by δ (the sweeps that needed it are deleted; ordering routes @optimized consumers through live dispatch). If (b) later re-introduces re-visitation, it owns that decision.
- **Solver-side fixpoint** (geometry-in-the-loop) — explicitly deferred by `geometric-relations.md`; unrelated to this ordering work.
- **Unnamed realizations** — never had cell keys; the post-walk mints never wrote them either; no change.
- **#4905's mint-arm polish** — adjacent task, unchanged.

## 11. Open questions (tactical)

1. **Invariant module placement** — `crates/reify-eval/src/invariants.rs` vs a test-support module. Suggested: a small `pub` module in the crate (callable from integration tests and, later, a debug_assert site). Decide in α.
2. **debug_assert promotion** — after the exclusion set has soaked across the corpus for a while, promote the checker to an always-on debug-build assertion in `eval`/`eval_cached`/`edit_*`? Suggested: revisit at μ (the (b) design session) with soak data. Decide then.
3. **Post-deletion diagnostic passes** — `detect_unresolved_ad_hoc_selectors` / `detect_unresolved_geometry_consumers` (`engine_eval.rs:4541/:4552`) may shrink once ordering is structural. Suggested: leave in δ unless trivially dead; file follow-up if non-trivial.
4. **Invariant corpus scope** — exact fixture set for α's harness (all of `crates/reify-eval/tests` fixtures + `examples/*.ri`?). Decide in α; err broad.

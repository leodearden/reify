# Design — Option A: Unified Build DAG (eager, dependency-ordered single-pass evaluation of `Engine::build()`)

**Status:** design + de-risked, implementation-ready *after* the listed required changes are absorbed. **/prd-candidate** — do not implement from this doc directly; hand off via `/prd` (decompose) or `/do`.
**Provenance:** multi-agent design session (ultracode) — 7-agent reconnaissance → 10-agent design synthesis (judge panel + specialists) → 27-agent adversarial red-team. Red-team verdict: **GO-WITH-CHANGES** (worklist spine sound; the original "bodies-unchanged / minimal" framing was materially optimistic — corrected throughout).
**Code anchors** are as of `HEAD b0077500f5`; main moves fast, so **re-locate every symbol at implementation time** — cite-by-symbol, the line is a hint.
**Owners / consumers:** task 3205 (curated-edge fillet) and task 4275 (DFM `fits_build_volume`) are the two blocked L2 consumers this unblocks. Durable background: `~/.claude/projects/-home-leo-src-reify/memory/reference_constraint_phase_pre_geometry.md`, `data/escalations/afk-digest.md`, escalations `esc-3205-34` / `esc-4275-38`.

---

## 1. Problem — one root cause behind two blocked L2s

`Engine::build()` (`crates/reify-eval/src/engine_build.rs`) runs a **fixed, kind-ordered pipeline**, not a data-dependency graph:

| Phase | What it does | Key sites |
|---|---|---|
| **P1 `check()`** | `eval()` populates `values: ValueMap` (params/autos/lets, lets topo-sorted by `detect_let_cycle`); then `check_constraints_against_templates()` evaluates constraints over those **frozen** values with a **kernel-less** `EvalContext`. | `engine_constraints.rs:531`; `EvalContext::new` `reify-expr/src/lib.rs`; `SimpleConstraintChecker::check` `reify-constraints/src/lib.rs:55` |
| **P2 `execute_realization_ops`** | per-template → per-realization → per-op loop; the **only** phase that calls the kernel; mints handles into `step_handles` (global) + `named_steps` (per-template). | `engine_build.rs:3714` |
| **P3 `post_process_geometry_handle_cells`** | the **only** place `Value::GeometryHandle` enters `values` (hydrated from `named_steps`). | `engine_build.rs:4879` |
| **P4 `run_post_processes`** | geometry queries (`bounding_box`/`volume`/…), body-mass-props, topology selectors (`edges`/`faces`/…), ad-hoc selectors — all whole-template passes. | `engine_build.rs:5337`; `try_eval_geometry_query` `geometry_ops.rs:1747`; `try_eval_topology_selector` `geometry_ops.rs:2645` |
| **P5 export** | terminal handle → mesh / STEP. | — |

`BuildResult.constraint_results` is taken **frozen** from `check_result` (`engine_build.rs:2580`) and **never re-checked** after geometry exists. The **discriminator** (two independent traces agree): value cells *do* receive the P3/P4 patch; `constraint_results` do *not*.

**The shared root cause** — a consumer in an *earlier* phase reaches *forward* across a phase boundary to a producer in a *later* phase:

- **esc-3205 (task 3205) — curated-edge `fillet(solid, edges, radius)`.** The fillet `GeometryOp` dispatches in P2, but its `edges` arg is a topology-selector resolved only in P4, and the parent solid's handle enters `values` only in P3. Dispatch fails: *"target geometry handle not yet resolved in the values map."* (The 3-arg `Fillet.edges` IR + `resolve_subhandle_list` scaffolding are green-at-unit on the **task/3205 branch**; **main's `Fillet` is still 2-arg** — `reify-ir/.../geometry.rs:585`.)
- **esc-4275 (task 4275) — DFM constraint `fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`.** Evaluated in P1 before geometry; `bounding_box` resolves in P4; `build()` returns the frozen-Indeterminate P1 result.

Thesis under test: **"Reify is a dependency graph, not a phase-by-kind pipeline."** Collapse the fixed P1…P5 into one pass ordered by data dependency, realizing geometry inline at the point a consumer's dependencies are satisfied.

---

## 2. Existing substrate (this is a re-sequencing, not a green-field graph engine)

Verified present on main — Option A *extends* this, it does not invent it:

- **`deps.rs`**: `DependencyTrace { reads, realization_reads }` (`:22`); `ReverseDependencyIndex` with **both** a VC→`NodeId` map (`index`) **and** a Realization→`NodeId` map (`realization_index`, `add_realization`/`realization_dependents_of`, `:67/:104/:115`) — **confirmed (C1)**; an unused `DependencyMap::topological_order()` (a Kahn-based, abandoned single-pass prototype, VC-only — *not* the production sorter).
- **`dirty.rs`**: `compute_levels`/`topological_sort` (Kahn over `NodeId`), `compute_dirty_cone` (`:26`), `compute_eval_set` (dirty ∩ demand), `DebugOrd` deterministic tie-break (`:253`).
- **`engine_eval.rs`**: `detect_let_cycle` (`:312`) — Kahn-local-to-lets, cycle = size-mismatch (`:341`).
- **`cache.rs`**: `NodeId` = `Value | Constraint | Realization | Resolution | Compute` (`:18`) — **confirmed (C3)**, no GeometryOp/Selector/Query kinds.
- **`reify-ir/.../value.rs`**: `DeterminacyState` = `Determined | Undetermined | Provisional | Auto` (**C13**); autos start `Undef(Auto)`, flip to `Determined` only after their `Resolution` solver pass.
- **`reify-core/.../diagnostics.rs`**: `DiagnosticCode` is `#[non_exhaustive]` (**C14**) — new codes are additive.
- Constraint checker is a **trait** (`ConstraintChecker`); `SimpleConstraintChecker` builds `EvalContext::new(values, functions)` — **no kernel** (**C6**). The engine owns the kernel and `rewrite_geometry_queries`, so geometry can be resolved engine-side **before** the trait boundary — **no trait break**.

**Two facts the red-team corrected that change the work estimate:**
- `extract_realization_dependencies` returns `realization_reads = Vec::new()` **unconditionally** (`deps.rs:38/:344/:401`), its `Boolean { .. } => continue` arm **drops both operands** (`deps.rs:394`, **C4**), and the Modify/Transform/Pattern/Sweep arms destructure only `args` — never their `GeomRef` targets/profiles. The sole `realization_reads` writer folds into `NodeId::Value` keys only. **So the dependency graph does not yet contain Realization→Realization, Constraint→Realization, or cross-sub `GeomRef::Sub` edges.** Building those is a *substantial* edge-extraction pass, not a one-line addition.
- `try_eval_topology_selector` / `try_eval_geometry_query` are called **only** from the whole-template P4 passes (`engine_build.rs:5436`/`:5305`); `execute_realization_ops` takes `values: &ValueMap` **read-only** and neither hydrates output cells nor resolves selectors. **So "fold P3/P4 inline" is genuinely new per-node executor code, not a body-unchanged re-dispatch.**

---

## 3. Deliverable 1 — Target execution model

### 3.1 Spine: an online Kahn worklist over a heterogeneous node graph

A new module `crates/reify-eval/src/engine_fixpoint.rs` holds `run_unified_pass(...)`. It is an **online Kahn topological worklist** over today's `NodeId` enum (no new node *kinds*; a multi-op realization stays **one** coarse `NodeId::Realization`). It is driven off the existing `ReverseDependencyIndex` (O(V+E)) — **not** `compute_levels`, which is Value-only-decrement (`dirty.rs:212`, **C2**) and quadratic (`:213`), and which the design explicitly disowns.

> **Why a coarse node, not per-op?** It preserves the verified atomic per-realization rollback (`handle_start` `engine_build.rs:3754` → `step_handles.truncate(handle_start)` `:4592`, **C9**), keeps the `RealizationCache` key 1:1 (no per-op memo), and avoids the `&mut self` kernel-reentrancy hazard (`execute_with_history` is `&mut self` `geometry.rs:2338`; queries are `&self` `:2347`, **C8**) — the coarse driver takes the `&mut kernel` borrow only at a realization's scheduled slot, never recursively. The rejected lazy/pull `force()` model (candidate A2) would hold `&mut` across a re-borrow.

**Readiness gate = `DeterminacyState::Determined`, not map-presence.** Autos are `Undef(Auto)` until their `Resolution` node's solver pass writes them back `Determined`. Gating on `Determined` makes auto-settle a real graph edge, so a consumer of an auto is structurally scheduled after the solver and auto-settle order cannot perturb the schedule.

**Realization is eager-over-reachable, NOT lazy dead-code-elimination.** Cold `build()` seeds the demand set with **all** realizations, byte-preserving today's eager kernel-error surfacing (`mark_realization_failed` `engine_build.rs:4816`); only the already-demand-scoped warm paths restrict to a visible/dirty cone. (Lazy DCE — silently never realizing unconsumed geometry — is rejected as a behavior change that would suppress declared-but-unconsumed geometry errors.)

**Determinism** rides `BTreeSet<DebugOrd>` `pop_first` + content-derived `NodeId`s (entity+member / entity+index strings; no addresses, no `HashMap`-iteration leak). ⚠️ Note that `DebugOrd` is **lexicographic, not declaration order** — see §4/§6, this is load-bearing for export and for cross-component scheduling.

### 3.2 Three first-class geometry-path executors (corrected — *not* "bodies unchanged")

The whole-template P3/P4 passes must be **decomposed** into schedulable per-node executors so a selector can run *between* two realizations:

1. **Realization executor** — wraps the existing `execute_realization_ops` body (rollback preserved verbatim), then runs a **per-realization slice** of `post_process_geometry_handle_cells` to hydrate *that realization's* output `Value::GeometryHandle` into `values` immediately (folding P3 inline at the realization's slot). Decoupling this slice from its current `&template.realizations` whole-template loop is real work.
2. **Selector/query-cell executor** — a **per-cell slice** of `post_process_topology_selectors` / `try_eval_geometry_query`, run with the live kernel + `named_steps` at the cell's scheduled slot (after its backing realization's node).
3. **Constraint executor** — engine-side `rewrite_geometry_queries` (with the new arm, §3.3) folds geometry-query leaves to literals, then the unchanged kernel-less `SimpleConstraintChecker`. The constraint node is scheduled after its geometry deps, so the folded literals are real.

Kahn edges for the geometry path: `body-realization → output-cell hydration → selector-cell → consumer-realization`.

### 3.3 How constraints obtain geometry — engine-side, no trait break

`rewrite_geometry_queries` (`geometry_ops.rs:1908`) today recurses on geometry-query-leaf / `BinOp` / `UnOp` arms but falls through `_ => expr.clone()` (`:1942`, **C5 confirmed**). An **outer** `FunctionCall` such as `fits_build_volume(bounding_box(..), bounding_box(..))` — not itself a geometry query — hits that fall-through, so its inner `bounding_box` leaves are **never folded today**. Required: a **new `FunctionCall`-args recursion arm** that recurses into each argument subtree (mirroring the `BinOp` recursion) while leaving the outer non-query call intact for the kernel-less checker. The checker (`reify-constraints`) stays byte-identical; the engine resolves geometry before the trait boundary.

> ⚠️ **Not sufficient alone (red-team breaker, §7):** even with the new arm, `bounding_box(proc.build_volume)` does **not** fold, because `resolve_geometry_handle_arg` matches **only** `CompiledExprKind::ValueRef` and returns `None` for the member-access (`sub.member`) shape — so the cross-sub leaf folds to `Undef` and the constraint stays Indeterminate. Cross-sub / `IndexAccess`-on-`StructureRef` geometry-handle resolution is a required addition (§7, §4.2).

### 3.4 Constraint freeze (C7) is intentionally retired in the unified path

`constraint_results` is currently frozen pre-geometry (`engine_build.rs:2580`, **C7**). In `UnifiedDag` mode, every constraint runs as a worklist node after its realization deps and its result is written to `BuildResult` directly. **Cleanest precedence rule:** drop the pre-geometry constraint dispatch from `check()` entirely and run *all* constraints as worklist nodes (avoids double-evaluation and any "which result wins" ambiguity). The kernel-less `check()` path stays for `reify check` / no-kernel callers.

---

## 4. Deliverable 2 — Cycle-detection contract (load-bearing acceptance bar)

A genuine cycle across **any** node-kind pair must yield a **clear** diagnostic naming the cycle members — never silent `Undef`, never a hang. Task 4317 proved this is easy to get wrong.

### 4.1 Two-stage detection

**Stage A — Kahn residue (hang-proof).** The online worklist *is* Kahn. In-degree counts **all** edge kinds (VC reads, Realization→VC arg reads, cell→Realization `realization_reads`, Constraint→Realization, Realization→Realization via `GeomRef::Sub`, Resolution→VC `auto_params`). After the worklist drains, `residue = nodes \ processed`. Because Kahn never pops a node whose in-degree stayed > 0, **cyclic nodes are never executed** (no kernel call, no handle insert), there is **no fixpoint iteration**, and the loop terminates in O(V+E). *A cycle cannot hang.* This generalizes the verified `detect_let_cycle` size-mismatch trick (`engine_eval.rs:341`).

**Stage B — Tarjan SCC discriminator.** The residue is (i) nodes in true cycles ∪ (ii) nodes merely *downstream* of a cycle. Run one Tarjan SCC over the residue subgraph (successor iteration in `DebugOrd` order for determinism):
- `|SCC| > 1`, or a singleton **with a self-edge** (`let x = x + 1`) → **genuine cycle** → one `E_EVAL_CYCLE` per SCC.
- singleton **no self-edge** → **stranded-downstream** → no cycle diagnostic; left `Undef` (cause `BlockedByCycle` if the undef-tracer is wired).

This three-way separation is the upgrade over a flat set-difference:

| Situation | In residue? | In SCC>1 / self-loop? | Diagnostic |
|---|---|---|---|
| Genuine cycle (any kind-pair) | yes | yes | `E_EVAL_CYCLE`, ordered path |
| Downstream of a cycle | yes | no (singleton) | none (cause `BlockedByCycle`) |
| Missing producer (unbound ref) | **no** | n/a | normal unbound/`Undef` at eval |
| Failed realization (kernel error) | **no** | n/a | geometry-error `Diagnostic`; downstream cells `Undef` |

A missing producer is **not** a residue case: a node reading an absent cell skips that edge, stays in-degree-0, *is* scheduled, evaluates to `Undef`. So the residue is *only* cycles + their downstream cone, which Tarjan cleanly separates.

### 4.2 Diagnostic, ordered path, all node kinds

- New `DiagnosticCode::EvalCycle` (`E_EVAL_CYCLE`, error) + reserved `EvalUnresolved` (`E_EVAL_UNRESOLVED`) — additive (`#[non_exhaustive]`, **C14**); round-trip/serde tests mirror `UnresolvedType`.
- New `NodeId::describe()` (enum unchanged) names every kind: `Value→entity.member`, `Realization→realization 'name'`, `Constraint→constraint #id`, `Resolution→scope 'name'`, `Compute→computation #id`. This closes the verified `_ => None` regression where `detect_let_cycle` dropped non-Value kinds.
- **Ordered path** via a DFS-on-stack slice confined to each SCC: e.g. `E_EVAL_CYCLE: circular dependency: Bracket.fillet_edges -> realization 'Bracket#1' -> Bracket.fillet_edges`. Determinism: drive Tarjan's outer iteration and successors in `DebugOrd` order, and order multi-SCC diagnostics by lex-first member, so the diagnostic *vector* is byte-identical run-to-run.

### 4.3 Solver-round interaction — corrected

The solver adds **no** edges at solve time; it is one static `NodeId::Resolution` node per scope (a single forward pass, no fixpoint, cannot hang). A **`let`/value** cycle through an unsettled auto is a real static SCC caught before solve.

> ⚠️ **Corrected (red-team, confirmed):** a **geometry-backed constraint that constrains an auto** is **not** a static SCC and is **not** caught before solve. `build_solver_problem` filters constraints by `extract_dependency_trace(c.expr).reads ∩ auto_ids` (`engine_eval.rs:603`); for `fits_build_volume(bounding_box(part), …)` the constraint reads `{part, build_vol}`, never the auto `w` (read by *part's realization*, not by the constraint expr). The constraint is dropped from the solver problem; the only auto↔Resolution coupling is the runtime solver *write*, never a static read edge — so no Tarjan SCC can contain it. The honest contract: compute a **transitive auto-read closure** (constraint.reads → backing Realization → that realization's value/realization reads, *with the `deps.rs:394` Boolean-operand drop fixed*) and, when a geometry-backed constraint transitively reads an auto, **emit `E_EVAL_UNRESOLVED` and decline to solve that class** — preserving the no-hang guarantee. (A genuine geometry-in-the-loop solver round would contradict "one forward pass per scope" and is out of scope.) The Stage-2 differential corpus must carry this case explicitly, because **legacy degrades identically** so a pure legacy-vs-unified diff will not surface it.

### 4.4 Acceptance bars

Per-kind-pair cycle tests (param↔let; geom↔constraint; selector↔op; realization↔realization via `GeomRef::Sub`; resolution-involved value cycle); self-loop `let x = x`; two disjoint cycles → two deterministically-ordered diagnostics; **missing producer is NOT a cycle**; **failed realization is NOT a cycle**; determinism (100× → byte-identical diagnostic vector); every node in a genuine SCC left unexecuted (no kernel call, no handle insert, `BuildResult` carries the error not a panic/frozen-Indeterminate); the geometry-backed-constraint-on-auto case yields `E_EVAL_UNRESOLVED`, not a hang and not a false `E_EVAL_CYCLE`.

---

## 5. Deliverable 3 — Warm-state / `eval_cached` impact + determinism

There is a **family** of warm surfaces; all must funnel through one ordering core: (a) `eval_cached` (`engine_eval.rs:2822`, editor/LSP, **expr-only, no kernel**); (b) `build_snapshot` (`engine_build.rs:1795`) and `tessellate_from_values` (`:3295`, GUI geometry); (c) the `concurrent.rs` parallel adapter; (d) `edit_param`/`edit_source`.

### 5.1 The task-4317 twin — corrected scope (OPEN, FIVE sites)

> ⚠️ **Stale premise corrected (red-team, verified `HEAD b0077500f5`):** task/4317 is **not merged into main**; C11 is **open and broader** than the design first stated. The bare (`.with_determinacy`-less) cell-eval exists at **five** warm/edit sites, not two: `engine_eval.rs:3252` (eval_cached `Let`), `engine_eval.rs:3068` (eval_cached `Param`-default), `concurrent.rs:481` (wave-2; also needs `snapshot_values` plumbed), `engine_edit.rs:1053` (`edit_param` top-level `Let`), `engine_edit.rs:2487` (`edit_source` top-level `Let`). `engine_edit` adds `.with_determinacy` only for *guard* cells, leaving non-guard `let r = determined(x)` cells evaluating `Undef` on edit; the `DeterminacyPredicate` None-branch silently returns `Undef`, and its `debug_assert` sits inside the `Some` arm so it does **not** fire on the None case (silent in debug *and* release).

**Structural fix:** one private `Engine::cell_eval_ctx(values, snapshot_values, runtime_sink)` constructor that *always* carries `.with_meta + .with_determinacy + .with_runtime_diagnostics`. Every cell-expr eval in the unified driver routes through it — there is then no call site that can drop determinacy. `eval_cached` stays expr-only by **executor selection** (kernel-less executors for the geometry node kinds), not a separate code path. Whoever lands first (task/4317 vs this) must re-apply at post-merge line numbers; land a **warm-path RED regression before Stage-2** so the corpus baselines against a fixed legacy.

> Note (refuted over-claim): dropping `.with_determinacy` does **not** leak `Undef` past the `Determined` gate for *ordinary* `Let` cells — cold and warm both stamp `(val, Determined)` for plain lets, so a plain-let differential would see identical results. The genuine residue is warm **`DeterminacyPredicate`** evaluation (the real 4317-twin), which `cell_eval_ctx` fixes. This is a parity issue, not a gate-leak.

### 5.2 Warm/incremental unification is its OWN migration stage (corrected)

> ⚠️ **Corrected:** `eval_cached` and `concurrent.rs` wave-2 contain **zero** references to `execute_realization_ops` / `named_steps` / kernel / `try_eval_geometry_query` / `try_eval_topology_selector` — they are purely expression-level. Making warm builds "demand-scope realizations via the same driver" requires threading the entire kernel/`named_steps`/realization-execution/rollback stack into a path that has none of it. **Scope this as its own explicit migration stage with its own gated corpus.** Until it lands, the 3205/4275 *folding* is unreachable on the warm path, and Stage-2 must **not** assume warm == cold for those cases.

Two further warm invariants the red-team confirmed:
- **Solved-value back-prop:** `eval_cached`'s `SolveResult::Solved` arm is a documented no-op (safe today only because the sole production caller, reify-lsp, takes the cold branch on any content change). The unified driver's Resolution executor on the warm/demand path **must** write solved autos back as `Determined` and re-dirty downstream `let` nodes in the same pass (mirroring cold `eval()`), not inherit the no-op arm. Stage-2 must include a `let y = auto_x + N` driven through the warm path.
- **Incremental realization dirty-prop is currently dead code:** `compute_dirty_cone_with_realizations` (`dirty.rs:95`, **C15**) has **no production caller**; `diff_realizations` keys on the **static IR `content_hash`** (id+ops), which never moves on a value-driven geometry change; `edit_param`/`edit_source` unconditionally `clear_realization_cache()` (full flush, `engine_edit.rs:901`). **Scope the "incremental sub-DAG" claim to VALUE cells for initial landing**; realization warm-correctness is delivered by full-flush cold re-execution. Selective realization eviction requires a `RealizationNodeData` *executed-result* hash (recompute-then-compare) feeding `changed_realizations` first — otherwise propagation silently no-ops (a guaranteed future 4317-class stale).

### 5.3 `build_snapshot` export is pop-order-fragile (corrected, medium)

`build_snapshot` exports `*step_handles.last().unwrap()` (`engine_build.rs:2100`) — the global last-appended handle. Under `DebugOrd` (lexicographic) pop order this can export a **different** body than legacy for a multi-realization/multi-entity module. `build()` is **immune** (positional `terminal_handles[t_idx][r_idx]` + `collect_export_bodies_walk`, `:2369/:2483`). Required: replace `step_handles.last()` with `build()`'s positional terminal-handle surfacing before routing `build_snapshot` through the worklist; add a multi-realization snapshot-export case to Stage-2. (`build_snapshot` is test-only today — low blast radius but a real correctness trap under reordering.)

### 5.4 Determinism (three pillars + a stronger cross-surface guarantee)

(1) **Order** — content-derived `NodeId`s + `BTreeSet<DebugOrd>` tie-break → identical input ⇒ identical schedule. (2) **Value** — each executor is pure given frozen `values`; per-kernel `GeometryHandleId`s are minted in the deterministic execution order and never enter the sort. (3) **Diagnostics** — pushed in pop order ⇒ byte-identical vector. **Cross-surface:** because cold/`eval_cached`/`build_snapshot`/concurrent/edit all share `cell_eval_ctx` and the same worklist over the same memoized `trace_map`, "warm output == cold output" becomes *structural* rather than hand-mirrored — retiring 4317's failure mode. The concurrent path runs a Kahn level in parallel; sound only because intra-level nodes have no mutual edges and the result map is insert-by-key order-insensitive — **the new realization→selector/constraint edges must be re-verified not to place two realizations sharing a `named_steps` namespace in one level**, else serialize realization nodes within a level.

---

## 6. Deliverable 4 — Proof it covers BOTH 3205 and 4275 (corrected mechanics)

### 6.1 esc-3205 — curated-edge fillet

Idiom: `let b = box(...)`; `let top_edges = edges_at_height(b, h, tol)`; `let filleted = fillet(b, top_edges, r)`.

> ⚠️ **Mechanics corrected (red-team, verified main + task/3205):** a sibling geometry `let` referenced by bare ident (`b`) is **inlined** into the consuming realization's op list as **`GeomRef::Step`** (resolved against per-realization `step_handles`), **not** `GeomRef::Sub` through `named_steps`. `GeomRef::Sub` is *exclusively* the `self.<sub>.<member>` cross-component handshake. The earlier "filleted reads `GeomRef::Sub body` / `named_steps['body']`" framing was wrong, as was the cited "`deps.rs:279` realization_ref fallback" (that symbol is `geometry_cell_realization_reads`, a realization→its-own-output-cell edge; `realization_ref` is a runtime `GeometryHandle` field, not a graph edge).

**Corrected trace.** `filleted` is a self-contained realization (`b` inlined as `Step 0`). The real cross-node dependencies are **value cells**:
1. `NodeId::Realization(b)` has only scalar param deps → in-degree 0 → pops first → mints `b`'s handle and (via the per-realization hydration slice) `b`'s `Value::GeometryHandle` cell.
2. `top_edges` is a selector `Value` cell with a **new `realization_reads = [b]`** edge (added in `build_trace_map_and_fields`) → ready once `b` ran → the selector executor resolves the curated edge list against the live kernel into `values[top_edges]`.
3. `NodeId::Realization(filleted)` reads `top_edges` (a `ValueRef`, already in `reads`) and `b`'s hydrated handle cell → ready last. On task/3205, `compile_geometry_op` resolves the parent via `resolve_parent_geometry_handle_arg(target_expr, values)` (now populated) and the `edges` arg via `eval_named_arg` reading `values[top_edges]` (now populated) → **dispatches with both inputs resolved.** The "target geometry handle not yet resolved" failure cannot occur because the schedule linearized producer-before-consumer.

Rollback preserved (coarse node); OCCT per-edge FFI (`make_fillet_with_history`) unchanged — Option A only fixes the scheduling that starved the op of resolved args. **Hard prerequisite:** land task/3205's **green in-loop fix** (the IR *and* the working selector-thread that makes `fillet3arg_edges_at_height_records_4_curated_edges` pass), not just the 2-arg→3-arg IR.

### 6.2 esc-4275 — DFM `fits_build_volume`

Actual stdlib form (`stdlib/process.ri:190-192`): `param proc : Adding`, bound at the call site by `let proc = FdmPrinter()`, constraint `fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`.

1. `part`'s realization and `proc.build_volume`'s realization reach in-degree 0 from their scalar params → run → mint handles.
2. Constraint `C` gets `realization_reads` for the realizations whose `bounding_box` leaves it needs (detected via `is_geometry_query_call`, `geometry_ops.rs:1795`) → scheduled strictly after both.
3. At `C`: `rewrite_geometry_queries` with the **new `FunctionCall`-args arm** folds the inner `bounding_box(part)` leaf to a `Value::BoundingBox` literal, leaving the outer `fits_build_volume` geometry-free → kernel-less checker → real `Satisfied`/`Violated`. The driver writes `constraint_results` (un-freezing `engine_build.rs:2580`).

> ⚠️ **Corrected (red-team breaker):** the same-scope `bounding_box(part)` leaf folds, but `bounding_box(proc.build_volume)` does **not** with the new arm alone — `resolve_geometry_handle_arg` matches only `CompiledExprKind::ValueRef` and rejects the member-access (`proc.build_volume`) shape → that leaf folds to `Undef` → `fits_build_volume(bbox, Undef)` → still Indeterminate. **Required addition:** `IndexAccess`-on-`StructureRef` / cross-sub (`sub.member`-keyed) geometry-handle resolution in `resolve_geometry_handle_arg`. The 4275 acceptance test must use the genuine `let proc = FdmPrinter()` form and assert a *definite* `Satisfied`/`Violated`.

---

## 7. Deliverable 5 — Blast radius + staged, flagged migration

### 7.1 Changes vs stays (honest scope — *not* "bodies unchanged")

**Changes** (reify-eval, concentrated):
- **`deps.rs` — the real work, not a footnote.** A new `GeomRef`-resolution edge-extraction pass feeding `build_trace_map_and_fields` + `ReverseDependencyIndex::build_from_graph_and_fields`: walk `Boolean.left/right`, `Modify`/`Transform`/`Pattern.target`, `Sweep.profiles`; treat `GeomRef::Step` as intra-node (no edge), map `GeomRef::Sub(name)` → producing `RealizationNodeId` and register via `add_realization` **counted in in-degree**; fix the `:394` Boolean-operand drop; populate `realization_reads` for **selector/query Let cells AND geometry-reading constraints**. Enumerate **three** new edge types: selector/query-cell→realization, constraint→realization, realization→realization.
- **`engine_fixpoint.rs` (NEW)** — online Kahn worklist over `ReverseDependencyIndex`; `Determined` readiness gate; in-degree + decrement uniform over **all** edge kinds (not the Value-only `compute_levels` shortcut); Tarjan-SCC residue discriminator + ordered-path reconstruction; debug-assert "no executor pops a node whose producer's `named_steps` handle is absent."
- **`engine_build.rs`** — `build()` per-template loop replaced under flag by a driver call invoking the **three decomposed executors** (Realization wrapping `execute_realization_ops` with rollback verbatim; per-realization output-cell hydration slice; per-cell selector/query resolution slice). `BuildResult` writes driver-computed `constraint_results` (C7 retired). `build_snapshot`/`tessellate_from_values` route through the same demand-scoped driver; **fix `build_snapshot` `step_handles.last()` export** to positional terminal handles. Specify where `seed_cross_sub_named_steps` runs in the per-node model (Realization-executor prelude re-seeding `named_steps` from completed child realizations).
- **`geometry_ops.rs`** — `rewrite_geometry_queries` new `FunctionCall`-args arm; `resolve_geometry_handle_arg` cross-sub/`IndexAccess` resolution.
- **`engine_eval.rs`** — `cell_eval_ctx` single site (fixes C11 across five sites); `detect_let_cycle` generalized to whole-graph SCC reporting; `eval_cached` + concurrent adapter migrated to `cell_eval_ctx`.
- **`engine_edit.rs`** — migrate top-level `Let` eval to `cell_eval_ctx`; keep full `RealizationCache` flush for the initial landing.
- **`cache.rs`** — `NodeId::describe()` (enum unchanged). **`reify-core/diagnostics.rs`** — `EvalCycle` + `EvalUnresolved` (additive).

**Stays unchanged (verified):** `reify-compiler` (whole crate — `collect_value_refs` lives in `reify-ir`); `reify-constraints` (whole crate — kernel-less checker, no trait break); the kernel crates + `dispatcher.rs` BFS + `RealizationCache` keying + OCCT FFI; `execute_realization_ops`'s body + its rollback. *(Note: "reify-eval zero-change" was never claimed — reify-eval is where essentially all the work lands.)*

### 7.2 Staged, flagged rollout

`enum BuildScheduler { LegacyMultiPass, UnifiedDag }` (default **UnifiedDag** as of Stage-4 #4362; `REIFY_BUILD_SCHEDULER=legacy` is the one-release kill-switch) + `feature = "unified-dag"` (vestigial; no longer gates `from_env`; pending Stage-5 removal #4727).

- **Stage 0** — land additive edges; add a debug-only `assert_dag_complete` on every *legacy* build verifying the unified DAG would schedule every `named_steps` producer before its consumer. **Upgrade it to check realization→realization and constraint→realization reachability, not just VC reachability.** Zero user impact; surfaces missing edges as test failures.
- **Stage 1** — driver behind flag, default OFF; mechanical fixes (rewrite arm, cross-sub resolution, C7 un-freeze, determinacy unification) active only on the unified path. **Add a Stage-1 `residue == ∅` gate** on every acyclic legacy-passing corpus design (any false-positive `E_EVAL_CYCLE` or stranded-without-SCC is a worklist-totality bug).
- **Stage 2** — differential corpus: full `reify-eval/tests/` + `tests/golden` under both schedulers; assert `BuildResult` equivalence on the overlap domain; legitimate divergences (3205/4275 + newly-resolved-not-`Undef`) in a **per-case, reasoned allow-list** (no blanket patterns). Run unified twice → byte-identical. **Expand the corpus** to cases a pure legacy-vs-unified diff cannot surface because legacy degrades identically: auto+geometry-constraint; cross-sub multi-body assembly (parent named lexicographically before children); warm-path `let y = auto_x + N`; warm-path determinacy-predicate let cells via `edit_param`/`edit_source`/`eval_cached`/`concurrent`; multi-realization snapshot export; the 4275 let-bound form.
- **Stage 3** — unified-only acceptance tests (`fillet_curated_edges_3205_e2e`, `dfm_fits_build_volume_4275_e2e`), `#[cfg_attr(not(feature="unified-dag"), ignore)]`.
- **Stage 4** ✅ — landed (#4362). Default is `UnifiedDag`; `REIFY_BUILD_SCHEDULER=legacy` is the one-release kill-switch.
- **Stage 5** — delete legacy loop bodies, the enum, `detect_let_cycle`'s let-local body.

**Hard sequencing dependencies:** (1) land task/3205's **green in-loop fix** before claiming the geometry path sound; (2) warm/incremental unification is its **own** stage (it needs the kernel/`named_steps`/realization stack threaded into expr-only paths); (3) cross-kernel interleave can surface the bare-`GeometryHandleId` collision in `FeatureTagTable`/`TopologyAttributeTable` (tasks 4349/4351) earlier than today's per-template grouping — the per-kernel table reset must stay per-build, and the 4349 re-key may need to land first if a multi-kernel module is in the warm test set.

---

## 8. Deliverable 6 — Honest A-vs-B comparison

**A** = Unified Build DAG (this design). **B** = keep the pipeline, add two phases: **B1** a deferred geometry-op phase after `post_process_topology_selectors` (for 3205), **B2** a post-geometry constraint re-check pass (for 4275).

| Axis | A (Unified DAG) | B (Deferred phase + re-check) |
|---|---|---|
| Root-cause fit | Both escalations = one class (forward-reach across a phase boundary); one fix. | Two bespoke patches for one root cause. |
| Phase count | −4 (removes the fixed ordering). | +2 (5→7). |
| N-deep selector→op→selector chains | single sort, arbitrary depth. | a deferred op feeding a later selector needs a 2nd deferral → N phases or a fixpoint (re-deriving A, badly). |
| 4275 mechanical fix | new `FunctionCall`-args arm + cross-sub resolution. | needs the **same** arm + resolution. |
| Atomic rollback | preserved verbatim (coarse node). | preserved, but B1's re-run duplicates the rollback window. |
| Warm-state incrementality | the sorted pass *is* the incremental executor; one driver across surfaces. | none; deferred phase re-runs unconditionally; the `body_mass_props`-before-selectors time-bomb (task 3620) **remains**. |
| 4317 determinacy divergence | retired structurally (one `cell_eval_ctx`). | untouched. |
| Cycle handling | one total Kahn + Tarjan-SCC across all kinds; no hang, no silent `Undef`. | each new phase needs its own termination reasoning; B1's deferred chain risks a re-run loop with no global cycle guard. |
| Blast radius | larger (new driver + executor decomposition), flag-gated/reversible. | smaller (two additive phases). |
| Pays down phase-by-kind debt | **yes** (the thesis). | no (entrenches it). |
| Stepping-stone value | terminal. | **real**: B2's re-check + the shared rewrite arm are a strict subset of A. |

**Decision: choose A.** (1) Both escalations are provably one root cause — A fixes the *class*, B fixes two instances and leaves the next to a third patch. (2) A retires two independently-tracked time-bombs for free (task 3620 fixed-order `body_mass_props`; the 4317 cold/warm divergence); B touches neither and makes 3620 worse. (3) A is the warm-state win; B gives zero incrementality. (4) A is buildable from verified existing substrate.

**Honest case for B (recorded, not rationalized):** B has a genuinely smaller, lower-risk blast radius and does not re-sequence the central pipeline; its two pieces are a **strict subset of A**, so under schedule pressure (or the task/3205 sequencing dependency) **shipping B2 first is a legitimate stepping-stone A later subsumes with zero rework**; and B avoids A's cross-kernel-interleave risk (4349/4351) because phases keep today's per-template grouping. **Mitigation that makes A acceptable:** the Stage-0…5 flagged rollout + legacy/unified differential corpus means A never lands big-bang — legacy stays default and kill-switch until the gate is green; B has no equivalent legacy oracle since it mutates the same frozen result in place, making A the *more* reversible path despite the larger diff.

---

## 9. Red-team verdict & residual-risk register

**Verdict: GO-WITH-CHANGES.** The worklist spine (online Kahn + size-mismatch residue + Tarjan SCC + per-SCC `E_EVAL_CYCLE`, content-derived `NodeId` determinism) is **sound for edge-complete graphs**; the substrate to support it (`realization_index`, `NodeId` kinds, `DeterminacyState`, `#[non_exhaustive]` diagnostics) is confirmed present. The failures are all **edge-extraction completeness, executor decomposition, and resolution completeness** — not spine logic.

**Design-breaking (must be absorbed before implementation-ready):**
1. **Edge-completeness is the spine.** Add the `GeomRef`-resolution pass (Boolean operands, Modify/Transform/Pattern/Sweep targets, `GeomRef::Sub`→Realization edges, counted in in-degree); fix `deps.rs:394`. Without it, cross-component assemblies and the proofs mis-order under lexicographic `DebugOrd` pop order.
2. **4275 cross-sub leaf does not fold** — add `IndexAccess`/cross-sub geometry-handle resolution to `resolve_geometry_handle_arg`; assert a definite verdict on the genuine let-bound form.
3. **3205 "bodies unchanged" is false** — specify the three schedulable executors + `body→hydration→selector→consumer` edges; land 3205's green in-loop fix first.

**Managed risks (mitigation + acceptance bar required):** geometry-backed-constraint-on-auto false-negative → `E_EVAL_UNRESOLVED` + corpus case (§4.3); warm/incremental unification is its own stage (§5.2); warm Resolution executor must back-prop solved autos (§5.2); incremental realization dirty-prop is dead code + full-flush contradiction → scope to value cells initially (§5.2); `build_snapshot` pop-order export fix (§5.3); cross-sub value-ref path edge-incompleteness (`let x = self.comp.body`) → first-class edges; C7 retirement + constraint precedence explicit (§3.4); worklist in-degree/decrement uniform over all edge kinds + Stage-1 residue gate.

**Refuted attacks (design holds):** `compute_levels` Value-only asymmetry is a *forward hazard the design already disowns*, not a live bug (auto_params *are* in `reads`); dropping `.with_determinacy` does **not** leak `Undef` past the `Determined` gate for plain lets (only `DeterminacyPredicate` cells, the real twin); `eval_cached` serving pre-solve `Undef` for autos is unreachable in production; the spine itself is sound for edge-complete graphs (same-scope constraint/selector ordering rides transitively through the value cell via the `R→VC` output edge — a direct `R→C` edge is not required for the *in-scope* case; the genuine gap is the *cross-sub/cross-realization* `GeomRef` path).

**Stale premises corrected:** **C11/4317 is OPEN on main across five sites** (not landed, not two) — reclassify from "precondition met" to "bug the unification must fix"; the "`deps.rs:279` realization_ref fallback" edge **does not exist**; the proof-3205 "`GeomRef::Sub body` / `named_steps[body]`" mechanic is **wrong** (`b` is inlined as a `Step`; the load-bearing edge is the selector value cell). C5/C1/C2/C4/C6/C7/C10/C13/C14/C15 all **confirmed as stated**.

### 9.1 Load-bearing code-premise ledger (verified `HEAD b0077500f5`)

| # | Claim | Status |
|---|---|---|
| C1 | `ReverseDependencyIndex` has `realization_index` + `realization_dependents_of` (`deps.rs:67/104/115`) | ✅ confirmed |
| C2 | `compute_levels` decrements only `NodeId::Value` (`dirty.rs:212`) + quadratic rescan (`:213`) | ✅ confirmed (design disowns it) |
| C3 | `NodeId` = Value/Constraint/Realization/Resolution/Compute only (`cache.rs:18`) | ✅ confirmed |
| C4 | `extract_realization_dependencies` Boolean arm drops operands (`deps.rs:394`); `realization_reads` unconditionally empty (`:38/:344/:401`) | ✅ confirmed |
| C5 | `rewrite_geometry_queries` falls through `_ => expr.clone()` (`geometry_ops.rs:1942`); outer `FunctionCall` not folded today | ✅ confirmed (new arm required) |
| C6 | `SimpleConstraintChecker` kernel-less `EvalContext::new(values, functions)` (`reify-constraints/src/lib.rs:55`) | ✅ confirmed (no trait break) |
| C7 | `BuildResult.constraint_results` frozen from `check_result` (`engine_build.rs:2580`) | ✅ confirmed (retired in unified path) |
| C8 | `execute_with_history` `&mut self` (`geometry.rs:2338`); queries `&self` (`:2347`) | ✅ confirmed |
| C9 | rollback `handle_start` (`:3754`) + `truncate` (`:4592`) | ✅ confirmed (preserved verbatim) |
| C10 | main `Fillet` IR 2-arg (`geometry.rs:585`); 3-arg on task/3205 | ✅ confirmed (sequencing dep) |
| C11 | `eval_cached` drops `.with_determinacy` (the 4317 twin) | ⚠️ **corrected — OPEN, five sites, task/4317 not merged** |
| C12 | `detect_let_cycle` size-mismatch + Value-only `_ => None` describe | ✅ confirmed (generalize) |
| C13 | `DeterminacyState` variants; autos `Undef(Auto)`→`Determined` post-Resolution | ✅ confirmed |
| C14 | `DiagnosticCode` `#[non_exhaustive]` | ✅ confirmed (additive codes) |
| C15 | `compute_dirty_cone_with_realizations` exists, staged/unused | ✅ confirmed (dead code) |

---

## 10. Recommended next step

Hand off to **`/prd`** to decompose. A natural batch shape (DAG):

1. **Foundational edge-extraction** (Stage 0): `deps.rs` `GeomRef`-resolution pass + Boolean-arm fix + selector/query/constraint `realization_reads` + `assert_dag_complete` (realization & constraint reachability). *No behavior change; the correctness substrate everything else rides on.*
2. **`cell_eval_ctx` determinacy unification** (the C11/4317 fix across five sites) + warm-path RED regression. *Independently valuable; can land before the driver.*
3. **The worklist driver** (`engine_fixpoint.rs`) + cycle contract (Tarjan-SCC + `E_EVAL_CYCLE`/`E_EVAL_UNRESOLVED` + `NodeId::describe`) behind the `UnifiedDag` flag, default off + Stage-1 residue gate.
4. **Geometry-path executors** (realization wrapper + per-realization hydration slice + per-cell selector/query resolution slice) + `rewrite_geometry_queries` arm + cross-sub `resolve_geometry_handle_arg` + C7 retirement. *Depends on task/3205's green in-loop fix.*
5. **Differential corpus + acceptance** (Stages 2–3), with the expanded corpus cases.
6. **Warm/incremental unification** (its own stage): `build_snapshot`/`tessellate`/`eval_cached`/`concurrent` onto the driver; `build_snapshot` export fix; Resolution back-prop; value-cell-scoped incremental (full realization flush retained).
7. **Cutover + legacy removal** (Stages 4–5).

**Do not start implementing from this doc.** It is the de-risked design; the PRD gates (G1–G6) and decomposition come next.

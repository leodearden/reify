# PRD — Unified Build DAG (single-pass, dependency-ordered `Engine::build()`)

**Status:** deferred → ready to activate after Part 1 + re-scoped task 3205 machinery land. v0_6. **Approach B + H** (load-bearing core-engine seam — contract + two-way boundary tests below). Authored 2026-06-07.
**Design source:** `docs/design/engine-unified-build-dag-option-a.md` (multi-agent design + 27-agent red-team; verdict GO-WITH-CHANGES — spine sound, the failures were edge-completeness / executor-decomposition / resolution-completeness, all absorbed here). This is **Part 2 of 2**; Part 1 = `docs/prds/v0_6/engine-build-dag-substrate.md`.
**Code anchors** are as of `HEAD b0077500f5`; main moves fast — **re-locate every symbol at implementation time**.

---

## 1. Goal — collapse the fixed P1…P5 pipeline into one data-dependency pass

`Engine::build()` runs a **fixed, kind-ordered pipeline** (check → realize → hydrate → post-process → export), not a data-dependency graph. Two blocked L2 escalations are the **same root cause**: a consumer in an *earlier* phase reaches *forward* across a phase boundary to a producer in a *later* phase.

- **task 3205 — curated-edge `fillet(solid, edges, radius)`.** The fillet op dispatches in P2 but its `edges` selector resolves only in P4, and the parent handle enters `values` only in P3 → *"target geometry handle not yet resolved."*
- **task 4275 — DFM `fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`.** Evaluated in P1 before geometry; `bounding_box` resolves in P4; `build()` returns the frozen-Indeterminate P1 result.

Replace the per-template P1…P5 loop (behind a flag) with an **online Kahn worklist** over today's `NodeId` graph, realizing geometry inline at the point a consumer's dependencies are satisfied. Both escalations are then fixed by the *class*, not patched twice.

## 2. User-observable surface (what proves it landed)

- **`reify check`/`build` of the curated-fillet idiom** (`let e = edges_at_height(b,h,tol); let f = fillet(b,e,2mm)`) produces a non-`Undef` Solid whose recorded `GeometryOp::Fillet{edges,…}` has `edges.len()==4` and whose volume ≠ the all-edges fillet — under the `UnifiedDag` scheduler. (`fillet_curated_edges_3205_e2e`.)
- **`reify check examples/process/std_process_dfm.ri`** reports a **definite** `Satisfied`/`Violated` for `fits_build_volume` (flips when the part exceeds the envelope), not `Indeterminate`. (`dfm_fits_build_volume_4275_e2e` + the task-4275 CLI example.)
- **A genuine dependency cycle of any node-kind pair** yields a clear `E_EVAL_CYCLE` diagnostic naming the cycle members in a deterministic ordered path — never a silent `Undef`, never a hang.
- **A geometry-backed constraint that constrains an auto** yields `E_EVAL_UNRESOLVED` (declined-to-solve), not a hang and not a false cycle.

## 3. Sketch of approach (the five deliverables, condensed — full mechanics in the design doc §3–§7)

### 3.1 Spine (`engine_fixpoint.rs`, NEW)
Online Kahn worklist over the existing `ReverseDependencyIndex` (O(V+E)), **not** `compute_levels` (Value-only-decrement, quadratic — explicitly disowned). One **coarse** `NodeId::Realization` per multi-op realization (preserves the verified atomic rollback `handle_start`→`truncate`, the `RealizationCache` 1:1 key, and avoids the `&mut self` kernel-reentrancy hazard). **Readiness gate = `DeterminacyState::Determined`, not map-presence** (auto-settle becomes a real edge). **Realization is eager-over-reachable** (cold `build()` seeds demand with all realizations — byte-preserving today's eager kernel-error surfacing; *not* lazy DCE). Determinism rides `BTreeSet<DebugOrd> pop_first` + content-derived `NodeId`s.

### 3.2 Three first-class geometry-path executors (the corrected "not bodies-unchanged")
1. **Realization executor** — wraps `execute_realization_ops` (rollback verbatim) then a **per-realization slice** of `post_process_geometry_handle_cells` hydrating *that* realization's `Value::GeometryHandle` inline (folds P3 at the realization slot).
2. **Selector/query-cell executor** — a **per-cell slice** of `post_process_topology_selectors` / `try_eval_geometry_query`, run with the live kernel + `named_steps` at the cell's scheduled slot.
3. **Constraint executor** — engine-side `rewrite_geometry_queries` (new arm, §3.3) folds geometry-query leaves to literals, then the unchanged kernel-less `SimpleConstraintChecker`. **No trait break** (the engine resolves geometry before the trait boundary).

Kahn edges: `body-realization → output-cell hydration → selector-cell → consumer-realization`.

### 3.3 Constraints obtain geometry engine-side (two required additions)
- **New `FunctionCall`-args recursion arm** in `rewrite_geometry_queries` (today falls through `_ => expr.clone()` at `:1942`, so an outer non-query call like `fits_build_volume(bounding_box(..), …)` never folds its inner `bounding_box` leaves). Recurse into each argument subtree, leaving the outer call intact for the checker.
- **Cross-sub / `IndexAccess`-on-`StructureRef` resolution** in `resolve_geometry_handle_arg` (today matches **only** `CompiledExprKind::ValueRef`, so `bounding_box(proc.build_volume)` folds to `Undef`). Required for the 4275 `let proc = FdmPrinter()` form to fold its second leaf.

### 3.4 Cycle contract (load-bearing acceptance bar)
**Stage A — Kahn residue (hang-proof):** in-degree counts **all** edge kinds; cyclic nodes never pop (never executed, no kernel call, no handle insert); O(V+E), no fixpoint → a cycle cannot hang. **Stage B — Tarjan SCC discriminator** over the residue: `|SCC|>1` or singleton-with-self-edge → `E_EVAL_CYCLE` (one per SCC, ordered path via DFS-on-stack confined to the SCC); singleton no-self-edge → stranded-downstream, left `Undef` (no false cycle). `NodeId::describe()` names every kind. Missing producer and failed realization are **not** residue cases. Determinism: Tarjan outer iteration + successors in `DebugOrd` order → byte-identical diagnostic vector.

### 3.5 C7 retirement + the auto-constraint guard
- **C7 (frozen `constraint_results`) is retired in the unified path:** drop the pre-geometry constraint dispatch from `check()`; run **all** constraints as worklist nodes after their geometry deps; `BuildResult` writes driver-computed results. The kernel-less `check()` path stays for `reify check` / no-kernel callers.
- **Geometry-backed-constraint-on-auto:** compute a **transitive auto-read closure** (constraint.reads → backing realization → its value/realization reads, with the `:394` fix); when a geometry-backed constraint transitively reads an auto, emit `E_EVAL_UNRESOLVED` and **decline to solve that class** (a genuine geometry-in-the-loop solve contradicts "one forward pass per scope" and is out of scope). Legacy degrades identically, so the differential corpus must carry this case explicitly.

### 3.6 Staged, flagged rollout
`enum BuildScheduler { LegacyMultiPass, UnifiedDag }` (default **UnifiedDag** as of Stage-4 #4362; `REIFY_BUILD_SCHEDULER=legacy` is the one-release kill-switch; Stage-5 deletion pending #4727) + `feature = "unified-dag"` (vestigial — no longer gates `from_env`; pending Stage-5 removal #4727). Stages map to the decomposition (§8): driver+cycle (δ) → executors+resolution+C7 (ε) → differential corpus (ζ) → acceptance (η) → warm/incremental (θ) → cutover+legacy delete (ι).

## 4. Resolved design decisions

- **D1 — Coarse node, not per-op.** Preserves atomic rollback, the cache key, and the `&mut kernel` borrow discipline (taken only at a realization's scheduled slot). The lazy/pull `force()` model (candidate A2) is rejected — it holds `&mut` across a re-borrow.
- **D2 — Eager-over-reachable, not lazy DCE.** Cold `build()` seeds all realizations to byte-preserve eager declared-but-unconsumed-geometry error surfacing.
- **D3 — Readiness = `Determined`, not map-presence.** Makes auto-settle a structural edge.
- **D4 — C7 retired by dropping pre-geometry dispatch entirely** (cleanest precedence: no double-eval, no "which result wins"). `check()` stays kernel-less for `reify check`.
- **D5 — Auto-constraint class declines to solve** with `E_EVAL_UNRESOLVED` (not a hang, not a false `E_EVAL_CYCLE`, not a silent `Undef`).
- **D6 — 3205 in-loop greenness is *downstream* of this driver, not upstream.** The re-scoped task 3205 delivers the scheduling-independent **machinery** (3-arg IR + `resolve_subhandle_list` + per-edge FFI + `E_EMPTY_SELECTION`, unit-green against pre-resolved handles); the **in-loop e2e** (`fillet_curated_edges_3205_e2e`) is green only under `UnifiedDag` and lives here as η. (Corrects the design doc's "land 3205 green in-loop first" — §1 proves that idiom cannot dispatch on legacy, so its greenness cannot be a legacy prerequisite. The dependency was inverted.)
- **D7 — Warm/incremental unification is its own stage (θ).** `eval_cached`/`concurrent` wave-2 are purely expression-level (zero references to the kernel/`named_steps`/realization stack); threading that stack in is a separate migration with its own gated corpus. Until θ lands, the 3205/4275 *folding* is cold-path-only and the corpus must not assume warm == cold for those cases.
- **D8 — Incremental realization eviction scoped to value cells for initial landing.** `compute_dirty_cone_with_realizations` is dead code; `diff_realizations` keys on the static IR hash (never moves on a value-driven geometry change); `edit_param`/`edit_source` full-flush `clear_realization_cache()`. θ keeps full-flush cold re-execution for realization warm-correctness; selective realization eviction (an *executed-result* hash) is deferred to a follow-up.

## 5. Pre-conditions for activating

- **Part 1 merged** (`engine-build-dag-substrate.md`): the populated edge graph (α) + `assert_dag_complete` (β) + `cell_eval_ctx` (γ). **Hard prerequisite** — the driver rides the edge graph; building it against an incomplete graph is the red-team's #1 design-breaker.
- **Re-scoped task 3205 machinery merged**: 3-arg `Fillet` IR (main is still 2-arg — `reify-ir/src/geometry.rs:585`), `resolve_subhandle_list`, per-edge `make_fillet_with_history` FFI, `E_EMPTY_SELECTION` — all unit-green against pre-resolved handles. **Hard prerequisite for ε/η.**
- **Substrate confirmed present** (design ledger C1–C15): `realization_index`, `NodeId` kinds, `DeterminacyState`, `#[non_exhaustive]` `DiagnosticCode`, kernel-less `SimpleConstraintChecker`. New `EvalCycle`/`EvalUnresolved` codes are additive.
- **No grammar change** — G3 grammar-gate N/A (`fillet(b,e,r)`, `edges_at_height`, `fits_build_volume` all already parse).
- **θ-only:** if a multi-kernel module enters the warm test set, the cross-kernel `GeometryHandleId` collision (tasks 4349/4351) may need the KernelHandle re-key first; the per-kernel table reset must stay per-build.

## 6. Contract + two-way boundary tests (H component)

**Contract — scheduler equivalence on the overlap domain.** For every build in the existing corpus, `UnifiedDag` produces a `BuildResult` **equivalent** to `LegacyMultiPass`, except for an explicit **per-case, reasoned allow-list** of legitimate divergences (the 3205/4275 cases + newly-resolved-not-`Undef` constraints). Running `UnifiedDag` twice on any input is **byte-identical** (determinism). Stage-1 adds a `residue == ∅` gate on every acyclic legacy-passing case (any false `E_EVAL_CYCLE` or stranded-without-SCC node = a worklist-totality bug).

**Cycle-contract acceptance bars (faces the consumer = anyone who writes a cyclic/degenerate program):**

| Scenario | Postcondition |
|---|---|
| Per-kind-pair cycle (param↔let; geom↔constraint; selector↔op; realization↔realization via `GeomRef::Sub`; resolution-involved value cycle) | one `E_EVAL_CYCLE` per SCC, ordered path naming members |
| Self-loop `let x = x` | `E_EVAL_CYCLE` (singleton + self-edge) |
| Two disjoint cycles | two deterministically-ordered diagnostics |
| Missing producer (unbound ref) | normal unbound/`Undef` — **not** a cycle |
| Failed realization (kernel error) | geometry-error `Diagnostic`; downstream `Undef` — **not** a cycle |
| Geometry-backed constraint on an auto | `E_EVAL_UNRESOLVED` — not a hang, not a false cycle |
| Determinism | 100× run → byte-identical diagnostic vector; every genuine-SCC node left unexecuted (no kernel call, no handle insert) |

**Differential-corpus boundary cases (faces the producer = the driver; legacy-vs-unified diff alone cannot surface these because legacy degrades identically):**

| Case | Why it needs explicit coverage |
|---|---|
| auto + geometry-constraint | both schedulers Indeterminate today → `E_EVAL_UNRESOLVED` must be asserted directly |
| cross-sub multi-body assembly (parent named lexicographically before children) | `DebugOrd` pop order can mis-export without the §3.1 edges |
| warm-path `let y = auto_x + N` | the warm Resolution executor must back-prop solved autos (the no-op `SolveResult::Solved` arm) |
| warm determinacy-predicate let via `edit_param`/`edit_source`/`eval_cached`/`concurrent` | the 4317-twin parity (Part 1 γ provides the baseline) |
| multi-realization snapshot export | `build_snapshot`'s `step_handles.last()` (`:2100`) export must move to positional terminal handles |
| 4275 let-bound form (`let proc = FdmPrinter()`) | cross-sub leaf folding (§3.3) — assert a *definite* verdict |

The η acceptance task names `fillet_curated_edges_3205_e2e` + `dfm_fits_build_volume_4275_e2e`; ζ names the full differential corpus + these boundary cases — closing G2's loop.

## 7. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `engine-build-dag-substrate.md` (Part 1) | consumes | edge graph + `cell_eval_ctx` | Part 1 | hard prereq |
| task 3205 (re-scoped, `geometry-modify-sweep-completion.md`) | consumes | curated-fillet **machinery** (3-arg IR, `resolve_subhandle_list`, per-edge FFI, `E_EMPTY_SELECTION`) | task 3205 | hard prereq for ε/η |
| task 4275 (`process-dfm-completion.md`) | produces-for | the post-geometry constraint re-check that flips `fits_build_volume` to definite | **this PRD (ε)** | 4275 depends on ε; 4275 keeps its CLI example + doc reconcile |
| tasks 4349/4351 (cross-kernel `KernelHandle` re-key) | consumes (θ only) | `FeatureTagTable`/`TopologyAttributeTable` keying under cross-kernel warm interleave | tasks 4349/4351 | θ pre-condition if a multi-kernel module enters the warm corpus; G4 seam with 4262 |

Seam-ownership resolved (no reciprocal ambiguity): the **3205 in-loop e2e** is owned **here** (η), the **machinery** by re-scoped 3205; the **4275 engine fix** is owned **here** (ε), the **4275 example/doc** by task 4275.

## 8. Decomposition plan

DAG: **δ → ε → {ζ, η}**; **η → θ → ι**. δ depends on Part 1-α/β; ε additionally depends on Part 1 + re-scoped 3205 machinery. Task 4275 gains a dep on ε.

- **δ — `engine_fixpoint.rs` worklist driver + cycle contract** (behind `UnifiedDag` flag, default off). Online Kahn over `ReverseDependencyIndex`; `Determined` gate; uniform in-degree/decrement over all edge kinds; Tarjan-SCC residue + ordered-path; `NodeId::describe()`; additive `EvalCycle`/`EvalUnresolved` codes; Stage-1 `residue == ∅` gate. *Modules:* `crates/reify-eval/src/engine_fixpoint.rs` (NEW), `cache.rs` (`describe`), `crates/reify-core/src/diagnostics.rs`. *Signal:* **leaf+intermediate** — the cycle-contract acceptance bars (§6) emit real `E_EVAL_CYCLE`/`E_EVAL_UNRESOLVED` diagnostics, deterministic; also unlocks ε. *Prereq:* Part 1-α, Part 1-β. *grammar_confirmed: true.*
- **ε — three geometry-path executors + `rewrite_geometry_queries` arm + cross-sub `resolve_geometry_handle_arg` + C7 retirement + auto-constraint guard** (under flag). `build()` per-template loop replaced by the driver call invoking the Realization wrapper (rollback verbatim) + per-realization hydration slice + per-cell selector/query slice; `seed_cross_sub_named_steps` in the Realization-executor prelude; `BuildResult` writes driver-computed `constraint_results`. *Modules:* `crates/reify-eval/src/engine_build.rs`, `geometry_ops.rs`, `engine_eval.rs`. *Signal:* **intermediate** — unlocks ζ/η; directly: a geometry-backed constraint re-checks post-geometry (no longer frozen-Indeterminate). *Prereq:* δ, Part 1, re-scoped 3205 machinery. *grammar_confirmed: true.*
- **ζ — differential corpus (Stage 2).** Full `reify-eval/tests/` + `tests/golden` under both schedulers; `BuildResult` equivalence on the overlap with a per-case reasoned allow-list; the §6 expanded boundary cases; run unified 2× byte-identical. *Modules:* `crates/reify-eval/tests/`. *Signal:* **leaf** — corpus green under both schedulers + 2× byte-identical unified; committed allow-list. *Prereq:* ε. *grammar_confirmed: true.*
- **η — unified-only acceptance (Stage 3).** `fillet_curated_edges_3205_e2e` (curated fillet records 4 edges, volume ≠ all-fillet) + `dfm_fits_build_volume_4275_e2e` (definite Satisfied/Violated, flips on envelope), `#[cfg_attr(not(feature="unified-dag"), ignore)]`. *Modules:* `crates/reify-eval/tests/`. *Signal:* **leaf** (the headline integration gate) — both e2e pass under the unified flag. *Prereq:* ε, re-scoped 3205 machinery. *grammar_confirmed: true.*
- **θ — warm/incremental unification (its own stage).** Route `build_snapshot`/`tessellate_from_values`/`eval_cached`/`concurrent` through the demand-scoped driver; **fix `build_snapshot` `step_handles.last()` → positional terminal handles**; warm Resolution executor back-props solved autos + re-dirties downstream lets; value-cell-scoped incremental (full realization flush retained); re-verify concurrent intra-level realizations don't share a `named_steps` namespace. *Modules:* `crates/reify-eval/src/engine_build.rs`, `engine_eval.rs`, `concurrent.rs`, `engine_edit.rs`. *Signal:* **leaf** — the §6 warm boundary cases pass (warm `let y = auto_x + N`; warm determinacy-predicate let; multi-realization snapshot export). *Prereq:* η. *grammar_confirmed: true.*
- **ι — cutover + legacy removal (Stages 4–5).** ✅ **Stage 4 landed (#4362).** Default is now `UnifiedDag`; `REIFY_BUILD_SCHEDULER=legacy` is the one-release kill-switch. Stage 5 (delete legacy loop bodies, the `BuildScheduler` enum, `detect_let_cycle`'s let-local body, `from_env_value` machinery, `set_build_scheduler` seam, and the `unified-dag` feature def) is deferred to #4727. *Modules:* `crates/reify-eval/src/`. *Signal:* **leaf** — Stage-4: default is `UnifiedDag` ✅; Stage-5: legacy paths removed (pending #4727). *Prereq:* θ + operational green-CI confidence (human-gated). *grammar_confirmed: true.*

## 9. Out of scope for this PRD

- **Geometry-in-the-loop solver rounds** (a constraint that must drive an auto *through* realized geometry) — declined-to-solve with `E_EVAL_UNRESOLVED`; a future PRD if demanded.
- **Selective realization eviction** (executed-result-hash-driven incremental realization dirty-prop) — θ keeps full-flush; a follow-up after `RealizationNodeData` result hashing exists.
- **The cross-kernel `KernelHandle` re-key** (tasks 4349/4351) — referenced as a θ pre-condition, owned there.
- **Part 1's edge graph + `cell_eval_ctx`** — prerequisite, not built here.

## 10. Open questions (tactical; not blocking)

1. **`reify check` vs `build` divergence for geometry-backed constraints.** `check()` stays kernel-less → reports geometry-backed constraints `Indeterminate`; `build()` (unified) reports definite. **Suggested resolution:** treat as an intended, documented contract (kernel-less check cannot resolve geometry by construction); document it in the `reify check` help/spec during ε. Decide during ε.
2. **`N` for the Stage-4 default flip.** How many green CI runs gate ι's cutover. **Suggested:** a fixed count (e.g. 2 weeks of nightly green or N=20 CI runs) + a human go/no-go (ι is human-gated). Decide at θ-completion.
3. **`assert_dag_complete` retirement.** Once the unified path is default (ι), whether the Part 1 debug assertion stays as a permanent invariant check or is removed with the legacy loop. **Suggested:** keep it (cheap insurance against future edge-extraction regressions). Decide at ι.
4. **Concurrent intra-level realization serialization.** If θ's re-verification finds two realizations sharing a `named_steps` namespace can land in one Kahn level, serialize realization nodes within a level. **Suggested:** serialize conservatively; measure. Decide during θ.

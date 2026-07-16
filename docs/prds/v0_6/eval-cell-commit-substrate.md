# Eval cell-commit substrate

Status: **deferred** (hotspot-program hardening batch, survey §H1). Milestone v0_6. Authored 2026-07-06 in interactive `/prd` session; scope ratified by Leo 2026-07-06 ("hardening first P1→P5; P6 folded into the executor, NOT standalone"). Approach **B + H** (contract + two-way boundary tests).

Owner of invariants **INV-EVAL-1, INV-EVAL-2, INV-EVAL-3, INV-EVAL-4** (`docs/invariants.md`).

## §0 — Purpose

The per-cell **evaluate-and-commit** transaction — build a cell eval context → `eval_expr` → choose a determinacy rule → write values + snapshot → record cache → record journal — has **no primitive**. It is re-inlined ~15 times across four live eval paths (`eval`, `eval_cached`, `edit_param`, guarded-group/`unfold`) and two dead ones. This is the structural cause of the repo's **#1 recurring bug class**: the *same* bug fixed once per path (4317→4332, 4356; 2266; 2259/2267; 2195). Every fix strengthens one copy; the other copies silently keep the defect until a user trips over each in turn.

This PRD introduces the missing primitive and the required-capability eval context, migrates every transaction copy onto them, makes the three implicit determinacy rules and the currently-unrecorded cache legs **explicit and typed**, and reduces the parity surface by deleting the dead concurrent stack. It is the **hardening prerequisite** the ratified unified-DAG-executor milestone was sequenced behind (survey §"cross-cutting themes" #6; `eval-uniform-dependency-handling.md`).

Provenance: `docs/notes/bug-hotspot-survey-2026-07-05.md` §H1 (path-inventory table, file:line anchors) + §"latent bugs" + §"cross-cutting themes"; the 2026-07-05 decisions ledger (fused-memory, `agent_id=hotspot-program-planning`).

## §1 — Consumer + user-observable surface (G1)

Every mechanism this PRD introduces has a named consumer:

- **`commit_cell_result` primitive** — consumed by **every migrated eval path in this PRD** (§7 boundary tests), and by the **async-recalc Phase A** feature (bookmark task **#5023**), whose completion transaction *is* a cell-commit ("record result + freshness + cache" in one critical section). #5023's own description names "eval cell-commit primitive (INV-EVAL-1)" as a dependency to wire; the cross-edge is wired in decomposition.
- **`CellEvalCtx` required-args constructor** — consumed by every migrated ctx site; replaces the partially-adopted `cell_eval_ctx` (task 4356, done: adopted at only a handful of the ~86 `eval_ctx_with_meta` sites as of the 2026-07-05 survey).
- **Diagnostics-in-cache + shared detector registry** (INV-EVAL-3) — consumed by `reify check` / LSP fast-path serves: a runtime warning that today is swallowed on a cache hit surfaces on a repeated evaluation of unchanged content.
- **snapshot↔cache divergence audit** (INV-EVAL-4) — consumed by the verify pipeline (runs green over `examples/`), complementary to the landed `check_no_stale_undef` checker (task 4952, INV-EVAL-5 — a *distinct* invariant).
- **One ordering core** (P5) — `dirty.rs`'s flat sort (`topological_sort`/`compute_eval_set`) **converges onto** the `engine_fixpoint` Kahn core (`run_unified_pass_seeded`); the vestigial level-batched `compute_levels` retires with the dead concurrent stack (ο). A **result-preserving order change** — the two sorts genuinely diverge (level-batched vs global-priority; Value-only vs realization-aware), so *not* byte-identical — reducing the surface where scheduling-order bugs (INV-EVAL-5 class) hide.

Downstream milestone consumer (prose, not a hard edge): the **unified DAG executor** migration (`docs/prds/v0_6/eval-uniform-dependency-handling.md`; sequencing task #4727). P1/P2 were explicitly ratified as its hardening enablers.

**User-observable surface.** A user running `reify check` (or the LSP) twice on unchanged source sees a runtime warning (e.g. field-index-out-of-bounds) that previously vanished on the second, cache-served evaluation; the verify pipeline gains a snapshot↔cache agreement gate; the concurrent-stack deletion is observable as a green build with re-homed test pins accounted for.

**Engine-integration seam (G1 sub-check).** No new seam is introduced. The primitive and ctx constructor sit *inside* the existing eval walk — they refactor the body of paths that already plug into the engine's evaluate/edit entry points (`engine-integration-norm.md` §3.6 freshness-only walk is the adjacent catalogued seam that P5's ordering core and §"Freshness walk" touch; the freshness walk itself is **not** rewired here — see §9).

## §2 — Contract (H)

### §2.1 `commit_cell_result`

A single primitive performs the four legs of a cell-eval commit atomically (no path can perform three of four):

```rust
/// Commit one cell's evaluation result across all four legs in one place.
/// There is no way to write a subset by omission — a skipped leg is an
/// explicit `CacheLeg::Skip`, visible and revisitable.
pub(crate) fn commit_cell_result(
    &mut self,
    node: ValueCellId,
    value: Value,
    determinacy: DeterminacyRule,
    trace: TraceSource,
    version: VersionId,
    cache_leg: CacheLeg,
) -> CommitOutcome;
```

- **values leg** — insert `(value, determinacy-derived DeterminacyState)` into the live `values` map.
- **snapshot leg** — insert the same into `snapshot.values`.
- **cache leg** — per `CacheLeg` (below).
- **journal leg** — emit the `Started`/`Completed` EvalEvent pair (the task-2195 helper `record_eval_completed`, engine_eval.rs:369, adopted at only 5 sites, is the internal implementation and is subsumed).

`CommitOutcome` carries the committed `(Value, DeterminacyState)` so callers that previously read back the inserted tuple keep working. **Name note:** the return type is `CommitOutcome`, *not* `EvalOutcome` — `reify_eval::cache::EvalOutcome` (variants `Changed`/`Unchanged`) already exists and would collide (confirmed against current main).

### §2.2 `DeterminacyRule` — the three implicit rules, made explicit

Today three determinacy behaviours are chosen implicitly and inconsistently across paths (the intentional divergence documented in `reeval_cone_cell`'s `# Determinacy` doc section at `engine_eval.rs:4883-4897` — "A future reader should NOT change this to match the main-pass unconditional Determined rule" — and the solver version-space rule). They become an explicit input the caller **must** pass — the divergences are **encoded, not erased**:

```rust
pub enum DeterminacyRule {
    /// Always `Determined` regardless of value (plain lets, param binds).
    UnconditionalDetermined,
    /// Derive `Determined`/`Undetermined` from the value + predicate
    /// (DeterminacyPredicate cells; the 4317/4356 family).
    DeriveFromValue,
    /// Always `Undetermined` (rejected-override-no-default; solver-owned
    /// version-space cells awaiting solve).
    Undetermined,
}
```

The migration must **preserve each site's current rule** (characterization-first): a site that stamps `(val, Determined)` today passes `UnconditionalDetermined`; the DeterminacyPredicate sites pass `DeriveFromValue`; rejected-override / awaiting-solve sites pass `Undetermined`. No behaviour change at migration time — the point is to make the rule a typed, non-defaultable argument so a future edit cannot silently drop determinacy (the `4317`-twin at `engine_eval.rs` DeterminacyPredicate None-branch).

### §2.3 `TraceSource`

The provenance tag the journal/trace records for this commit (which path produced it: cold eval, cached serve, edit re-eval, guarded-group, post-pass overwrite). Makes the journal self-describing across paths and lets the divergence audit (§2.6) attribute a mismatch to its producing path.

### §2.4 `CacheLeg` — unrecorded sites become explicit decisions

```rust
pub enum CacheLeg {
    /// Record a cache entry for this cell (the default for recorded sites).
    Record,
    /// Deliberately do NOT record (documented reason). Replaces today's
    /// silent omissions (e.g. cyclic cells per task 2266; the S4
    /// rejected-override path per task 2195).
    Skip(&'static str),
}
```

Every currently-unrecorded transaction site is audited during migration and re-expressed as `CacheLeg::Skip("<reason>")` — the omission becomes visible in the source and revisitable, instead of being an accident of control flow. This directly closes the 2266 ("cyclic let-cells write garbage to cache") / 2195 ("Param-cell evals invisible to journal/cache") class.

### §2.5 `CellEvalCtx` — required-capability constructor

The `EvalContext` builder (`reify-expr/src/lib.rs:46-94`) is an optional-capability builder where every missing capability degrades **silently** (missing determinacy → silent `Undef` at lib.rs:946-947; missing runtime-diagnostics sink → dropped warnings). The existing partially-adopted helper is `cell_eval_ctx` (engine_eval.rs:4852, only 4 of ~86 `eval_ctx_with_meta` sites as of the 2026-07-05 survey). The replacement constructor makes the load-bearing capabilities **required arguments**, as a **free function** taking `functions` and `meta_map` as parameters (which kills the borrow-scope excuse documented in the `cell_eval_ctx` doc-comment at `engine_eval.rs:4832-4862` — "build inline … for borrow-scope reasons … omitting it only causes restricted-field samples to return `Value::Undef`" — that motivated leaving the method form partially adopted):

```rust
/// The ONLY sanctioned way to build a cell eval context inside the engine.
/// Determinacy predicate, runtime-diagnostics sink, and containment are
/// REQUIRED — there is no builder path that omits them.
pub(crate) fn cell_eval_ctx<'a>(
    values: &'a ValueMap,
    functions: &'a FunctionTable,
    meta_map: &'a MetaMap,
    determinacy: &'a DeterminacyPredicate,   // required
    runtime_sink: &'a mut RuntimeDiagnosticSink, // required
    containment: Containment,                // required
) -> EvalContext<'a>;
```

Signatures above are the **design intent**, not a frozen API — the architect adjusts argument shapes to the real borrow structure, but the invariant (**determinacy + sink + containment are non-omittable**) is fixed.

### §2.6 snapshot↔cache agreement audit (INV-EVAL-4)

A debug/verify-gate harness asserts: **for every cell in `snapshot.values`, a cache entry exists whose content-hash matches the snapshot value** (modulo explicit `CacheLeg::Skip` cells, which are exempt by construction). Ships **warn-first** to enumerate today's divergences over the `examples/` corpus, then flips to **assert** once §7's migration + post-pass routing eliminate them. This is the cheap structural form of the no-stale invariant; it is **complementary** to the landed `check_no_stale_undef` (4952) — that checks *stale Undef*, this checks *content-hash agreement*; do not merge them.

### §2.7 Diagnostics as data (INV-EVAL-3)

`NodeCache` entries gain a per-node diagnostics vector (constructor takes an explicit, possibly-empty vec — no path can forget it). A **single shared post-pass detector registry** runs identically on `eval`, `eval_cached`, and `edit_check`, replacing the **cold-only detector asymmetry**: today post-passes such as the annotation-args materialization eval driver (`engine_eval.rs:4357`, #3556) run on the cold `eval` path but are **not** run by `eval_cached` (comment at :4373), and detector ordering is pinned by scattered "must run before …" convention comments (e.g. :6269) rather than a single registry. On a fast-path (cache-hit) serve, the cell's stored diagnostics are **replayed**. **Ownership rule (double-emission guard):** each diagnostic has exactly one owner per serve — **replayed XOR freshly-pushed**, never both.

## §3 — Sketch of approach (scope, per Leo's ratification)

1. **P1 (INV-EVAL-1, INV-EVAL-2).** Build `commit_cell_result` + `DeterminacyRule`/`TraceSource`/`CacheLeg`/`EvalOutcome` (§2.1–2.4) and the required-args `cell_eval_ctx` (§2.5). Migrate all transaction + ctx copies, **split into narrow per-file tasks** (engine_eval.rs sites incl. `reeval_cone_cell` and the annotation pass / engine_edit.rs `edit_param` sites / unfold.rs). Currently-unrecorded sites become explicit `CacheLeg::Skip`.
2. **P3 (INV-EVAL-4).** Route `eval`'s structural-query/self-datum post-pass overwrites (`engine_eval.rs:3454-3463` region) through the primitive (part of the engine_eval.rs migration), and add the §2.6 divergence audit (warn→assert). Complementary to 4952; dup-checked.
3. **P4 (INV-EVAL-3).** Diagnostics-in-cache (§2.7): attach per-node diagnostics to `NodeCache`, replay on fast-path serves, single shared detector registry across eval/eval_cached/edit_check.
4. **P5 (converge, don't preserve — corrected 2026-07-06 per esc-5045-29).** `dirty.rs`'s flat topological sort (`topological_sort`:153 / `compute_eval_set`:236) **delegates to** the `engine_fixpoint` Kahn core (`run_unified_pass_seeded`, engine_fixpoint.rs:273), adopting its **global-priority, realization-aware** order; the level-batched `compute_levels`:171 is **retired** (its only non-vestigial consumer is `topological_sort`; its `Vec<Vec>` level output feeds only the dead concurrent stack that ο #5065 deletes). `resolve_order` stays separate (different domain). This is a **result-preserving order change**, *not* byte-identical: the two sorts provably diverge on two axes — level-batched-drain vs `pop_first`, and Value-reads-only vs `reads`+`realization_reads` (in-tree counterexample `[a,z,b,c]` vs `[a,b,c,z]` at `unified_dag_edit_path.rs:150-190`). Correctness is pinned by INV-EVAL-5's registry definition (valid topological order; no-stale-Undef checker, task 4952) + final-result equivalence on the differential corpus — **not** a byte-identical old==new schedule (which is unsatisfiable; the edit path already runs the Kahn order per θ2 #4531). **Sequenced after ο** (dead-stack deletion) so `compute_levels` retires cleanly.
5. **Dead-path deletion.** DELETE the concurrent stack — `crates/reify-runtime/src/{concurrent,concurrent_eval}.rs` + `crates/reify-eval/src/concurrent.rs` (zero production callers; the wave-1 adapter carries an **unfixed** 4356-class bug at `concurrent_eval.rs:399-410` that must be **deleted, not fixed**). Re-home shared-property tests (the acyclic-linear-schedule pins in `reify-eval/tests/concurrent.rs`) onto the live code they pin **first**.
6. **Freshness walk.** **KEEP** `propagate_freshness_only` (engine_admin.rs:2075 method / freshness_walk.rs:130 free fn). Relocate its invariant warning to the load-bearing end — a `// TODO(#5023): async completions must invalidate dependents` breadcrumb at `run_compute_dispatch` (engine_compute.rs:177), citing #5023 (async-recalc Phase A, its named production consumer) in proper PTODO form.

## §4 — Resolved design decisions

1. **The primitive owns all four legs; skips are typed.** No `commit_*_partial` variants — a subset write is a `CacheLeg::Skip("reason")`, never an omission. (Closes the extract-but-abandon anti-pattern: "an extraction isn't done until adoption is total and the old shape is unrepresentable.")
2. **Determinacy is a required, non-defaultable argument** carrying the three rules as explicit variants. The intentional divergences (engine_eval.rs:4747-4755; solver version-space) are **encoded** as `DeriveFromValue`/`Undetermined`, not flattened.
3. **`cell_eval_ctx` is a free function with required capability args**, taking `functions`/`meta_map` as parameters — the borrow-scope excuse (engine_eval.rs:4690-4707) is dissolved, not worked around.
4. **Migration is characterization-first / behaviour-preserving.** Each site keeps its current determinacy rule and cache-recording decision; the change is *typing* the decision, not altering it. Divergences surface as `CacheLeg::Skip` reasons and the §2.6 audit, not as silent behaviour flips.
5. **The divergence audit ships warn-first, then asserts** (per the registry's ratified rollout: contract → warn-mode corpus sweep → fix bulk producers → flip to enforce, with a break-glass env knob mirroring the main-gate ENFORCE/BYPASS pattern).
6. **The dead concurrent stack is deleted, not fixed.** Its wave-1 adapter's 4356-class bug is not a defect to repair — the whole stack has zero production callers. Tests that pin *real* shared properties are re-homed onto live code first; tests that only exercised the dead adapter are dropped (accounted for in the test-count parity signal).
7. **`edit_source` is neither deleted nor migrated now** — it is a product question (is warm source-edit latency a current pain?). Filed as a **pending capstone `[MILESTONE]`** task (§8 π, §9) gated on the P1 slice: on dispatch it escalates for Leo's decision — wire-after-P1 if yes, delete if unanswered by 2026-08-06. (Pending-milestone SOP, not a parked bookmark — the scheduler surfaces it the moment P1 lands.)
8. **P6 (eval/eval_cached merge) is out of scope** — folded into the unified-DAG-executor milestone per Leo. P1/P3/P4 are its hardening enablers.

## §5 — Pre-conditions for activating

- **Substrate exists today (G3, re-verified against current main HEAD `4d696e63` on 2026-07-06):** `commit_cell_result`, `DeterminacyRule`, `TraceSource`, `CacheLeg` are **new** (confirmed not present). The return type is `CommitOutcome` because `EvalOutcome` **already exists** (`reify_eval::cache::EvalOutcome`, Changed/Unchanged) — a same-name type would collide. The migration targets all exist: `reeval_cone_cell` (engine_eval.rs:4918), the annotation-args post-pass (engine_eval.rs:4357), the self-datum post-pass (engine_eval.rs:3451-3473), the transaction copies across engine_eval.rs / engine_edit.rs (e.g. :953/:1004, :1176/:1357, :2907/:2920, :3089/:3336) / unfold.rs (:337-367, :570-602), `run_unified_pass_seeded` (engine_fixpoint.rs:273), `dirty.rs` `topological_sort`/`compute_levels` (:153/:171; `resolve_order` lives separately in resolve_order.rs:302), the concurrent stack files (`reify-runtime/src/{concurrent,concurrent_eval}.rs`, `reify-eval/src/concurrent.rs`), `propagate_freshness_only` (engine_admin.rs:2075 method → freshness_walk.rs:130 free fn), `run_compute_dispatch` (engine_compute.rs:177, confirmed to contain **zero** `propagate_freshness_only` calls). No novel `.ri` syntax anywhere — the grammar gate is a **no-op** for this PRD.
- **No hard upstream PRD dependency.** This PRD is a leaf-hardening batch; it does not wait on the unified-DAG executor (it precedes it).
- **Cross-edge:** async-recalc Phase A (#5023) is wired `depends_on` the P1 primitive task (its completion transaction consumes it).

## §6 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| async-recalc Phase A (`docs/prds/async-recalc-phase-a.md`, bookmark #5023) | consumes | `commit_cell_result` (completion transaction) | this PRD (produces primitive); #5023 wires the completion call | edge wired: #5023 → dep on task α |
| `engine-build-hardening` (concurrent session; INV-BUILD-2 owner) | coordinates | version-id **allocator** removes one bump site living in `concurrent.rs` | engine-build-hardening owns the allocator | **note**: deleting the concurrent stack (task ο) *removes* that bump site — one fewer migration site for them, not a blocker either way. Noted in ο's description. |
| `god-file-decomposition` (concurrent session) | coordinates | test-eviction of `engine_build.rs` | god-file-decomposition | no overlap — this PRD does not touch engine_build.rs |
| `eval-uniform-dependency-handling.md` / unified-DAG executor (#4727) | produces-for | P1/P3/P4 harden the eval paths it later unifies (P6) | that PRD owns the executor | prose consumer; no hard edge (deletion/migration precede it) |
| task 4952 (`check_no_stale_undef`, done) | complements | INV-EVAL-5 vs this PRD's INV-EVAL-4 divergence audit | 4952 (landed) | complementary; audits are distinct — do not merge |

No new contested-ownership pair is introduced (the three known contested seams in the overlay are all geometry/multi-kernel; none touched here).

## §7 — Boundary-test sketch (H, two-way)

Each row faces both the **producer** (the primitive/ctx) and a **consumer** (a migrated path). These become the observable signals of the migration leaves (closing G2).

| # | Scenario | Preconditions | Postcondition (asserted) | Faces |
|---|---|---|---|---|
| B1 | Cache-swallowed warning resurfaces | a cell emits a runtime warning (field-index-OOB) on cold eval; unchanged source re-evaluated (cache hit) | the warning **is re-emitted** on the second `reify check` / LSP eval (replayed from NodeCache) | producer: diagnostics-in-cache; consumer: fast-path serve |
| B2 | Determinacy preserved across paths | a DeterminacyPredicate cell + a plain let + a rejected-override cell | `eval`, `eval_cached`, and `edit_param` all commit **identical** `(value, DeterminacyState)` for each (via `DeriveFromValue` / `UnconditionalDetermined` / `Undetermined`) | producer: `DeterminacyRule`; consumers: all migrated paths |
| B3 | Skip legs are explicit & audited | a cyclic let-cell (2266 shape) | migrated site records `CacheLeg::Skip`; the divergence audit **exempts** it and passes; no garbage cache entry is written | producer: `CacheLeg`; consumer: cache fast-path |
| B4 | snapshot↔cache agreement | any `examples/` model | audit runs green (asserting mode) — every non-Skip snapshot value has a hash-matching cache entry | producer: audit; consumer: verify pipeline |
| B5 | Detector parity across serve modes | a model with a post-pass-detectable defect | the shared detector registry produces the **same** diagnostic set on cold `eval`, warm `eval_cached`, and `edit_check`; each diagnostic emitted **exactly once** (replayed XOR fresh) | producer: detector registry; consumers: 3 paths |
| B6 | Ordering-core convergence | any dirty-cone schedule | `dirty.rs`'s flat sort (now delegating to the Kahn core) produces a **valid topological order** whose **final result-set is identical** to the pre-delegation baseline on the differential corpus (`unified_dag_differential_corpus.rs`), and the no-stale-Undef checker (4952) stays green — the schedule *order* changes (Kahn global-priority), the *results* do not; **not** byte-identical | producer: Kahn core; consumer: dirty.rs |
| B7 | Deletion parity | build after concurrent-stack deletion | build+test green; re-homed shared-property pins pass against live code; test-count delta fully accounted (dropped tests named) | producer: live code; consumer: test suite |

## §8 — Decomposition plan (G2 signals per leaf)

Greek labels; task IDs assigned at decompose time. Every task cites its INV-id(s) + enforcement mechanism in its done-criteria (INV-META-1). Files are narrow per-file locks; same-file tasks are chained by dependency to serialize cleanly (engine_eval.rs is the mandatory contention point).

**P1 — primitive + ctx + migration (INV-EVAL-1, INV-EVAL-2):**
- **α** *(intermediate, foundation)* — `commit_cell_result` + `DeterminacyRule`/`TraceSource`/`CacheLeg`/`EvalOutcome` in a new `cell_commit.rs`. Unlocks γ/δ/ε/ι and the #5023 consumer. Signal: unit test proves the atomic four-leg commit + `CacheLeg::Skip` path. INV-EVAL-1 (type).
- **β** *(intermediate, foundation)* — required-args `cell_eval_ctx` free-function constructor in a new `cell_eval_ctx.rs`. Unlocks γ/δ/ε. Signal: it does not compile if determinacy/sink/containment are omitted (type-level). INV-EVAL-2 (type).
- **γ** *(leaf)* — migrate **engine_eval.rs** sites (`eval`, `eval_cached`, guarded-group param cells, `reeval_cone_cell` at :4918, the annotation-args post-pass at :4357, and the structural-query/self-datum post-pass at :3451-3473) onto α+β; unrecorded sites → `CacheLeg::Skip`. Deps: α, β. Signal (B2/B3): parity fixture — a warning swallowed on the cached path resurfaces; determinacy identical cold-vs-warm. INV-EVAL-1, INV-EVAL-2.
- **δ** *(leaf)* — migrate **engine_edit.rs** `edit_param` sites (NOT `edit_source`). Deps: α, β. Signal (B2): edit-path re-eval commits identical determinacy to eval; a dropped edit-path warning surfaces. INV-EVAL-1, INV-EVAL-2.
- **ε** *(leaf)* — migrate **unfold.rs** guarded-group/unfold site. Deps: α, β. Signal (B2/B3): unfold path commits via the primitive; parity fixture green. INV-EVAL-1, INV-EVAL-2.

**P3 — post-pass routing + divergence audit (INV-EVAL-4):**
- **ι** *(leaf)* — snapshot↔cache content-hash divergence audit, warn-first over `examples/` then flip to assert (§2.6). New gate test. Deps: γ (post-pass routed there). Signal (B4): audit green (asserting) over `examples/` in verify; a seeded divergence self-test proves it fires. INV-EVAL-4 (test warn→assert). Complementary to 4952 — do not merge.

**P4 — diagnostics as data (INV-EVAL-3):**
- **κ** *(intermediate, foundation)* — `NodeCache` entries carry a per-node diagnostics vec (explicit-vec constructor). File: cache.rs. Unlocks μ. Signal: unit test — an entry constructed with diagnostics round-trips them. INV-EVAL-3 (data).
- **λ** *(intermediate, foundation)* — shared post-pass detector registry in a new `detectors.rs`, callable identically by any path. Unlocks μ. Signal: unit test — the registry yields the same diagnostic set given the same post-pass state. INV-EVAL-3 (shared registry).
- **μ** *(leaf)* — wire the registry into `eval`/`eval_cached`/`edit_check` and replay stored diagnostics on fast-path serves (dissolves the cold-only asymmetry — the annotation-args post-pass at :4357 that `eval_cached` skips per :4373 — and the scattered "must run before …" ordering conventions); double-emission guard (replayed XOR fresh). Deps: κ, λ, γ, δ. Signal (B1/B5): field-index-OOB warning resurfaces on a **repeated** `reify check` of unchanged content (regression fixture); each diagnostic emitted exactly once across the three serve modes. INV-EVAL-3 (type+data).

**P5 — one ordering core (strengthens INV-EVAL-5):**
- **ν** *(leaf; corrected 2026-07-06 per esc-5045-29 — original premise was false)* — `dirty.rs`'s flat sort (`topological_sort`:153 / `compute_eval_set`:236) **delegates to** `run_unified_pass_seeded` (adopting the Kahn global-priority, realization-aware order); the vestigial level-batched `compute_levels`:171 + its `compute_levels_*` unit tests are **deleted** (last consumer is the dead concurrent stack ο removes). `resolve_order` untouched. File: dirty.rs. **Depends on ο** (#5065) so `compute_levels` retires cleanly. Signal (B6): the `unified_dag_differential_corpus` gates + no-stale-Undef checker (4952) stay green (valid topo order + identical final result-set); a differential test asserts **result-set equivalence** — NOT byte-identical order (the two sorts provably diverge, `[a,z,b,c]` vs `[a,b,c,z]`; the "old==new order" premise was unsatisfiable). Strengthens INV-EVAL-5 (single scheduling core; test).

**Dead-path deletion (strengthens INV-EVAL-1/INV-EVAL-2 — shrinks the parity surface):**
- **ξ** *(intermediate)* — re-home the shared-property tests onto the live code they actually pin: `run_unified_pass_returns_acyclic_linear_schedule` (reify-eval/src/concurrent.rs:1008 → engine_fixpoint tests, since it pins the Kahn core P5 also delegates to) and the back-prop shared-auto pins (:780, :886); plus any real-property pins in `reify-eval/tests/concurrent.rs` / `reify-runtime/tests/concurrent_eval.rs`. Unlocks ο. Signal: re-homed tests pass against live code; the pins that only exercised the dead adapter are enumerated for ο. Strengthens INV-EVAL-1/2.
- **ο** *(leaf)* — DELETE `crates/reify-runtime/src/{concurrent,concurrent_eval}.rs` + `crates/reify-eval/src/concurrent.rs` (+ their `mod` decls; + `tests/concurrent.rs`). Deps: ξ. Signal (B7): build+test green post-deletion; test-count delta fully accounted (re-homed vs dropped named). Note in description: this removes the version-id bump site the `engine-build-hardening` allocator would otherwise migrate (coordinate; not a blocker). Strengthens INV-EVAL-1/2.
- **π** *(capstone `[MILESTONE]`, **pending** — gated on α/β/γ/δ/ε)* — the pending-milestone SOP for a design decision (supersedes deferred-bookmark parking). The scheduler holds it until the P1 slice lands+merges; its **dispatch is the notification** that the decision is ripe. On dispatch it does NOT implement — it `escalate_blocker`s for Leo's product decision: wire `edit_source` onto `commit_cell_result` (if warm source-edit latency is a current pain) OR delete it (recommended default if unanswered by **2026-08-06**), then stops blocked on the escalation. No INV until resolved.

**Freshness walk (breadcrumb toward INV-EVAL-6, owner #5023):**
- **ρ** *(leaf)* — relocate `propagate_freshness_only`'s invariant warning to `run_compute_dispatch` (engine_compute.rs:177) as `// TODO(#5023): async completions must invalidate dependents`; KEEP the function. Signal: `reify-audit --pattern PTODO` accepts the #5023 citation (live, non-terminal); the breadcrumb is present at the load-bearing end. Breadcrumb toward INV-EVAL-6 (owner #5023); PTODO gate.

**Cross-batch edge:** `#5023 depends_on α`.

## §9 — Out of scope

- **P6 (eval/eval_cached merge into one mode-parameterized walk)** — folded into the unified-DAG-executor milestone (`eval-uniform-dependency-handling.md`). Impl-site breadcrumb: leave a comment at the top of the migrated `eval`/`eval_cached` bodies naming P6 + this §9 as the endgame.
- **P7 (engine-side strict `get_strict` lookups; the reify-expr numeric-promotion table)** — independent; coordinate with 4952/4963/4973 and compiler/expr work respectively.
- **P7a (Undef-provenance split)** — coordinate scope with task 4952's PRD, not here.
- **`edit_source` migration/deletion** — gated on the π capstone `[MILESTONE]` (pending, dep-gated on P1; escalates to Leo on dispatch).
- **Wiring `propagate_freshness_only` into async completion** — owned by #5023 (this PRD only breadcrumbs it).
- **engine_build.rs work** (version-id allocator, reset invariants, test-eviction) — `engine-build-hardening` / `god-file-decomposition`.

## §10 — Open questions (tactical; deferred to impl)

1. **Exact borrow shape of `cell_eval_ctx`** — the required-args invariant is fixed (§2.5); the precise argument-by-reference vs by-value split is the architect's call at task β. **Suggested resolution:** mirror the borrow structure the current `eval_ctx_with_meta` sites already satisfy.
2. **Where the shared detector registry's post-pass state comes from on `edit_check`** — eval/eval_cached have the snapshot in hand; edit_check's exact hand-off point is an impl detail. Decide during task μ.
3. **Break-glass knob name for the §2.6 audit** — mirror the main-gate `*_ENFORCE`/`*_BYPASS` pattern; concrete env-var name decided at task ι.
4. **Which `tests/concurrent.rs` pins are "real shared property" vs "dead-adapter only"** — enumerated during task ξ (the re-home), not pre-decided here.

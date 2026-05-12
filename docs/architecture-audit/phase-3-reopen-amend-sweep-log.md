# Phase 3 Reopen-and-Amend Sweep Log

**Date:** 2026-05-12
**Agent:** audit-reopen-amend-sweep
**Policy applied:** `feedback_task_chain_user_observable.md` (two-branch remediation rule)
**Source:** `docs/architecture-audit/phase-3-files-synthesis.md` §1 cluster C-07 + §5e + §2 Pattern 3 (~10 cited cases)

## Method recap

For each cited "done while load-bearing wiring is absent" case:
1. Verified task state via `mcp__fused-memory__get_task`.
2. Verified `done_provenance` shape for false-positive smell.
3. Spot-checked code via grep to confirm the audit finding still holds.
4. Classified as **(a)** leave done + file follow-up, or **(b)** file true prereq + depend + reopen.
5. Filed via `submit_task(planning_mode=True)` + `commit_planning(target_status=pending)`.
6. Reopened via `set_task_status(done→deferred)` with `reopen_reason` per `feedback_phantom_done_use_deferred_not_inprogress.md`.

All filings: `project_id="reify"`, `agent_id="audit-reopen-amend-sweep"`, `project_root="/home/leo/src/reify"`.

## Branch (b) — reopen with prereqs

| Case (orig task) | Audit ref | Prereq filed | Final leaf signal |
|---|---|---|---|
| 2954 (fea-gui-rendering screenshot_window docs-only) | findings/fea-gui-rendering.md M-001; synthesis M-019 | **3479** | `screenshot_window` MCP tool returns base64 PNG data-URL of full Tauri window state (panels/overlays/probe popups) — not just WebGL canvas; verified via MCP smoke test asserting non-empty PNG bytes > canvas-only `screenshot` size + unit test asserting renderer.render() ran before toPng |
| 250 (AdHocSelector engine evaluator → Undef) | findings/persistent-naming-v2.md M-022 | **3482** | Fixture `.ri` calling `@face("top")` evaluates to a Frame value (not Undef); selector against a renamed/deleted face emits `E_ADHOC_SELECTOR_UNRESOLVED` rather than silently returning Undef. Also unblocks the user-facing payoff of task 2652 (resolve_unique_by_attribute consumer) |
| 2699 (topology-selectors 11 eval dispatch arms absent) | findings/topology-selectors.md M-003 + findings/persistent-naming-v2.md M-019; GR-005 | **3484** | Integration test calling each of the 11 selectors (edges, faces, edges_by_length, faces_by_area, faces_by_normal, edges_parallel_to, edges_at_height, center_of_mass, moment_of_inertia, adjacent_faces, shared_edges) against a known primitive returns a non-Undef value with expected shape + element count; existing ignored sites in topology_selector_smoke_tests.rs are un-ignored and pass |

All three reopened via `done→deferred` with explicit `reopen_reason` citing the audit. Dependency edges (e.g. 2954 → 3479) added so the orchestrator can't claim the reopened task before its prereq lands.

Once each prereq lands, the reopened task should flip `deferred→done` with new `done_provenance`.

## Branch (a) — leave done + file follow-up

| Case (orig task) | Audit ref | Follow-up filed | Final leaf signal |
|---|---|---|---|
| 2347 (stdlib-trait-audit doc stale) | findings/stdlib-trait-breadth.md M-002 | **3487** | `docs/notes/stdlib-trait-audit.md` (or renamed `stdlib-trait-breadth-audit-v01.md`) accurately reflects the trait-inheritance state on main: each resolved gap has DONE markers + task/commit refs (2349/bc5c2d69aa, 2352/b3429254e3, 2354); still-open items flagged; cross-link to `materials_mechanical.ri:32-38` Trait Resolution Policy added |
| 2335 (propagate_freshness_only no production caller) | findings/freshness-4-variant.md M-013 | **3489** | Integration test constructs a Pending→Final upstream transition with no value change, runs the engine through the production trigger path (edit/kernel completion/evaluate), asserts downstream Freshness::Final is reached WITHOUT a re-evaluation having fired |
| 2645 (OpenVDB adapter shipped; `elaborate_field` Imported arm still returns Undef) | findings/imported-field-source.md M-007/M-008 + GR-003 | **3490** | Fixture `.ri` with `imported { path = "fixtures/cube.vdb" format = OpenVDB grid = "density" }` compiles without Severity::Error, evaluates to a `Value::Field` whose probe samples match the cube's known content, and the realization cache key incorporates the file's content-hash so a content change invalidates the cache |

Rationale for branch (a): each task delivered real, useful work (an audit doc; the freshness walk + 1300+ LOC tests; the OpenVDB FFI adapter + ingest). The missing link is downstream wiring or doc refresh, naturally filed as a separate follow-up rather than reopening the closed work.

## Existing fix-now task adequate (skip)

| Case (orig task) | Audit ref | Existing task | Why no action |
|---|---|---|---|
| 2657 (Manifold MeshGL `KernelAttributeHook` stub returning Discarded) | findings/persistent-naming-v2.md M-018; GR-004 | **3472** (in-progress per phase-3-fixnow-filing-log.md C-39) | 3472's title + scope already cover the MeshGL attribute walk; user-observable leaf already specified ("Boolean union of two annotated solids on Manifold kernel returns Preserved attributes; selector against a propagated face resolves correctly"). Branch (a): trait wiring landed; 3472 is the follow-up |
| 2658 (selector vocabulary v2 — Rust pubs not in dispatch table) | findings/persistent-naming-v2.md M-019; cluster C-10 | **3466** (in-progress per fix-now log) | 3466 explicitly covers the selector_vocabulary_v2 names; user-observable leaf already specified. Branch (a): library landed; 3466 is the follow-up |
| 215 (`CompiledModule.doc` field never added on compiled types) | findings/reify-doc-tool.md M-006; cluster C-24 | **3462** (pending per fix-now log) | 3462's scope: "thread doc strings through compiler types (TopologyTemplate/CompiledFunction/TraitDef/EnumDef)" — exactly the M-006 gap. Branch (a): 215's "propagation" lowered the AST→compiled boundary; 3462 fixes the missing field |
| 2652 (PNv2 selector resolution attribute-lookup primary) | findings/persistent-naming-v2.md M-013 | **3482** (this sweep) | The library function `resolve_unique_by_attribute` is wired; the missing piece is a production caller. 3482 (AdHocSelector engine evaluator, filed under 250's reopen) IS that production caller. No additional task needed |

## Sanity-check anomalies (Leo's notice)

Three flavours surfaced:

**1. `done_provenance.kind=found_on_main` — confirmed reconciler-flip pattern (see `feedback_reconciler_found_on_main_false_positive.md`)**

- **2358 (AlwaysCancelWhenStale)** — `done_provenance` is `{kind: "found_on_main", commit: 7f47912164…, note: "architect-reported task already on main; … commit 357226436 \"feat: implement commitment-aware task handling…\""}`. Audit verified the wiring DID land (commitment.rs:190-220, concurrent.rs:329-369, 3 integration tests at concurrent_eval.rs:3417-3605). This is a TRUE-positive found_on_main — the work really was on main — but the architect-driven flip closes the task ~4 weeks AFTER the implementing commit, which obscures provenance for future archaeology. **No action needed; flagged as a true-positive instance of the pattern for Leo's record.**
- **2491 (test polish in OCCT inertia tests)** — `done_provenance` is `{kind: "found_on_main", note: "orchestrator CAS-retry sweep: workflow logged DONE at 2026-04-27 16:45:46; pre-rebase merge SHA was unknown (post-rebase landing SHA not pinned)"}`. This is unverifiable from the provenance alone; the linked context is suggestive of a CAS-retry-induced ambiguity. **No action needed; flagged for Leo's awareness.**

**2. Audit citation misattribution — task IDs in `phase-3-files-synthesis.md` §1 cluster C-07 list don't always map to the cited mechanism**

- **2491** is captioned in the C-07 list as "auto-type-param-resolution call-site", but the actual task 2491 is polish-grade test-comment tweaks in OCCT density/inertia tests (per its `description` field and `metadata.modules = [reify-kernel-occt/src/lib.rs]`). The REAL "auto-type-param call-site" gap is `findings/auto-type-param-resolution.md` M-009 (compile-pipeline call site that invokes `resolve_auto_type_params`) — and that gap is already addressed by **task 3465** (filed under cluster C-05 in the fix-now log). **No action needed beyond this note; 3465 owns the real work.**
- **2456** is captioned in the C-07 list as "cache config no consumer". The actual task 2456 is `warm_pool::donate_with_cost` emitting `Evicted` events on same-key overwrite — fully wired with regression test at commit ec38212c27. The REAL "cache config no consumer" gap is cluster C-12 (Manifest.kernel_pins / CacheConfig / NodePolicyOverrides / warm_state_budget_bytes / auto_type_params::max_depth — config types unread) — and that gap is already addressed by **task 3467** (filed under cluster C-12 in the fix-now log). **No action needed beyond this note; 3467 owns the real work.**

Both misattributions are mechanical-sloppy in the C-07 caption list; the underlying findings and the fix-now log got the right tasks. Suggests the synthesis should be checked against per-task `metadata.modules` next time. Filed audit-trail task **3476** for 2491 was the right move at first glance, then **cancelled** as a duplicate of 3465 once cross-checked.

**3. Phantom-done at one level back — 2669 (imported-field end-to-end smoke test) is `done` at commit 3b3bcbadb0 but `elaborate_field` still returns Undef**

While investigating 2645 (OpenVDB adapter), I noticed task 2669 ("end-to-end smoke test + diagnostic coverage") is also marked done — yet `imported_field_e2e.rs:96-122` actively pins `lambda == Value::Undef`. The audit treats 2645 as the failure surface; the closer-to-truth observation is that **2669 is the actual phantom-done** (its title promises end-to-end smoke coverage; the only test on main is the Undef-pinning one). Filing 3490 against 2645 (as the upstream "the adapter is real but isolated") is the most pragmatic remediation surface, but 2669 might warrant a separate reopen if/when Leo wants to apply the same policy to that sibling phantom-done. Out of this sweep's scope. **Flagged for follow-up consideration.**

## Issues encountered

- One `submit_task` call (the M-009 audit-trail entry) initially succeeded as task **3476** but was rapidly cancelled once cross-checked against the existing fix-now log (3465 covers the work). No duplicate task left running.
- Six `submit_task(planning_mode=True)` returned synchronous `{task_id, status: "deferred", planning_mode: true}` results, all then flipped to `pending` via one `commit_planning` call. No timeouts, no curator races.
- Three `set_task_status(done→deferred, reopen_reason=...)` calls succeeded immediately with reconciliation marked async; no terminal-exit rejection or other gate.
- No fused-memory MCP errors, no PRD edits, no gap-register edits, no code edits.

## Summary

- **Branch (a) — leave done + new follow-up:** 3 cases (2347, 2335, 2645) → 3 new tasks (3487, 3489, 3490).
- **Branch (b) — reopen with prereqs:** 3 cases (2954, 250, 2699) → 3 new prereqs (3479, 3482, 3484), 3 reopens (done→deferred), 3 dependency edges added.
- **Existing fix-now adequate (skip):** 4 cases (2657 → 3472, 2658 → 3466, 215 → 3462, 2652 → covered transitively by 3482).
- **Sanity-check anomalies:** 5 (2358 + 2491 + 2456 reconciler/citation flavours; 2669 sibling phantom-done observation).
- **Net new active tasks:** 6 pending (3479/3482/3484/3487/3489/3490), 3 reopened-deferred (2954/250/2699), 1 cancelled (3476 dup).

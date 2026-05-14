<!-- 2026-05-14 RECOVERY AUDIT TRAIL
This filing log was authored 2026-05-12. The task IDs referenced below
(3462-3474 fix-now batch) were LOST in the 2026-05-13 fused-memory SIGABRT.
Full recovery in two passes:
  Pass 1 — worktree_orphans (4 tasks with active worktrees at SIGABRT):
    3465 (Auto-type-param resolver)            → 3522
    3466 (Selector v2 vocabulary)              → 3523
    3469 (Kinematic interferes/min_clearance)  → 3524
    3472 (Manifold KernelAttributeHook MeshGL) → 3525
  Pass 2 — agent re-file 2026-05-14 (the remaining 9 with no recovery trace):
    3462 (Doc-tool: thread doc through compiler types) → 3557
    3463 (reify doc: build_doc_model + render_html)    → 3562  [chain dep 3557]
    3464 (reify doc --stdlib CLI surface)              → 3565  [chain dep 3562]
    3467 (5 reify-config consumers)                    → 3572
    3468 (to_global / envelope helpers)                → 3575  [dep 3540 SIR-α]
    3470 (extract_input_tolerance_promise read fix)    → 3578
    3471 (Kinematic singularity diagnostic)            → 3580
    3473 (WarmStatePool drain_events)                  → 3582  [dep 3420 CN-α]
    3474 (Stdlib shell ElasticResult alias)            → 3583  [dep 3540 SIR-α]
Body preserved as historical record. See gap-register.md top banner.
-->

# Phase 3 Fix-Now Filing Log

**Date:** 2026-05-12
**Agent:** audit-followup-fixnow
**Policy applied:** `feedback_task_chain_user_observable.md` (Task DAGs must terminate in user-observable behavior at leaves)
**Source:** `docs/architecture-audit/phase-3-files-synthesis.md` §4 fix-now disposition (17 clusters)

## Filed (new)

Tasks all created via `submit_task(planning_mode=True)` and flipped to `pending` via `commit_planning` in one batch.

| Cluster | Task ID | Title | Leaf observable behavior |
|---|---|---|---|
| C-05 | 3465 | Auto-type-param resolver: invoke Phase A/B/C orchestrator from compile pipeline; populate CompiledModule.auto_type_substitution | Fixture .ri with inferable type-param compiles AND eval yields correctly-typed value (not Real placeholder); negative-path emits E_AUTO_TYPE_PARAM_UNRESOLVED |
| C-10 | 3466 | Selector v2 vocabulary: register selector_vocabulary_v2 names in GEOMETRY_TOPOLOGY_SELECTOR_NAMES + eval dispatch | Fixture .ri calling intersect/union/complement/except/faces_perpendicular_to/extremal_by_bbox/faces_by_surface_kind compiles AND evaluates to non-Undef selection sets |
| C-12 | 3467 | Wire reify-config consumers (Manifest.kernel_pins, CacheConfig, NodePolicyOverrides, warm_state_budget_bytes, auto_type_params::max_depth) | Manifest TOML override → engine/compiler picks up each of 5 fields (verifiable via 5 distinct assertions) |
| C-24 | 3462 | Doc-tool: thread doc strings through compiler types (TopologyTemplate/CompiledFunction/TraitDef/EnumDef) | Compiled module's types carry `doc: Some(…)` populated from AST (unit test in reify-compiler) |
| C-25 | 3463 | reify doc: wire build_doc_model + replace render_html_stub so `reify doc` emits real HTML | `reify doc <file.ri>` produces HTML containing doc-comment text and symbol names |
| C-29 | 3468 | Stdlib: implement to_global(stress, frame) + linear_combine frame handling + typed envelope helpers | Multi-load-case linear combine produces LinearCombineResult with non-Undef frame and numerically-correct worst_case / min_max_stress envelope vs hand-computed reference |
| C-31 | 3469 | Kinematic interferes/min_clearance: apply per-body world_transform before OCCT distance probe | Fixture 2-body chain that overlaps only when FK-positioned returns interferes_with=true and min_clearance<0 |
| C-34 | 3470 | extract_input_tolerance_promise: read Provenance.tolerance_guarantee, not non-existent `tolerance` cell on Input | Fixture with Provenance.tolerance_guarantee=0.01mm yields per-stage budget reflecting that promise (not the default) |
| C-37 | 3471 | Kinematic singularity: route snapshot()/sweep() through solve_loop_closure_with_diagnostics; surface is_singular + typed diagnostic | Near-singular kinematic snapshot returns Snapshot.is_singular=true AND EvalResult diagnostic stream contains typed KinematicSingular entry |
| C-39 | 3472 | Manifold KernelAttributeHook: implement MeshGL attribute walk (no longer task_9_pending Discarded) | Boolean union of two annotated solids on Manifold kernel returns Preserved attributes; selector against a propagated face resolves correctly |
| C-41 | 3464 | reify doc --stdlib: stdlib-page surface in `reify doc` CLI | `reify doc --stdlib --out <dir>` produces HTML pages for stdlib traits/structures with known names visible in index |
| C-43 | 3473 | WarmStatePool: drain Evicted/Donated events at eval boundary and surface in eval diagnostic stream | Engine under memory pressure surfaces Evicted+Donated entries in EvalResult.diagnostics |
| C-44 | 3474 | Stdlib shell ElasticResult: implement `result.stress = result.stress.mid` backward-compat alias | Shell-solve fixture: `result.stress` and `result.stress.mid` yield identical tensor fields |

**Total new tasks filed: 13** across 13 clusters.

### Dependency chain (within the batch)

- 3462 (C-24) ← 3463 (C-25) ← 3464 (C-41)
  - C-24 propagates the doc field through compiler types; C-25 builds the DocModel + replaces the HTML stub (needs C-24); C-41 adds the stdlib-page surface (needs C-25).

All other tasks (C-05, C-10, C-12, C-29, C-31, C-34, C-37, C-39, C-43, C-44) are single-leaf and independent within the fix-now batch.

## Filed (follow-up to existing)

None this pass. Two existing tasks were considered and judged adequate — see next section.

## Existing task adequate

| Cluster | Existing Task | Why no action |
|---|---|---|
| C-03 | 3117 (deferred) | Title already specifies the user-observable outcome: tighten `ElasticResult::displacement` and `::stress` from Real → Field<X,Y> as the acceptance criterion. Description names the probe test + the resolver fix path. Leaving alone; matches policy. |
| C-11 | 2656 + 2831 (both pending) | These two tasks cover the boolean and fillet/chamfer eval-side wiring per the persistent-naming-v2 task-3 contract (AttributeHistory::Boolean/Fillet/Chamfer variants + dispatch). Each task's metadata.files names the engine_build wire site and an e2e integration test (topology_attribute_boolean_e2e.rs, topology_attribute_local_features_e2e.rs). The completion = user-visible attribute propagation through Boolean/fillet/chamfer ops, which the e2e tests already encode. Adequate as-is. |
| C-27 | 3115 (deferred) | Acceptance includes "the 15 blocked-composite sites tightened" — that is the user-observable downstream change, modulo the v0.5 fractional-exponent caveat already documented. Adequate. |

## Design-deferred

| Cluster | Reason |
|---|---|
| C-33 | **CancellationHandle type** (task 3384) is explicitly "pick one of three options" (Arc<AtomicBool> / tokio_util CancellationToken / custom). Plus the Pending sentinel is "Value::Pending variant vs. reuse freshness flag" — also a binary design choice. Per the policy this is design-ambiguous, not fix-now under "well-defined completion conditions". Task 3384 is already filed and surfaces both options on the architect during planning; that is the correct shape. No new fix-now task. |

## Issues encountered

- No fused-memory MCP errors. All 13 `submit_task(planning_mode=True)` calls returned synchronous `{task_id, status: "deferred", planning_mode: true}` results; the subsequent `add_dependency` and `commit_planning(target_status="pending")` succeeded without retry.
- Two existing dependency-cited tasks (#3115 and #3117) are in `deferred` status. They remain so under this pass since I did not flip them — they are bookmarked by the user's earlier audit follow-up workflow (task 3090 lineage) and re-activation is the user's call. Should you want them pending alongside the C-24/C-25/C-41 chain, a separate small flip is fine.
- No PRDs were touched. No code was touched. No gap-register edits.

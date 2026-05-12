# ComputeNode contract §8 DAG — filing log

Session: 2026-05-12 mechanical filing of the ComputeNode contract vertical-slice DAG into the task tracker.

Source contract: `docs/prds/v0_3/compute-node-contract.md` (commit `d2cfe40980`).
Cross-PRD prerequisite (for task η): `docs/prds/v0_3/structure-instance-runtime.md` (commit `b6da30e1f8`).

## Task IDs assigned

| Letter | Task ID | Title | Prereqs (task IDs) |
|---|---|---|---|
| α | 3491 | CacheStore::pending_cause admit NodeId::Compute(_) as chain root | none |
| β | 3492 | Real CancellationHandle (Arc<AtomicBool> wrapper) replaces unit-struct stub | none |
| γ | 3493 | Per-Engine dispatch registry + test::identity trampoline + @optimized→ComputeNode lowering wire | 3491, 3492 |
| δ | 3494 | Freshness::Pending integration during in-flight ComputeNode dispatch + atomic completion | 3493, 3491 |
| ε | 3495 | Cancellation wiring through dispatch (cooperative; ≤2× poll budget SLA) | 3493, 3492, 3494 |
| ζ | 3496 | OpaqueState lifecycle (cache read → slot populate → trampoline → write-back → cache donate → slot clear) | 3493 |
| η | 3497 | stdlib solve_elastic_static @optimized(solver::elastic_static) end-to-end vertical slice | 3494, 3495, 3496 (+ cross-PRD: structure-instance-runtime foundation slice) |
| θ | 3498 | Significance filter integrated into freshness walk at output-VC boundary | 3497 |
| ι | 3499 | Persistent-cache lookup/write integration at ComputeNode dispatch boundaries | 3497 (+ cross-PRD: persistent-fea-cache) |
| κ | 3500 | Mesh-morph engine wiring via ComputeNode at VolumeMesh realization dispatch | 3497, 3496 (+ cross-task: 2945, 2946 from mesh-morphing PRD) |
| μ | 3501 | Correct mesh-morphing PRD prose — axis-1 yes, axis-2 unchanged | none |
| ν | 3502 | Confirm 3379/3383/3384 cancelled as superseded by contract DAG | none |

All filed via `mcp__fused-memory__submit_task(planning_mode=true)`. All start in `deferred` status; will flip to `pending` via `commit_planning` at the end of this session.

## Dependency edges added (15 edges total)

| From | To (depends on) | Rationale |
|---|---|---|
| 3493 (γ) | 3491 (α) | dispatch registry uses extended pending_cause chain-root admission |
| 3493 (γ) | 3492 (β) | dispatch registry uses real CancellationHandle |
| 3494 (δ) | 3493 (γ) | Pending lifecycle wires into the registry's dispatch path |
| 3494 (δ) | 3491 (α) | output-VC pending_cause chain walk uses α's admission |
| 3495 (ε) | 3493 (γ) | cancellation wires into the registry's dispatch path |
| 3495 (ε) | 3492 (β) | cancellation uses real CancellationHandle |
| 3495 (ε) | 3494 (δ) | cancellation preserves last_substantive in Pending state |
| 3496 (ζ) | 3493 (γ) | OpaqueState lifecycle wires into the registry's dispatch path |
| 3497 (η) | 3494 (δ) | first real consumer needs Pending integration |
| 3497 (η) | 3495 (ε) | first real consumer needs cancellation |
| 3497 (η) | 3496 (ζ) | first real consumer needs warm-state |
| 3498 (θ) | 3497 (η) | significance filter integration needs a real consumer to verify against |
| 3499 (ι) | 3497 (η) | persistent-cache hookup uses real FEA output |
| 3500 (κ) | 3497 (η) | mesh-morph as ComputeNode consumer uses FEA warm-state |
| 3500 (κ) | 3496 (ζ) | mesh-morph consumes OpaqueState lifecycle |

DAG view (from contract §8):

```
α(3491) ─┐
         ├─→ γ(3493) ─┬─→ δ(3494) ─→ ε(3495) ─┐
β(3492) ─┘            │                       ├─→ η(3497) ─┬─→ θ(3498)
                      └─→ ζ(3496) ────────────┘            ├─→ ι(3499)
                                                           └─→ κ(3500) ←── (mesh-morph 2945, 2946)

μ(3501) (independent doc edit)
ν(3502) (post-filing audit confirmation; independent)
```

Companion tasks μ + ν have no edges into α–κ.

Cross-PRD dependencies — **rule reversal 2026-05-12 (post-filing):** cross-PRD deps MUST be real `add_dependency` edges, not metadata-only. Scheduler doesn't read metadata. Edges added retroactively where prereq task IDs exist. See updated memory `preferences-cross-prd-deps-real-edges`.

| Task | Edge added | Notes |
|---|---|---|
| κ (3500) | `add_dependency(3500, depends_on=2945)` | Mesh-morph BoundaryAssociation producer (status: `done`). Edge added 2026-05-12 retroactive sweep. |
| κ (3500) | `add_dependency(3500, depends_on=2946)` | Mesh-morph OCCT Projector concrete impl (status: `done`). Edge added 2026-05-12 retroactive sweep. |
| η (3497) | `add_dependency(3497, depends_on=3503)` | SIR-α (task 3503, in-progress) is the structure-instance-runtime foundation slice (`Value::StructureInstance` variant + `Steel_AISI_1045` ctor + match-site adapters). Edge added 2026-05-12 once SIR decomposition landed (the trampoline signature in compute-node-contract.md §2 was designed to anticipate `Value::StructureInstance` arms — SIR-α delivers what the trampoline expects). |
| ι (3499) | **NOT APPLICABLE** — supersedes the *open scope* of task 2974, but 2974 isn't a hard prereq (ι replaces 2974's open work). Recorded in description prose only. |

Informational metadata kept on tasks for human/audit readability (does not gate scheduling):
- η (3497) `metadata.cross_prd_dep`: structure-instance-runtime foundation slice
- κ (3500) `metadata.cross_task_deps`: `[2945, 2946]`

## Task-state side effects (already applied this session)

| Task | Action | Reason |
|---|---|---|
| 3379 | set_task_status → `cancelled` with reopen_reason | Subsumed by η (3497) — the vertical-slice owns trampoline registration; reify-solver-elastic API unchanged. |
| 3383 | set_task_status → `cancelled` with reopen_reason | Subsumed by γ (3493) — per-Engine dispatch registry + @optimized lowering. |
| 3384 | set_task_status → `cancelled` with reopen_reason | Split across δ (3494, pending) + ε (3495, cancellation). |
| 3378 | update_task description (append) | Status remains `deferred`. Added explicit prerequisites: structure-instance-runtime PRD + ComputeNode contract DAG η (3497). Noted η subsumes 3378's stdlib-decl scope; if η lands first, 3378 can be cancelled-as-superseded. |

## Supersession provenance

The contract's §8 makes these supersessions explicit:

- **2924** (FEA #16 acceptance) ← η (3497) — first-real-consumer vertical slice ships the stdlib decl + trampoline + smoke `.ri` as one unit.
- **2947** (mesh-morph engine wiring) ← κ (3500) — same wiring, framed as ComputeNode consumer.
- **2974** (persistent-fea-cache open work) ← ι (3499) — persistent-cache hookup belongs on the ComputeNode boundary.
- **3379** (P4) ← η (3497).
- **3383** (P3.4) ← γ (3493).
- **3384** (P3.5) ← δ (3494) + ε (3495).

These prior tasks remain readable for audit purposes; only 3379/3383/3384 are flipped to `cancelled` (2924/2947/2974 are not because they belong to other PRDs and their acceptance is now nominally re-met by the contract DAG — the relevant PRDs can absorb the supersession in a follow-up sweep).

## Done foundation tasks (left as-is)

3380 / 3381 / 3382 / 3385 (P3.1 / P3.2 / P3.3 / P3.6) are already `done` and stand. The contract's §8 confirms they need no rework.

## Session-end procedure

After this filing log, the session:
1. Calls `commit_planning` on 3491,3492,3493,3494,3495,3496,3497,3498,3499,3500,3501,3502 (target_status=pending).
2. Writes a summary memory under `observations_and_summaries` capturing the IDs + log location.

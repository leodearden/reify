# structure-instance-runtime §8 DAG — filing log

Session: 2026-05-14 re-decomposition of the SIR PRD after the 2026-05-13 SIGABRT lost the original SIR-α (task #3503) plus siblings (3504 / 3508 / 3510 / 3512) and the GR-031 envelope task (#3468).

Source contract: `docs/prds/v0_3/structure-instance-runtime.md` (commit `b6da30e1f8`).
Recovery context: `docs/task-recovery-2026-05-13/investigation.md` (forensic write-up of the watchdog SIGABRT + ~150 task-row WAL-discard event).

## Task IDs assigned (new — original IDs unrecoverable)

| Letter | New Task ID | Original Task ID | Title | Prereqs (task IDs) |
|---|---|---|---|---|
| α | 3540 | 3503 (lost) | SIR-α: Value::StructureInstance foundation slice (wide-lock — variant + registry + adapter sweep + compile-lowering + cache-key + Steel/PointLoad/FixedSupport ctors) | none |
| β-mat | 3542 | 3504 (lost) | SIR-β-mat: Remaining materials reachable via SIR foundation (Aluminium_6061_T6, Titanium_Ti6Al4V, ABS_Plastic) | 3540 |
| β-load | 3544 | 3508 (lost) | SIR-β-load: PressureLoad rewrite (snake → Pascal + stdlib structure_def + retire Rust builtin) | 3540 |
| β-sup | 3546 | 3510 (lost) | SIR-β-sup: PinnedSupport rewrite (stdlib structure_def + retire Rust builtin) | 3540 |
| β-mlcfea | 3549 | 3512 (lost) | SIR-β-mlcfea: LoadCase / MultiCaseResult ctor rewrites as stdlib structure_defs | 3540 |

All filed via `mcp__fused-memory__submit_task(planning_mode=true)` with `agent_id="claude-recovery-sir"` and `project_id="reify"`. Each carries `metadata.audit_provenance = "re-filed 2026-05-14 post-SIGABRT (was task #<old-id>; lost in 2026-05-13 14:25 BST watchdog event)"` and `metadata.original_task_id`.

SIR-γ (GR-031 composed-stress envelope helpers) — the PRD §8 Phase 3 explicitly states "no new task filed here" (the envelope helpers task pre-existed as #3468). After the SIGABRT, task #3468 was repurposed by curator recovery for an unrelated WarmStatePool / cost-per-byte placeholder (replayed from `tickets.db.candidate_json`). No live SIR-γ envelope task exists today; flagged for parent-session decision.

SIR-δ (gap-register companion edits per PRD §10) is not a queued task — per the PRD it happens in the authoring session, not the orchestrator.

## Dependency edges added (4 intra-batch edges total)

| From | To (depends on) | Rationale |
|---|---|---|
| 3542 (β-mat) | 3540 (α) | remaining materials become reachable only once the new ctor-lowering path is live |
| 3544 (β-load) | 3540 (α) | PressureLoad rewrite consumes `trait Load` + new ctor-lowering path |
| 3546 (β-sup) | 3540 (α) | PinnedSupport rewrite consumes `trait Support` + new ctor-lowering path |
| 3549 (β-mlcfea) | 3540 (α) | LoadCase/MultiCaseResult rewrites consume the new variant + ctor-lowering path |

DAG view (mirrors PRD §8 dependency view):

```
                              ┌→ 3542 (SIR-β-mat) ────→ GR-019 closed
                              │
3540 (SIR-α, high-pri) ──────┼→ 3544 (SIR-β-load) ──┐
                              ├→ 3546 (SIR-β-sup) ──┴→ GR-011 closed
                              │
                              └→ 3549 (SIR-β-mlcfea) (multi-load-case-fea coordination)

(SIR-γ envelope helpers / GR-031 — no live task; original #3468 lost + repurposed)
```

## Cross-PRD edges to wire later (Phase 1.7 of parent session)

Per the brief: do NOT wire cross-PRD edges in this filing session. Instead, each task carries `metadata.cross_prd_dep_pending` strings describing the desired edges; parent session wires them.

| Edge to wire (later) | From task | Direction | Notes |
|---|---|---|---|
| compute-node-contract.md η (current task 3426) `depends_on` SIR-α (3540) | 3426 | downstream → upstream | The CN trampoline signature in `compute-node-contract.md §2 ComputeFn` was designed to anticipate `Value::StructureInstance` arms; SIR-α delivers what the trampoline expects. Cross-PRD edge MUST be a real `add_dependency` edge (per `preferences_cross_prd_deps_real_edges`). |
| GR-024 buckling-eigensolver α (current task 3450) `depends_on` SIR-α (3540) | 3450 | downstream → upstream | Stdlib structure_defs (BucklingOptions/Mode/Result/MCBR) consume `Value::StructureInstance` — cannot evaluate to non-Undef until SIR-α lands. |
| CN-ι (current task 3428) `depends_on` SIR-α (3540) — **maybe** | 3428 | downstream → upstream | Persistent-cache hookup at ComputeNode dispatch boundaries; PRD §5 specifies a `Value::StructureInstance` cache-key arm in `persistent_cache.rs`. SIR-α ships that arm; ι consumes it. Parent session to verify whether ι's scope already includes the arm or expects α to ship it. |
| **GR-031 SIR-γ (envelope helpers) — NO LIVE TASK** | — | — | Original #3468 was the envelope-helpers task per gap-register GR-031. Task #3468 has been repurposed to an unrelated WarmStatePool placeholder during recovery. Either (a) file a fresh SIR-γ task for envelope helpers (small mechanical leaf — see gap-register GR-031 Disposition / Notes), or (b) leave the SIR-α observable scope unchanged and rely on PRD §7.2 row `GR-031 composed-stress envelope` for coverage. **Flag for parent-session decision.** |
| 3474 retroactive edge (gap-register notes `3474 depends_on 3503`) — **OBSOLETE** | — | — | Current task #3474 is `cancelled` (per `python3 sqlite3 SELECT id,title,status FROM tasks WHERE id='3474'`) and carries the title `"Extend is_geometry_let to Block and Match initialisers (follow-up to 3395)"` — i.e. it has been reassigned by recovery. No edge needed. |

Also pending (independent of SIR — flagged for awareness):
- The gap-register `## §10` companion edits (GR-001 follow-up subsection, GR-011 / GR-019 / GR-031 Notes rows) reference the lost IDs `3503 / 3504 / 3508 / 3510 / 3512` and now-repurposed `3468`. A separate gap-register sweep is required to rewrite those references to the new IDs `3540 / 3542 / 3544 / 3546 / 3549`. Not in this session's scope — flagged for the audit / parent session.

## Metadata fields written (per PRD-decompose skill)

Every task carries:
- `source: "prd-decomposition"`
- `prd_path: "docs/prds/v0_3/structure-instance-runtime.md"`
- `prd_task_label` + `prd_letter`: Greek letter
- `user_observable_signal`: the CLI/example/test signal proving completion (per `feedback_task_chain_user_observable`)
- `consumer_ref`: downstream PRD/task or user surface
- `grammar_confirmed: true` (PRD §1 cites `feedback_prd_grammar_gate`'s grammar gate; task 2039 already shipped struct-ctor parsing)
- `files`: enumerated foundation-lock charter (SIR-α only) / per-task scope
- `modules`: subset of `files` for telemetry payload compatibility (per `project_metadata_files_canonicalized`)
- `audit_provenance` + `original_task_id`: recovery context
- `cross_prd_dep_pending`: list of strings naming edges the parent session must wire

Orchestrator does **not** currently read `user_observable_signal` / `consumer_ref` / `grammar_confirmed` — substrate for the F-infra follow-up session per the PRD-decompose skill notes.

## Verification (post-commit)

```
$ python3 -c "
import sqlite3
con = sqlite3.connect('/home/leo/src/reify/.taskmaster/tasks/tasks.db')
for tid in ('3540','3542','3544','3546','3549'):
    cur = con.execute('SELECT id, title, status, priority FROM tasks WHERE id=?', (tid,))
    print(cur.fetchone())
"
(3540, 'SIR-α: Value::StructureInstance foundation slice …', 'pending', 'high')
(3542, 'SIR-β-mat: Remaining materials reachable …', 'pending', 'medium')
(3544, 'SIR-β-load: PressureLoad rewrite …', 'pending', 'medium')
(3546, 'SIR-β-sup: PinnedSupport rewrite …', 'pending', 'medium')
(3549, 'SIR-β-mlcfea: LoadCase / MultiCaseResult ctor rewrites …', 'pending', 'medium')
```

All 5 tasks `pending` with priorities matching the PRD's high/medium split. 4 of 4 expected intra-batch `depends_on=3540` rows present in the `dependencies` table.

## Session-end procedure

After this filing log:
1. Filing log committed to `docs/architecture-audit/structure-instance-runtime-filing-log.md` for human/audit readability.
2. Parent session (recovery operation) will:
   - Wire cross-PRD edges in Phase 1.7 (3426→3540, 3450→3540, optionally 3428→3540; decide SIR-γ disposition).
   - Sweep the gap-register to rewrite lost-ID references (`3503/3504/3508/3510/3512` → `3540/3542/3544/3546/3549`).
3. Decide whether to file a fresh SIR-γ envelope task or leave GR-031 closure inside SIR-α's observable scope.

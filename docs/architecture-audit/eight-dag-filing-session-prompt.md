# ComputeNode contract §8 DAG filing session — start prompt

Short session — files the 10-task vertical-slice DAG that the ComputeNode contract authored. Paste-ready for a fresh Claude Code session.

This session is mostly mechanical: read the contract's §8, file the tasks via fused-memory `planning_mode`, link dependencies, mark task 3378's status, and cancel-as-superseded for tasks 3379/3383/3384.

---

## Paste this block

```
You are running a short task-filing session. The ComputeNode contract at
`docs/prds/v0_3/compute-node-contract.md` §8 specifies a 10-task
vertical-slice DAG (α through κ) plus two companion tasks (μ for
mesh-morph PRD prose correction, ν to cancel 3379/3383/3384). Your job:
file these tasks via fused-memory MCP, link the dependencies, mark
existing tasks as superseded.

The DAG design is settled. This session is execution, not design.

## PREREQUISITES (verify before filing)

  1. The ComputeNode contract is committed (commit d2cfe40980 or
     descendants). Verify with `git log --oneline docs/prds/v0_3/compute-node-contract.md`.
  2. The structure-instance-runtime PRD is committed at
     `docs/prds/v0_3/structure-instance-runtime.md`. §8 task η depends
     on it. If this PRD is not on disk, STOP — run the
     structure-instance-runtime session first.

If both prerequisites are satisfied, proceed.

## DELIVERABLES

  a. Tasks α–κ filed via fused-memory `planning_mode` (submit_task +
     commit_planning). Each task: title (imperative ≤80 chars),
     description (1-3 paragraphs including user-observable signal),
     dependencies (task IDs from this batch), memory_hints.
  b. Companion task μ filed: mesh-morph PRD prose correction (per
     contract §6 axis-1 routing decision — the PRD currently denies
     routing through ComputeNode; correct this).
  c. Companion task ν filed: cancel 3379/3383/3384 as superseded by
     the contract DAG. Use reopen_reason pointing at the contract.
  d. Task 3378 (FEA solve_elastic_static stdlib fn decl) status:
     verify it is still `deferred`; if so, update its description to
     reference the contract + structure-instance-runtime PRDs as its
     two prerequisites (it will be unblocked when both prereqs and
     the contract DAG land).
  e. Filing log at
     `docs/architecture-audit/phase-3-eight-dag-filing-log.md`:
     per-task summary, dependency edges, IDs assigned.

## REQUIRED READING (in order — short list)

  1. docs/prds/v0_3/compute-node-contract.md §8 (the DAG —
     authoritative spec)
  2. docs/prds/v0_3/compute-node-contract.md §7 (boundary tests
     — each task's user-observable signal often points at one of
     these)
  3. docs/prds/v0_3/structure-instance-runtime.md (prerequisite for
     η — confirm it's on disk; spot-check its decomposition for task
     IDs that η should depend on)
  4. ~/.claude/projects/-home-leo-src-reify/memory/feedback_task_chain_user_observable.md
     (every task names a user-observable signal)
  5. ~/.claude/projects/-home-leo-src-reify/memory/procedural_fused_memory_two_phase_writes.md
     (submit_task semantics)
  6. ~/.claude/projects/-home-leo-src-reify/memory/feedback_planning_mode_scope.md
     (planning_mode is right for this batch)
  7. ~/.claude/projects/-home-leo-src-reify/memory/procedural_set_task_status_semantics.md
     (cancel-as-superseded for ν; reopen_reason semantics)

## METHOD

For each task α–κ in the contract's §8:

  1. Read the task's spec in §8. Extract: name, description,
     prerequisites (which other tasks in α–κ), user-observable signal.
  2. Construct the task body:
     ```
     <title>

     Origin: ComputeNode contract §8 (commit <SHA>) task <Greek letter>.
     Supersedes / extends prior compute-node-infrastructure decomposition.

     <description from §8>

     User-observable signal at completion:
     <one-line signal from §8 or §7 boundary tests>

     Prerequisites: <task IDs from this batch, named in §8>
     ```
  3. Submit via `mcp__fused-memory__submit_task` with:
     `project_id="reify"`, `agent_id="audit-eight-dag-filing"`,
     `project_root="/home/leo/src/reify"`.
  4. Capture the returned task ID; map it to the Greek letter for
     dependency linking.

After all α–κ submitted, link dependencies via `add_dependency` calls
(the contract's §8 names the DAG edges).

Then file μ and ν:

  - **μ (mesh-morph PRD prose correction):** small targeted edit to
    `docs/prds/v0_3/mesh-morphing.md`. Locate the breadcrumb / prose
    that says mesh-morph "bypasses" or "does NOT route through"
    @optimized / ComputeNode; rewrite to reflect contract §6 axis-1
    decision (does route through). User-observable signal: the PRD
    text matches the contract; reviewable diff. Memory hint:
    `mesh-morphing ComputeNode axis-1 routing`. Dependency: none
    (independent of α–κ).
  - **ν (cancel-as-superseded for 3379/3383/3384):** call
    `set_task_status` on each of 3379, 3383, 3384 with status
    `cancelled` and reopen_reason citing the contract DAG that
    supersedes them. Then submit a small task ν with description
    "Confirm tasks 3379/3383/3384 are marked cancelled with
    reopen_reason citing contract DAG; close ticket when verified."
    User-observable signal: the three task statuses report cancelled
    via `mcp__fused-memory__get_task`.

After all submitted, run `commit_planning` to release the batch.

## TASK 3378 UPDATE

Read task 3378 via `get_task`. Confirm status is `deferred`. Update
its description (via `update_task`) to:

```
[existing description preserved at top]

---
Updated 2026-05-12 (audit follow-up):
Two named prerequisites unblock this task:
  1. `docs/prds/v0_3/structure-instance-runtime.md` lands (GR-001
     follow-up PRD — Value::StructureInstance variant + match-site
     adapters + Material starter library)
  2. ComputeNode contract §8 DAG α–κ lands (the dispatch surface this
     stdlib fn declaration plugs into)
When both land, 3378 transitions deferred → pending and the orchestrator
can schedule it. No DAG dependency edges added here — see §8 task η for
the actual structure-instance-runtime ↔ ComputeNode wiring.
```

## SESSION END

Stop when:
  1. All α–κ + μ + ν filed; IDs captured in the filing log.
  2. Dependencies linked between α–κ per the contract's §8 DAG edges.
  3. Tasks 3379/3383/3384 cancelled with reopen_reason citing
     contract.
  4. Task 3378 description updated.
  5. `commit_planning` succeeded.
  6. Summary memory written:
     ```
     mcp__fused-memory__add_memory(
       project_id="reify",
       agent_id="audit-eight-dag-filing",
       category="observations_and_summaries",
       content="ComputeNode contract §8 DAG filed 2026-05-12:
       α–κ + μ + ν assigned task IDs <list>; 3379/3383/3384
       cancelled as superseded; 3378 marked with two prereqs.
       Log at docs/architecture-audit/phase-3-eight-dag-filing-log.md"
     )
     ```

Do NOT:
  - Redesign tasks. The contract's §8 is authoritative.
  - File any tasks outside α–κ + μ + ν.
  - Edit code under crates/.
  - Commit anything.

Hard cap: 60k tokens (this is mechanical execution).

## EXPECTED SESSION LENGTH

20–40 minutes. Mostly fused-memory MCP calls + a small PRD edit for μ.
```

---

## Notes for Leo

- Run AFTER the structure-instance-runtime PRD authoring session lands its PRD and commits it.
- The session is mechanical; should not require substantive design conversation. If you find yourself making design calls, stop and consider whether the contract's §8 is ambiguous (in which case fix the contract first).
- Expected outputs: 12 task IDs (α–κ + μ + ν), 1 task update (3378), 3 task cancellations (3379/3383/3384).

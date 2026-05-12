# Decompose mode — turn a committed PRD into a queued task batch

Read a committed PRD, re-walk gates, file tasks via fused-memory in a deferred batch, wire dependencies, then flip the batch from `deferred` → `pending`.

## Preconditions

- PRD is committed at `docs/prds/<vM_N>/<slug>.md`. Verify via `git log -1 -- <path>`.
- Fused-memory MCP is reachable (`mcp__fused-memory__get_status`).
- No prior task batch for this PRD already exists (avoid duplicates). Check via `mcp__fused-memory__search(query="<prd slug>", project_id="reify")`.

If the PRD isn't committed yet, stop and tell Leo to either commit first OR (if the PRD was just authored in the same session) re-run the author-mode commit step.

## Flow

### Step 1 — Re-walk gates (fast)

The author mode established G1 / G3 / G4 / G5; this is a drift check, not a re-design.

- **G1 re-check.** For every mechanism in the PRD, scan that a consumer is named. If any mechanism appears without a consumer, escalate to Leo and stop. (Likely cause: author mode finished without G1 fully closed, or a post-commit edit introduced a new mechanism.)
- **G3 re-check.** Run `tree-sitter parse --quiet` on every syntax fixture in the PRD prose. See `references/grammar-gate.md`. Any failure stops the queue.
- **G4 re-check.** Read the cross-PRD relationship table; flag any reciprocal-ownership statements.
- **G5 informational.** Note whether the PRD declared B or B+H. If B+H, look for the integration-gate task in the decomposition plan and confirm it names the boundary-test sketch as its signal.

If anything fails, stop and ask Leo to fix the PRD before queueing.

### Step 2 — G2 walk (the load-bearing decompose-time check)

Enumerate every task in the PRD's decomposition plan. For each:

1. **Classify** leaf vs intermediate.
2. **Find the `user_observable_signal`** the PRD wrote for this task. (Author mode required this per stage 7.)
3. **Find the `consumer_ref`** — the downstream task or user surface.
4. **Find `grammar_confirmed`** — true if the task uses existing grammar, false if it queues grammar work (mark `grammar_prereq_task` in description).

If any leaf task lacks a user-observable signal, **stop** and surface to Leo. This is the audit's dominant failure shape (G2's purpose).

If any intermediate task has no named downstream consumer, surface it. Producer-only intermediate tasks with nothing downstream are a smell — typically the decomposition is missing an integration-gate task.

### Step 3 — File tasks via two-phase pattern (ALWAYS planning_mode=True)

Per memory `procedural_fused_memory_two_phase_writes`: use `submit_task` + `resolve_ticket`; on timeout, poll `get_task` rather than retrying.

Per memory `feedback_planning_mode_scope`: PRD-decomposition batches are the canonical use case for `planning_mode=True`. **Every task in the batch is filed with `planning_mode=True`, no exceptions.** This lands them as `deferred` so the scheduler doesn't pick anything up before the wiring is complete and the batch is flipped together in Step 5.

For each task in the plan, in dependency order (roots first):

```
submit_result = mcp__fused-memory__submit_task(
    title="<task title>",
    description="""<detailed description>

PRD: docs/prds/<vM_N>/<slug>.md task α/β/γ/...

User-observable signal: <signal>
Consumer: <consumer_ref>
Crates touched: <list>
""",
    project_root="/home/leo/src/reify",
    priority="<medium|high|critical>",
    planning_mode=True,
    metadata={
        "source": "prd-decomposition",
        "prd_path": "docs/prds/<vM_N>/<slug>.md",
        "prd_task_label": "α",  # the PRD's own label for this task
        "user_observable_signal": "<signal>",
        "consumer_ref": "<consumer_ref>",
        "grammar_confirmed": True,
        "modules": ["<crate_path>", ...],
    },
)
ticket = submit_result["ticket"]

resolve = mcp__fused-memory__resolve_ticket(
    ticket=ticket,
    project_root="/home/leo/src/reify",
    timeout_seconds=600,
)

if resolve["status"] in ("created", "combined"):
    task_id = resolve["task_id"]
elif resolve["status"] == "failed":
    # Surface to Leo, don't retry blindly
    # See _shared/ticket-failure-handling.md in dark-factory if reason is R4
    stop_and_escalate(resolve["reason"])
```

`combined` is normal — the curator may merge a new task into an existing one if the description is duplicative.

If `submit_task` itself times out (no ticket returned), **don't retry**; poll `get_task` to see whether the write landed asynchronously.

### Step 4 — Wire ALL dependencies (still deferred)

After all tasks have IDs (intra-batch and out-of-batch), wire **every** declared dependency before any status flip. Per memory `preferences_cross_prd_deps_real_edges`, all deps — including cross-PRD — must be real `add_dependency` edges; the scheduler doesn't read metadata.

```
mcp__fused-memory__add_dependency(
    id="<consumer_task_id>",
    depends_on="<producer_task_id>",
    project_root="/home/leo/src/reify",
)
```

Wire intra-batch (Greek-letter prereqs in the PRD map to task IDs you just got from resolve_ticket) and out-of-batch (PRD-declared prereqs to existing tasks elsewhere — e.g. "task 3117" or "compute-node-contract.md task η").

If the decomposition specified `metadata.unblocks` reverse-deps, set those via `update_task`.

Do **not** flip anything to `pending` until every edge in the batch is in. A partially-wired batch with some tasks already `pending` lets the scheduler grab a leaf whose real prereq hasn't been wired yet.

### Step 5 — Flip the whole batch deferred → pending in one call

Per memory `preferences_bookmark_task_pattern` (the converse of bookmark — bookmarks stay deferred, batch decomp ones get flipped pending after wiring lands) and `procedural_set_task_status_semantics` (set_task_status accepts comma-separated IDs).

Flip **every task in the batch together** in a single call — never one-at-a-time, never in dependency-root order. The whole batch becomes schedulable in one atomic moment; the scheduler handles unmet-deps tasks correctly per memory `feedback_blocked_vs_pending_semantics`.

```
mcp__fused-memory__set_task_status(
    id="<id1>,<id2>,<id3>,...",   # comma-separated, all batch IDs
    status="pending",
    project_root="/home/leo/src/reify",
)
```

If a single bulk call is rejected (e.g. payload-size cap), split into the smallest number of bulk calls that fit — still never one-at-a-time.

### Step 6 — Verify

```
mcp__fused-memory__get_tasks(project_root="/home/leo/src/reify")
```

Confirm every task in the batch shows up as `pending`, with the expected dependencies, with the metadata fields the skill wrote.

Print a summary table to Leo:

| PRD label | Task ID | Title | Prereqs | Observable signal |
|---|---|---|---|---|
| α | <id> | <title> | — | <signal> |
| β | <id> | <title> | α | <signal> |
| … | | | | |

### Step 7 — Hand-back

State:
- Number of tasks filed.
- Number of intra-batch and out-of-batch dependencies wired.
- Any tasks that came back as `combined` (and what they were combined into).
- A note that orchestrator-side does **not** currently read `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata — this metadata is substrate for the F-infra follow-up session. The skill wrote it; the orchestrator-side read is a separate design+implement session pair.

## Error handling

- **Curator gate closed / planning_mode batch rejected.** Per `feedback_trickle_ticket_submissions`, do **not** switch to non-planning_mode to paper over. Wait or escalate. PRD-decomp batches are the precise case where planning_mode is correct.
- **`add_dependency` fails** because a referenced task doesn't exist. Likely the out-of-batch prereq is in `deferred` or `cancelled` state; check via `get_task` and resolve with Leo.
- **`set_task_status` rejects with "metadata.files missing"** per memory `project_phantom_done_metadata_files_strip_may09`. Decompose mode shouldn't hit this since the tasks are fresh; if it happens it usually means a stale entry was combined into one of our new tasks. Investigate before retrying.

## Anti-patterns

- Don't peek behind fused-memory at any underlying storage (sqlite files, JSON dumps, etc.). Fused-memory is the only supported interface for task state. If you think you need to look at storage to debug, surface to Leo instead.
- Don't use `planning_mode=False` for the batch and then individually flip statuses to bypass the curator — that's the gameable shortcut the curator exists to prevent. If the curator is wedged, escalate to Leo.
- Don't flip tasks to `pending` one-at-a-time, or in waves as deps land. Wire **everything** first, then flip the **whole batch together** in a single bulk `set_task_status` call.
- Don't file follow-up tasks for things the PRD already covers as Open Questions — those stay in §Open questions, not as queued tasks.

## Resumption (if decompose was started but didn't finish)

If a prior session's decompose mode crashed partway:
1. Search fused-memory for any tasks already filed: `search(query="<prd-slug>", project_id="reify", include_planned=True)`.
2. Match against the PRD's decomposition plan — what's already filed (by `prd_task_label` metadata or title match) and what's missing.
3. Resume at the first missing task; new ones still go in `planning_mode=True` even if some siblings already exist.
4. Wire **all** dependencies (including any that should have been wired by the previous session — re-add is idempotent) before flipping anything.
5. Bulk-flip every batch task that's still `deferred` to `pending` in a single call.

Avoid double-filing. Curator-combining will catch most duplicates but cleanest is to detect existing entries first.

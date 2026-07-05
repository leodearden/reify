# Warm-Lane Degenerate Task-Branch Pointers: Root Cause + Seam Handoff

**Status:** diagnosis complete; reify ships a read-only classifier, DF wires both fix angles
**Task:** #5006
**Precedent / related:** task/4533 (degenerate-branch deletion precedent, 2026-06-23);
task/3572 & task/4168 (recurring degenerate-task-branch recon false-positive, 2026-06-24);
task 3523 (reblock_guard "warm-lane acquire fault" self-heals to `pending`, 2026-06-24);
task 4947 (stale-reblock_guard steward re-escalation loop — a related but distinct churn
family, see §5)

---

## 1. Symptom

A task branch `refs/heads/task/N` exists with **zero commits of its own**: it is a
strict ancestor of `main`, its tip is some **other** task's no-ff merge commit
("Merge task/`<other>` into main"), and the branch has never produced a commit that
cites its own task id. Verified live on this task's own branch before its first
commit:

```
$ git rev-list --count origin/main..task/5006
0
$ git log -1 --oneline task/5006
4971cf2260 Merge task/5004 into main
```

Dark-factory's citation-missing / phantom-done reconciliation sweep
(`orchestrator/merge_queue_store.py`'s `already_merged` check +
`orchestrator/git_ops.py`'s citation regex) treats `is_ancestor(task/N, main) ==
True` as "this looks landed" and looks for a citation of task N to confirm it. It
correctly finds none (the tip cites a *different* task) and correctly refuses to
auto-flip the task to done — but it **re-fires this same check on every
reconciliation pass** while the task stays blocked, producing a steady L1→L2
escalation stream that `resolve close_only` cannot durably stop, because the
underlying ref shape never changes until the task either produces its first
commit (self-heals) or is deleted.

---

## 2. Why the Ref Ends Up Parked This Way

### 2a. The acquire path (dark-factory side)

Dark-factory's `acquire_lane` creates the task branch with:

```
git worktree add -b task/N <lane> <base>
```

`<base>` is a recent `main` commit — frequently **another task's** no-ff merge
commit ("Merge task/`<other>` into main"), since `main` advances by exactly those
merges. At creation, `task/N` therefore starts life with `rev-list --count
main..task/N == 0` (it *is* `main`, or a recent ancestor of it) and a tip that
cites `<other>`, not `N`.

### 2b. The fault-before-first-commit window

This is normally transient: the task makes its first commit, `task/N` moves ahead
of `main`, and the shape self-heals. But if the task **faults or re-blocks before
producing that first commit**, the ref is left parked indefinitely on the foreign
merge-commit base. The reblock_guard signature for this fault class looks like:

```
warm-lane acquire fault for branch 'N' (seed/worktree ...)
```

This is the same acquire-fault signature observed (and shown to be transient
infra, self-clearing to `pending`) on task 3523. The degenerate-ref *shape* it
leaves behind when a task stays blocked for longer, however, is a distinct,
stable condition — not itself transient — and is the subject of this task. The
3580/4388/4875 instances and this branch's own pre-first-commit state on
task/5006 are the same signature: a task blocked (not yet re-dispatched) with its
branch still parked on a foreign merge-commit base.

### 2c. Reify is not the owner of this ref lifecycle

Confirmed by audit: no reify script creates, moves, or deletes `refs/heads/task/*`.
`scripts/seed-warm-lane.sh` only touches `LANE_DIR/target` (see
`docs/design/warm-lane-ref-visibility-seam.md` §2b for the exclusion detail);
`scripts/warm-lane-gc.sh` only removes orphan worktrees, never branch refs; the
only existing reify script that even *reads* a `task/*` ref is the read-only
`scripts/warm-lane-ref-check.sh` (#4855 — see §6 for how that seam differs from
this one). The ref is created and owned exclusively by dark-factory's acquire
path. Per this repo's invariant *"reify ships the primitive, dark-factory wires
the invocation,"* reify's job here is not to create/delete the ref itself but to
ship the shared classification predicate both DF-side fixes need.

---

## 3. The Mathematical Collapse: `count == 0` ⟺ `is_ancestor`

`git rev-list --count <main>..task/N` is the number of commits reachable from
`task/N` but not from `main`. This is `0` **if and only if** `task/N` is an
ancestor of `main` — i.e. `is_ancestor(task/N, main) == True`. So the two
conditions the task's symptom describes ("no commits of its own" and "parked on
a foreign ancestor") are not two independent signals — they are the **same**
signal, `count == 0`.

That collapse is exactly why a naive landed-phantom check re-fires forever on a
degenerate ref: `count == 0` is necessary but not sufficient evidence of
"landed" — a genuinely-landed branch also satisfies it (its own merge commit
*is* on `main` by definition once merged). The **only** thing that distinguishes
"degenerate, never actually landed" from "genuinely landed" is whether the tip
commit **cites its own task id**. That citation check is therefore the entire
discriminant, and it can only be evaluated meaningfully when `count == 0` (when
`count > 0` there are unique commits to inspect and the branch is simply live —
no ambiguity).

---

## 4. The Classifier: `scripts/warm-lane-degenerate-ref-check.sh`

Reify ships a **read-only** diagnostic with two modes. It never creates, moves,
or deletes a ref — proven hermetically by `tests/infra/test_warm_lane_degenerate_ref.sh`'s
byte-identical `git show-ref` before/after assertions on every run.

### 4a. Discriminant

```
degenerate  ⟺  rev-list --count <main>..task/N == 0   AND   tip does NOT cite task N
landed      ⟺  rev-list --count <main>..task/N == 0   AND   tip DOES cite task N
live        ⟺  rev-list --count <main>..task/N  > 0
absent      ⟺  refs/heads/<prefix>N does not exist
```

### 4b. Citation predicate — byte-for-byte agreement with dark-factory

`_cites_task <commit> <id>` mirrors (does **not** import — separate repo)
dark-factory's `orchestrator/git_ops.py` citation regex (~line 145-154, with the
task/1-vs-task/10 substring-safety note ~line 3696):

- a merge-commit subject matching `^Merge <prefix><id> into `, OR
- a `#<id>` reference,

both with **digit-boundary safety**: no adjacent digit before/after the id, so
`task/1` does not match `Merge task/10 into main`, and `#45` does not match
`#4588`. Keeping this predicate byte-for-byte identical to DF's own citation
regex is what lets reify's `degenerate` classification and DF's citation-missing
sweep agree on every ref — if they diverged, the sweep could still re-fire on a
ref reify calls safe to skip.

### 4c. Contract — single-ref mode

```
scripts/warm-lane-degenerate-ref-check.sh --task <id> \
    [--main-ref <ref>] [--branch-prefix <pfx>] [--repo <dir>|-C <dir>]
```

Stdout: exactly one line, `<class> <tip_sha>` (or `absent -`).

| Exit | Class | Meaning |
|---|---|---|
| 0 | `degenerate` | count==0, tip does not cite N — **skip-sweep / prune-safe** |
| 1 | `live` | count>0 — own commits ahead of main |
| 2 | — | usage error |
| 3 | — | structural (not a git work tree, or `--main-ref` unresolvable) |
| 4 | `landed` | count==0, tip DOES cite N — genuinely merged |
| 5 | `absent` | no such ref |

Exit 0 is deliberately the "degenerate" signal (not the conventional
success-with-no-findings 0) so DF can wire the shell idiom directly:
`if warm-lane-degenerate-ref-check.sh --task N; then skip_sweep; fi`.

### 4d. Contract — fleet-audit mode

```
scripts/warm-lane-degenerate-ref-check.sh --audit \
    [--main-ref <ref>] [--branch-prefix <pfx>] [--repo <dir>|-C <dir>] \
    [--status-cmd <cmd>]
```

Enumerates every `refs/heads/<prefix>*` ref (`git for-each-ref`), classifies
each by the same predicate as single-ref mode, and prints one row per ref:

```
<task_id> <class> <tip_sha> [status]
audit: degenerate=.. live=.. landed=.. absent=.. total=.. flagged=..
```

Non-numeric branch names under the prefix (e.g. `task/foo`) are skipped with a
stderr warning; the sweep never aborts on a single bad ref. Exit is always 0 on
a completed sweep — this is a diagnostic, not a gate.

**Optional status oracle** (`--status-cmd <cmd>` / `REIFY_DEGENERATE_REF_STATUS_CMD`):
invoked as `<cmd> <task_id>`, output trimmed of whitespace; empty output or a
non-zero exit is treated as `unknown` (non-terminal). This mirrors
`warm-lane-preflight.sh` Check 6's `REIFY_LANE_LEAK_STATUS_CMD` contract
byte-for-byte. When configured, every row gains a trailing `<status>` column and
`flagged` counts only **degenerate** refs whose status is **not** in
`{done, cancelled}` — the actionable subset. Without an oracle, `flagged` is
simply the raw degenerate count (the primitive stays hermetically testable with
no dependency on DF's task DB).

The `flagged` distinction matters because a degenerate ref on an **in-progress**
task (e.g. this very branch, task/5006, before its first commit) is normal,
transient, self-healing state — not a bug. Only a degenerate ref on a
**non-terminal, stuck** task (blocked, or a done/cancelled task whose ref was
never cleaned up in the *other* direction) is the actionable signal the
reconciler churn comes from.

---

## 5. Seam Handoff: Two DF-Side Fix Angles, One Shared Primitive

Reify cannot action dark-factory code directly (separate repo); this document
and the script are the reify-side deliverable. Both fix angles below consume the
**same** classifier so they can never disagree about what counts as degenerate.

### 5a. Angle B — reconciler skip (recommended primary fix)

DF's citation-missing / phantom-done reconciliation sweep should skip a ref this
script classifies `degenerate`, since it is provably not a landed phantom:

```python
# Pseudocode for DF wiring, in the citation-missing sweep:
check = run(['scripts/warm-lane-degenerate-ref-check.sh', '--task', str(task_id)],
            check=False)
if check.returncode == 0:  # degenerate
    continue  # not a landed phantom; skip this task in the sweep this pass
```

This alone stops the escalation-storm re-fire without deleting anything: the
sweep simply stops mis-treating a pre-first-commit (or permanently stuck)
branch as a landed-but-uncited phantom.

### 5b. Angle A, option 2 — re-block path deletes the ref

When a task re-blocks, DF's re-block path may additionally delete `task/N` if
this script classifies it `degenerate` at that moment:

```python
# Pseudocode for DF wiring, in the re-block handler:
check = run(['scripts/warm-lane-degenerate-ref-check.sh', '--task', str(task_id)],
            check=False)
if check.returncode == 0:  # degenerate
    git_ops.delete_branch(f'task/{task_id}')  # DF owns the delete
```

**DF owns the delete, not reify.** Mutating a shared ref from a reify script
risks tripping the `reference-transaction` main-gate tripwire (see this repo's
CLAUDE.md "Landing on main" invariants), and `warm-lane-ref-check.sh` (#4855)
already set the read-only precedent for this class of primitive. Reify's
contribution stops at classification.

### 5c. Seam ownership table

| Reify ships | DF wires |
|---|---|
| `scripts/warm-lane-preflight.sh` | acquire_lane preflight |
| `scripts/warm-lane-ref-check.sh` (#4855) | steward pre-resolution preflight (ref **visibility**) |
| `scripts/warm-lane-degenerate-ref-check.sh` (**this doc, #5006**) | reconciler skip (angle B) + re-block delete (angle A/2) (ref **content**) |

---

## 6. Distinct From #4855

This is **not** the same bug as `warm-lane-ref-check.sh` / #4855, and the two
scripts are deliberately separate primitives:

| | #4855 (`warm-lane-ref-check.sh`) | #5006 (this doc) |
|---|---|---|
| Failure mode | ref **visibility** — a transient single-shot "branch not found" race during release→acquire churn | ref **content** — a stable ref parked on a foreign main-ancestor |
| Symptom | steward can't *find* a branch that exists | reconciler *finds* a branch and misclassifies its shape |
| Fix shape | bounded retry / preflight before resolution | classify-then-skip (angle B) or classify-then-delete (angle A/2) |
| Stability | self-resolves within one retry window (~seconds) | persists indefinitely until first commit or explicit deletion |

Conflating them into one script would muddy both contracts; they have distinct
exit-code taxonomies and distinct DF call sites.

---

## 7. Cross-Links

| Task/Incident | Relevance |
|---|---|
| task/4533 (2026-06-23) | Precedent: a degenerate branch (no commits beyond main, ancestor of main) was deleted manually — the exact shape this task now classifies automatically. |
| task/3572, task/4168 (2026-06-24) | Recorded as "a recurring degenerate-task-branch recon false-positive issue" — the same re-fire pattern this task's classifier is meant to let DF suppress. |
| task 3523 (2026-06-24) | The acquire-fault reblock_guard signature (`warm-lane acquire fault for branch 'N'`) that precedes a degenerate ref being left behind; shown to self-clear to `pending` when infra recovers before the task is re-dispatched. |
| task 4947 | A **related but distinct** churn family: a stale reblock_guard signature causing steward re-escalation on an already-healthy, verified-green lane. That bug is about a stale *fault signature* on live work; this task is about a stable *ref shape* on stalled/blocked work. Both manifest as unwanted repeated escalation, but the fix locus and mechanism differ. |
| #4855 | Opposite failure mode (ref visibility vs. ref content) — see §6. |

---

## 8. Files Changed (task #5006)

| File | Role |
|---|---|
| `scripts/warm-lane-degenerate-ref-check.sh` | NEW: read-only classifier + fleet-audit primitive (the seam's reify side) |
| `tests/infra/test_warm_lane_degenerate_ref.sh` | NEW: hermetic regression guard (arg-parsing, structural errors, single-ref taxonomy, fleet audit, status-oracle) |
| `scripts/verify-pipeline-infra-tests.txt` | UPDATED: fast-run parity row for the new primitive |
| `docs/design/warm-lane-degenerate-ref-seam.md` | THIS FILE: root-cause + seam handoff |

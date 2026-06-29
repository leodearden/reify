# Warm-Lane Ref-Visibility Race: Root Cause + Seam Handoff

**Status:** diagnosis complete; DF-side fix needed (reify is innocent)  
**Task:** #4855  
**Incidents:** esc-4399-67/68 (`merge-branch-not-found:warm-lane-ref-visibility`);
task 4848 (_lane-13); task 4399 (_lane-46, 12 commits ahead of main)

---

## 1. Symptom

A steward running its merge inside a warm-lane worktree fails with:

```
branch not found in repo (tried 'task/NNNN' and 'NNNN')
```

for a branch that demonstrably **exists** in the shared common git dir at the
time the merge worker later resolves it.  Observed pattern:

| Incident | Lane | Symptom |
|---|---|---|
| task 4848 | _lane-13 | steward: "branch not found"; merge worker lands it fine from _merge-verify |
| task 4399 | _lane-46 (12 commits ahead of main) | esc-4399-67/68 `root_cause: merge-branch-not-found:warm-lane-ref-visibility`; "Failed after 1 attempt" |

The merge worker, resolving the same branch later from its own _merge-verify
lane, succeeds — proving the branch ref exists in the repository throughout.

---

## 2. Reify-Side Innocence

Reify warm-lane provisioning scripts are **audited innocent**:

### 2a. Lane commondir correctly shares refs

A linked worktree `git -C <lane> rev-parse --git-common-dir` resolves to the
main checkout's `.git` directory.  Refs created anywhere (e.g.
`git -C <main> branch task/NNNN`) are immediately visible from the lane because
the lane's commondir **is** the main checkout's `.git`.

Verified on this task's own lane (`_lane-31`):
```
$ git -C /home/leo/src/warm-lanes/worktrees/_lane-31 rev-parse --git-common-dir
/home/leo/src/reify/.git

$ git -C /home/leo/src/warm-lanes/worktrees/_lane-31 rev-parse --verify refs/heads/task/4855
5eea8557f0ee65b8ae3f68c0ebc21aa03c94257b  ✓
```

### 2b. seed-warm-lane.sh never touches refs

`scripts/seed-warm-lane.sh` clones only `LANE_DIR/target` (via
`cp -a --reflink=always`) and bulk-stamps sources pruning `.git/`:

```bash
find "$LANE_DIR" -mindepth 1 \
    \( -path "$LANE_DIR/target" \
       -o -path "$LANE_DIR/.git" \
       -o -path "$LANE_DIR/target.reseed-trash.*" \) -prune \
    -o -exec touch -h -d "2020-01-01T00:00:00" {} +
```

The `.git` exclusion means worktree refs are **never stamped, moved, or
deleted** by the seed primitive.

**Note (#4896, esc-4892-99):** after task #4896 the reseed trash no longer lives
inside the lane.  The old `LANE_DIR/target.reseed-trash.$$` path is now
`$(dirname LANE_DIR)/.reseed-trash/$(basename LANE_DIR).$$` — a pool-level sibling
outside `LANE_DIR`.  The prune clause above is **retained as defense-in-depth** to
guard against any legacy in-lane trash left by pre-#4896 seeds; it matches nothing
for new seeds (trash is structurally outside the lane-rooted walk).  The `.git`
exclusion and ref-innocence invariant are unaffected by this relocation.

### 2c. provision/relocate only set up the XFS mount and path-stable symlink

`scripts/provision-warm-lane-fs.sh` and `scripts/relocate-worktrees-to-warm-lane.sh`
set up the XFS mount and the `<repo>/.worktrees → <mount>/worktrees` path-stable
symlink.  Git stores the **absolute** gitdir path in each worktree's
`.git` file; the symlink ensures the absolute path resolves correctly
after relocation.  These scripts never touch refs.

### 2d. warm-lane-gc.sh / refresh-warm-base.sh do not delete task branches

- `scripts/warm-lane-gc.sh` removes only **orphan worktrees** (lanes with no
  corresponding `_lane-N` pool slot and a stale `gitdir` pointer) — it never
  deletes task branch refs.
- `scripts/refresh-warm-base.sh` reaps only retired base **generation dirs**
  (`.gen.N` directories superseded by a newer generation) — it never touches
  branch refs.

### 2e. Regression guard

`tests/infra/test_warm_lane_ref_visibility.sh` (Block A) proves this
invariant hermetically: ref resolves from a linked worktree **before and
after** a full `seed-warm-lane.sh --fresh-checkout` + `git clean -xfd -e target`
cycle (the full reset_lane provisioning path).  See Block C4 for the
byte-identical `git show-ref` read-only invariant.

---

## 3. Dark-Factory Root Cause

The fault is in the **dark-factory steward's single-shot, no-retry branch
resolution** racing warm-lane lifecycle branch-ref churn.

### 3a. The resolve call chain (DF file:line anchors)

```
escalation/src/escalation/server.py:721  merge_request()
  → git_ops_for_scan.resolve_branch_sha(full_branch)   # server.py:809

orchestrator/src/orchestrator/merge_queue.py:2331  _classify_branch_presence()
  → git_ops.resolve_queued_branch_ref(branch)          # merge_queue.py:2377
    → git_ops.resolve_branch_sha(prefixed)             # git_ops.py:2679
    → git_ops.resolve_branch_sha(branch)               # git_ops.py:2681

orchestrator/src/orchestrator/git_ops.py:2633  resolve_branch_sha()
  → rc, sha, _ = await _run(
        ['git', 'rev-parse', '--verify', f'refs/heads/{branch_name}'],
        cwd=self.project_root,                          # git_ops.py:2643-2646
    )
  → return sha if rc == 0 else None                    # git_ops.py:2647
```

`resolve_branch_sha` is **single-shot**: one `git rev-parse --verify` call,
no retry, no backoff.  If that call returns non-zero, `_classify_branch_presence`
falls through to the `find_merge_marker` check and, finding no marker, emits
`unknown_branch` with:

```python
reason=f'branch not found in repo (tried {prefixed!r} and {branch!r})'
```

The steward escalates after 1 attempt.

### 3b. Pure pack-refs race ruled out

`git rev-parse --verify refs/heads/task/NNNN` reads both loose refs
(`refs/heads/`) and `packed-refs` natively; a loose↔packed race alone cannot
cause `rev-parse` to return non-zero.  The fault is **TOCTOU** (timing), not
a loose-vs-packed visibility gap.

### 3c. The churn window

Dark-factory lane lifecycle performs two operations during a `release_lane` →
`acquire_lane` transition for the **same task branch**:

1. `release_lane` may detach the worktree from `task/NNNN` (or delete the
   branch ref as part of cleanup — this was the structural bug that already
   had a "full decouple" fix land in DF on 2026-06-25T19:25).
2. `acquire_lane` re-attaches the lane via `git worktree add` with the
   existing `task/NNNN` branch.

Between these two operations there is a window where `task/NNNN` either
doesn't exist or its ref file is temporarily absent.  The steward's
single-shot `resolve_branch_sha` landing inside this window sees "branch not
found" even though the branch existed before and after.

The 2026-06-25 "full decouple" fix narrowed the window but did not
eliminate it — the residual is the single-shot/non-retry policy in the
steward.

---

## 4. Seam Handoff: reify ships, DF wires

Reify ships `scripts/warm-lane-ref-check.sh` as a **read-only diagnostic
primitive** that DF can wire as a pre-resolution preflight.

### 4a. The script

```bash
# Usage:
scripts/warm-lane-ref-check.sh \
    --lane <dir> --task <id> \
    [--branch-prefix <pfx>]           # default: "task/"
    [--expect-common-dir <dir>]       # optional reify-provisioning check
    [--retries N]                     # default: 5 (1 = single-shot)
    [--delay S]                       # default: 0.5s

# Stdout: resolved 40-hex SHA on success; nothing on failure
# Exit codes:
#   0 — success (SHA on stdout)
#   1 — ref absent after N retries   ← steward should retry/back-off
#   2 — usage error
#   3 — commondir mismatch            ← reify provisioning regression
```

The script is **read-only**: it never creates, moves, or deletes refs.
Proven by `tests/infra/test_warm_lane_ref_visibility.sh` Block C4.

### 4b. Recommended DF change

Add a bounded retry to `GitOps.resolve_branch_sha` (or add a wrapper that
retries), OR wire `scripts/warm-lane-ref-check.sh` as a pre-resolution
preflight before `_classify_branch_presence`:

```python
# Pseudocode for DF wiring:
preflight = await run(['scripts/warm-lane-ref-check.sh',
                       '--lane', lane_dir,
                       '--task', branch,
                       '--retries', '5',
                       '--delay', '0.5'],
                      check=False)
match preflight.returncode:
    case 0:
        sha = preflight.stdout.strip()   # use this SHA; skip resolve_branch_sha
    case 1:
        # ref absent after 5 retries (0.5s each) — genuine unknown branch
        # proceed to _classify_branch_presence's find_merge_marker check
        pass
    case 3:
        # commondir mismatch — reify provisioning regression
        # escalate with a reify-provisioning hint rather than unknown_branch
        raise MergeError('reify provisioning regression: commondir mismatch')
```

The `--retries 5 --delay 0.5` default (2.5s total wait) is designed to
ride over the transient churn window without significant latency impact.

### 4c. Seam ownership

This follows the established reify/DF seam pattern:

| Reify ships | DF wires |
|---|---|
| `scripts/check-manifold-deps.sh` | verify.sh preflight |
| `scripts/setup-worktree-debug-port.sh` | factory worktree-provision |
| `scripts/warm-lane-preflight.sh` | acquire_lane preflight |
| `scripts/warm-lane-ref-check.sh` | **steward pre-resolution preflight** ← NEW |

Reify cannot action DF code directly; this document and the script are
the reify-side deliverable.

---

## 5. Cross-Links: Session Warm-Lane Integrity Theme

This incident is part of a broader warm-lane integrity investigation:

| Issue | Symptom | Root Cause |
|---|---|---|
| **This doc** (tasks 4848, 4399) | steward: "branch not found" | DF single-shot resolve racing lifecycle churn |
| esc-4171-106 (tasks 4171, 3468) | spurious `E0599: no method 'foo' found` rlib build errors | stale-base rlib mismatch after base refresh without lane reseed |
| esc-4399-67/68 | `merge-branch-not-found:warm-lane-ref-visibility`; "Failed after 1 attempt" | same as this doc |
| 2026-06-25T19:25 DF fix | `branch-ref lifecycle was COUPLED to lane-cache release` | DF full-decouple fix for release_warm_lane deleting task branches |

The stale-base rlib issue (esc-4171-106) is orthogonal: that is a **data
coherence** problem (base refreshed without forcing a lane reseed), not a
**ref-visibility** problem. Both are warm-lane integrity concerns but
require separate fixes.

---

## 6. Files Changed (task #4855)

| File | Role |
|---|---|
| `scripts/warm-lane-ref-check.sh` | NEW: read-only reify primitive (the seam's reify side) |
| `tests/infra/test_warm_lane_ref_visibility.sh` | NEW: hermetic regression guard (Blocks A/B/C) |
| `scripts/verify-pipeline-infra-tests.txt` | UPDATED: drift-guard row for the new primitive |
| `docs/design/warm-lane-ref-visibility-seam.md` | THIS FILE: root-cause + seam handoff |

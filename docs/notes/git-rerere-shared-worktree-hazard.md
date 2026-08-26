# Shared `.git/rr-cache` hazard runbook — `scripts/git-rerere-guard.sh`

**Task #5870 | 2026-08-13 | esc-5785-5**

Operational digest for the git rerere hazard in reify's shared-`.git` warm-lane pool: why rerere is
disabled repo-wide, how to recognise the two failure signatures it produces when it is not, and how
to recover a stuck lane without making things worse.

Every measurement below was reproduced first-hand on git 2.43.0 against throwaway repos and the live
store; the hermetic behavioural oracles live in `tests/infra/test_git_rerere_guard.sh`.

---

## 1. Mechanism

`rr-cache` is a git **COMMON** path. `git rev-parse --git-path rr-cache` resolves to the *common*
git dir from every linked worktree, while `MERGE_RR` and `index` resolve to
`.git/worktrees/<name>/`. Reify has 239 worktrees of one `.git` today, so **all of them share one
`/home/leo/src/reify/.git/rr-cache`** — 241 entries as of 2026-08-13, still growing.

Git takes its only rerere lockfile on the **per-worktree** `MERGE_RR`. That lock therefore provides
**zero cross-worktree mutual exclusion** over the shared payload directory: two lanes writing
`rr-cache/` concurrently are not serialised by anything.

Git exposes no configuration knob to relocate `rr-cache`, so **per-lane cache isolation is
impossible by construction**, and an external lock would have to wrap every `git merge`, `rebase`,
`cherry-pick`, `revert` and `stash pop` issued by 239 lanes plus the orchestrator and every
interactive shell across two repos — unenforceable, and it would serialise unrelated lanes. The only
sound fix is to remove the shared mutable state entirely: turn rerere off.

That costs nothing. `git grep -lI -e rerere -e rr-cache` over all tracked files returns **0 hits** —
nothing in reify references rerere.

## 2. The two hazards

### 2a. Silent cross-lane resolution bleed (the dangerous one)

Demonstrated end-to-end with two linked worktrees. Lane A (task X) resolves a conflict its way; lane
B (unrelated task Y) later merges and git prints:

```
Staged 'f' using previous resolution.
```

Lane B's working tree now holds **task X's resolution**, **already staged** (because
`rerere.autoupdate=true`), with **no conflict markers** and a **clean `git status`**. An agent
running `git commit` commits a foreign resolution having never seen a conflict.

rr-cache ids are content-addressed, so any two lanes hitting the same recurring hunk collide **by
design** — this is rerere working as intended, in an environment where "recurring" spans unrelated
tasks. It has bitten for real: task 5054 had to discard a plausible-but-wrong rerere resolution by
hand.

`rerere.autoupdate` is the half that makes this silent rather than merely wrong, which is why the
guard reports it independently of `rerere.enabled`.

### 2b. The stale `MERGE_RR.lock` signature

A failed rr-cache preimage write leaves a stale **zero-byte `MERGE_RR.lock`** in
`.git/worktrees/<lane>/` — or, for the **main checkout** (whose git dir *is* the common dir), at
`.git/MERGE_RR.lock`. Every subsequent `git commit` in that work tree then exits 128:

```
Unable to create '.../MERGE_RR.lock': File exists.
```

Two competing accounts of the *proximate* write failure exist and this note deliberately does **not**
adjudicate between them:

- **(a)** shared-cache collision/removal — another lane removing an entry mid-write (Mem0 `d4e7ad3a`);
- **(b)** `rr-cache/` sitting outside the landlock write-set in sandboxed lanes, so the `mkdir` of the
  per-conflict subdirectory is denied and the following write reports ENOENT (Mem0 `d3048711`,
  esc-5467-5).

Both are consistent with every observation in the transcripts, and distinguishing them would need a
core-dump analysis or a git bisect this work does not justify. **The fix is agnostic between them:**
with rerere explicitly off, neither path can execute at all.

## 3. The exit-128-yet-lands trap

**This is the real damage vector.** When `git commit` fails on the stale lock, it exits 128 (or
segfaults, 139) **after the commit object has been written and the ref has already moved**. At least
one observed commit segfaulted and landed intact.

A fatal-looking exit that actually succeeded invites a retry — and the retry **double-commits**.

> **ALWAYS run `git log --oneline -1` before retrying any git command that failed in a lane.**
> Confirm whether your commit is already HEAD. Do not reason from the exit code.

`scan-locks` repeats this warning inline on every STALE finding, because the operator reading it is
exactly the person about to retry.

## 4. Noise floor

A bare **`MERGE_RR`** (no `.lock` sibling) is **ORDINARY rerere state**, not the stuck condition. 41
of the live store's worktrees carry one right now, and none of them is broken.

Only **`MERGE_RR.lock`** indicates the stuck condition. Grep for the `.lock` suffix specifically, or
you will wrongly conclude the corruption is fleet-wide. `scan-locks` never matches a bare `MERGE_RR`.

## 5. Recovery — a stuck lane

1. **Check whether your commit already landed** — `git log --oneline -1` in the affected lane. See §3.
2. **Census the locks** — `scripts/git-rerere-guard.sh scan-locks` (read-only, never deletes).
3. **Read the classification.** `OPERATION-IN-PROGRESS` means a live `MERGE_HEAD` / `MERGE_MSG` /
   `CHERRY_PICK_HEAD` / `REVERT_HEAD` / `rebase-merge/` / `rebase-apply/` is present: **do not remove
   the lock** — finish or abort that operation from inside the worktree first, then re-scan.
4. **For a `STALE` finding**, run the exact command it printed, and only that:

   ```
   rm -f <common-git-dir>/worktrees/<lane>/MERGE_RR.lock
   ```

   A finding labelled `<main checkout>` sits one level up — `<common-git-dir>/MERGE_RR.lock`, with no
   `worktrees/<lane>/` component — because the main checkout's git dir *is* the common dir. Trust the
   printed path over this shape.

**NEVER delete `rr-cache/<id>/` while another lane may hold it.** That is precisely the operation
that reproduces the segfault + stale-lock signature — the "cleanup" would re-cause the disease across
239 live lanes. It is also unnecessary: an explicit `rerere.enabled=false` neutralises the residual
cache **in place**, which is measured, not assumed (`test_git_rerere_guard.sh` (f-c)/(f-d) runs a
real conflicted merge against a populated `rr-cache/` and asserts zero new entries).

Deletion stays manual and operator-driven throughout: `scan-locks` prints the command and never runs
it, because it must not reach into another lane's git dir.

## 6. The guard — `scripts/git-rerere-guard.sh`

```
scripts/git-rerere-guard.sh <check|arm|scan-locks> [target_dir]
```

`target_dir` defaults to the repo root one level up from the script. Any worktree of the store
resolves — via `rev-parse --git-common-dir` — to the same shared config and the same `rr-cache`, so
the main checkout and any lane are interchangeable.

| Subcommand | Effect | Exit 0 | Non-zero |
|---|---|---|---|
| `check` | Read-only. Never writes config anywhere. Reads the effective `rerere.enabled` / `rerere.autoupdate`, and sweeps every `config.worktree` in the store — the **main checkout's own** as well as every linked worktree's. | safe | **1** — rerere effectively armed |
| `arm` | Idempotently writes `rerere.enabled=false` + `rerere.autoupdate=false` to shared local config, then re-verifies via `check`. Never prunes `rr-cache`. | disarmed | **2** — shared config pinned, but an out-of-reach override survives; **1** — genuine failure of this run |
| `scan-locks` | Read-only census of `MERGE_RR.lock` across the **main checkout and every linked worktree**, classified STALE vs OPERATION-IN-PROGRESS. Never deletes. | clean | **1** — lock(s) found |

All diagnostics go to **stderr**; stdout stays empty so the exit code is the machine-readable signal.

`scan-locks` covers the main checkout deliberately: for it, git dir == common dir, so its lock lands
at `<common-git-dir>/MERGE_RR.lock` and never under `worktrees/`. The main checkout is an active
merge site — `scripts/land.sh` runs a real `git merge --no-ff` there — so a census that only globbed
`worktrees/*/` would report `clean` while every `git commit` on `main` exits 128.

### `arm` exit 2 — why it is advisory, not fatal

`arm` writes `--local` only, so it can never clear **another lane's** `config.worktree` — and that is
the dominant way its post-write re-verify still reports armed. Exit **2** says "the shared write
succeeded; an override this run cannot reach still wins", and names it. `setup-dev.sh` runs `arm` on
every invocation (beside the main-gate worktree config block) under `set -e`, and treats only exit 1
as fatal: one self-armed lane out of ~239 must not abort everything after that point (the
build-accelerator systemd units, npm, the smoke test) for every developer, with no remediation the
script could offer. On exit 2 it warns and points here.

The `config.worktree` sweep is itself gated on `extensions.worktreeConfig` being true, because git
does not read those files at all while the extension is off — an inert plant would otherwise produce
a false ARMED that `arm` could never clear.

### Why a guard, and not a one-time `git config` write

**git's default for `rerere.enabled` is `-1` = "enabled iff `rr-cache/` exists".** Measured:

| `rerere.enabled` | `rr-cache/` on disk | entries written by a conflicted merge |
|---|---|---|
| unset | present | **1 — rerere is ON** |
| unset | absent | 0 |
| explicit `false` | present | 0 |

Because the shared store carries a residual 241-entry `rr-cache/`, **losing the explicit `false`
silently re-arms the entire fleet**. `git config --unset rerere.enabled` is a **re-arm, not a
no-op**. The value must be present, not merely absent — so this ships as a re-runnable guard.

The value is also not durable here: Claude Code's worktree feature demonstrably clobbers shared
`.git/config` (the reason `setup-main-gate-worktree-config.sh` exists), a stray `core.hooksPath =
echo` was found in that same shared config during this task's earlier audit (esc-5870-1..3), and
**`config.worktree` was measured to BEAT shared config** — so any single lane can re-arm rerere for
itself. That last fact is why `check` **sweeps every `config.worktree` in the store** rather than
reading the shared file alone — and the sweep covers the **main checkout's own
`<common-git-dir>/config.worktree`** as well as every `worktrees/*/config.worktree`. For the main
checkout git dir == common dir, so its per-worktree config sits *beside* the shared config rather
than under `worktrees/`, and a glob over the linked worktrees alone is blind to it. That blind spot
is doubly silent: `check`'s effective-value read only ever sees the *target's own* `config.worktree`,
so from any lane a main-checkout self-arm would be invisible to both detection paths at once and
`arm` would report "disarmed and verified" while `main` stayed armed. The main checkout is also
where `scripts/land.sh` runs its sanctioned `git merge --no-ff` — an active merge site, the same
reasoning that already makes `scan-locks` cover `<common-git-dir>/MERGE_RR.lock`.

`arm` writes with `--local`, never `--worktree`: the point is that all 239 lanes inherit one shared
default with zero per-lane wiring.

## 7. Incident history

esc-5785-5 pairs **`_lane-22`** (triggered by a bad `git stash pop`) and **`_lane-16`** (a plain
`git merge main`), both 2026-07-30. The differing triggers are **irrelevant** — both funnel into the
same shared-rr-cache write path and produce the identical downstream signature.

Transcript evidence corrects two facts in the original escalation:

- the two incidents were **~10h50m apart, not "~55min"**;
- there were **more than two**. `_lane-39` (2026-07-29) preceded both by a day, `_lane-16`/task-5866
  recurred ~34min after the escalation was filed, and recurrences continued through 2026-08-08
  (`_lane-45`, `_lane-47`, `_lane-9` ×2, `_lane-49` ×2).

**The hazard is live as of 2026-08-13**: the shared store still reads `rerere.enabled=true`,
`rerere.autoupdate=true`, with 241 rr-cache entries across 239 worktrees. Closing it needs a one-time
operator run of `scripts/git-rerere-guard.sh arm` against `/home/leo/src/reify` — deliberately **not**
performed from a task commit, since it mutates host-wide shared state for hundreds of concurrently
running lanes. Filed as `escalate_info` on task 5870.

## 8. Open item

**`git fsck` on the shared store remains UNMEASURED.** The 2026-07-30 attempt was killed at a 900s
timeout. It needs a queue-idle window. Until then, whether the concurrent writes left any object-level
damage behind is an open question — the hardening here prevents *new* occurrences, and makes no claim
about the existing store's integrity.

## Pointers

| Topic | Source |
|---|---|
| Guard contract, exit codes, subcommand detail | `scripts/git-rerere-guard.sh` header |
| Behavioural oracles (real conflicted merges against a populated cache) | `tests/infra/test_git_rerere_guard.sh` |
| Sibling shared-`.git` hazard (`refs/stash` is one host-wide ref) | `CLAUDE.md` → "Warm lanes"; `hooks/reference-transaction` |
| Warm-lane pool lifecycle & invariants | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5 |

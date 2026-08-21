# The `_merge-verify` lane-dispatch seam

Cross-repo contract between reify (which ships the primitive) and dark-factory
(which wires the invocation). Task 5608; origin escalation esc-5363-5.

Sibling documents: `warm-lane-ref-visibility-seam.md`,
`warm-lane-degenerate-ref-seam.md`.

---

## 1. Confirmed mechanism

This upgrades esc-5363-5's hypothesis to a code-level finding, traced read-only
through dark-factory.

`<worktree_base>/<lane>.lock` is **one inode** — `verify_cancel.py`'s
`def lane_lock_path(lane_dir: Path) -> Path:` returns
`lane_dir.with_name(lane_dir.name + '.lock')`, i.e. a *sibling* of the lane
directory. Three dark-factory call sites acquire it, on
three different waits:

| Acquirer | Wait | On timeout |
|---|---|---|
| `GitOps.merge_verify_lease` (`git_ops.py`, `_MERGE_VERIFY_LEASE_WAIT_SECS`) | 300s, then **holds for the whole verify** (1–2h) | `MergeVerifyLeaseContended` → `workflow_types.py` `MergeVerifyLeaseContended: BlockDisposition(requeue_kind=REQUEUE, counts_against_requeue_cap=False)` → **retryable**, no escalation |
| `GitOps.reset_persistent_merge_worktree` (`git_ops.py`, `async def reset_persistent_merge_worktree(`) | 30s (`_RESET_WARM_LANE_LOCK_WAIT_SECS`, `git_ops.py` — DF task 3003 split this out of `_SEED_WARM_LANE_LOCK_WAIT_SECS` at the same 30s value) | **the lock acquire** raises `MergeVerifyLeaseContended` (DF task 3003; fail-CLOSED — tree untouched) → `workflow_types.py` `MergeVerifyLeaseContended: BlockDisposition(category=NONE, escalate_to_human=False, requeue_kind=REQUEUE, counts_against_requeue_cap=False)` → caught by `merge_queue.py` `_run_inflight_verify`'s defer arm, placed **before** its generic `except Exception` → `InflightStatus.REQUEUED` with `req.result` left PENDING → **transient DEFER, not a merge failure**. Two bounds: git faults *inside the method body* still raise plain `RuntimeError` and still resolve `blocked` (deliberate — "so a genuine git fault still classifies as blocked"); and continuous contention past `MAX_CONTENDED_LEASE_DEFER_SECS` (`merge_queue.py`, 4h) does terminally resolve `MergeOutcome('blocked')` |
| `_seed_warm_lane` (`git_ops.py`, `async def _seed_warm_lane(`) | `flock -x -w <_SEED_WARM_LANE_LOCK_WAIT_SECS> -E <_SEED_WARM_LANE_LOCK_TIMEOUT_RC>` — assembled from those two constants (currently 30 / 124); no such literal string exists anywhere in DF source | same 30s constant |

**That asymmetry on one inode WAS the defect**, not speculation about load — and
it is now HISTORICAL: DF task 3003 closed it upstream (verified at DF HEAD
`7cb0ef2e0c`, 2026-08-21). Recorded here because it is esc-5363-5's signature
and nothing else in this repo states it: a lease held for ~2h starves a 30s
waiter, and the two paths *then* disagreed about what a timeout on the *same
lock* meant — 300s said "requeue", 30s said "this merge failed". Pre-3003, a
plain `RuntimeError` fell through to `merge_queue.py`'s generic `f'Verification
error: {exc}'` handler in `_run_inflight_verify` → `MergeOutcome('blocked',
'Verification error: Timed out after 30s...')` → `workflow.py` `category =
'merge_error'` → `_mark_blocked` → escalation, terminal, never requeued. That
reason string is deterministic, so every attempt carried an identical
`merge_outcome_signature` — which is what tripped `workflow.py`'s
`consecutive_merge_thrash` ladder into a false-positive human escalation. That
is exactly the observed esc-5363-5 signature (task 5384's lease held the inode
for ~2h; task 5363's `reset_persistent_merge_worktree` waited 30s and died into
a `merge_error` escalation).

Note the base class was never the discriminator: `MergeVerifyLeaseContended`
**is-a** `RuntimeError`. "Plain `RuntimeError`" above had the base class right
and the classification wrong, which is precisely how a reader mis-derives this
seam — read the disposition table and the handler order, not the type's ancestry.

Both bounded waits now classify an acquire timeout the same way, so the
disagreement is gone. What survives is **the wait itself**: a starved dispatch
still burns the full 30s before deferring, then pays a requeue and re-dispatch
cycle. That cost is what §4(a)'s pre-dispatch consult avoids, and it is why the
reify-side guard's reason to exist is untouched by DF 3003.

Two further bounds keep "requeued, never escalates" from being unconditional
even inside the contended family. `LaneLockSelfOwnedLeak` (`git_ops.py`) is-a
`MergeVerifyLeaseContended` and keeps REQUEUE with no cap burn, but flips
`escalate_to_human=True` — a lane lock the kernel attributes to *this* process
with no registered in-process hold is a leaked fd, and nothing releases it
before process exit, so deferring can never succeed. (On the merge-worker path
that row is not what fires: the defer arm catches the leak as its parent class
first, and the loud first-occurrence signal is the `logger.error` at the
detection site in `GitOps._lane_lock_self_owned_leak`. The row governs `cli.py`
`verify-merge` and workflow block classification.) Separately, a fail-CLOSED
pre-check raises `MergeVerifyLeaseHeld` — same disposition row, same defer arm —
when a **different** live pgid holds the merge-verify lease.

`_RESET_WARM_LANE_LOCK_WAIT_SECS` is a hardcoded module constant. There is **no**
yaml key and **no** env override for it anywhere: zero hits for
`lock_timeout` / `lock_wait` / `flock_wait` / `lane_lock` across DF `config.py`,
DF `defaults.yaml`, and reify's `dark-factory-orchestrator.yaml`. Raising it
requires a dark-factory code change.

## 2. Hypothesis correction — do not re-file this

Speculative dispatch is **not** required to reproduce the failure, and
provisioning more verify lanes does **not** fix it.

`git.merge_spec_warm_lane_pool: true` has been live since task 4941
(`dark-factory-orchestrator.yaml:627`). With it, `merge_liveness.py`'s
`lane_path, warm = await git_ops.acquire_spec_lane(merge_commit)` routes
SPECULATIVE items to `_spec-N` lanes — **not** to `_merge-verify`. Only the
serial-head path (`merge_liveness.py`, `await
git_ops.reset_persistent_merge_worktree(merge_commit)`) and the periodic
safety valve touch `_merge-verify` at all.

So the candidate remediation "provision verify lanes to match
`speculation.depth`" is already live and did not prevent this. Recorded here so
a future reader does not re-derive it and re-file it as new work.

## 3. Reify-side contract (shipped by task 5608)

`scripts/warm-lane-lock-guard.sh` — a read-only availability oracle for one
warm-lane lock. The **lock axis** sibling of `warm-lane-disk-guard.sh`'s **disk
axis**.

```
scripts/warm-lane-lock-guard.sh check [--mount DIR] [--lane NAME] [--lock-path PATH]
```

| Option | Env | Default |
|---|---|---|
| `--mount DIR` | `REIFY_WARM_LANE_MOUNT` | — (required unless `--lock-path`) |
| `--lane NAME` | `REIFY_WARM_LANE_LOCK_GUARD_LANE` | `_merge-verify` |
| `--lock-path PATH` | `REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH` | derived |
| *(test seam)* | `REIFY_WARM_LANE_LOCK_GUARD_FLOCK` | `flock` |

**Exit codes.**

| Code | Meaning | stdout |
|---|---|---|
| `0` | IDLE — no exclusive holder observed | *empty* |
| `3` | BUSY — an exclusive holder was **positively** observed | exactly one line: `@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=<n> lock=<p>` |
| `2` | usage error — a wiring bug, not a verdict | *empty* |

Exit `3` is the pinned cross-repo throttle value, not a fresh dialect:
`docs/prds/warm-lane-pool-sizing-lifecycle.md:212-219` fixes it as "distinct
from the hard-floor 75 (`EX_TEMPFAIL`) AND the E1 config-error 2", and
`warm-lane-disk-guard.sh --soft` and `fleet-load-detector.sh` already emit it for
the same *throttle, do not requeue* meaning. The correct instruction here is
**defer this dispatch** — the item is fine, the lane is momentarily occupied —
which is throttle, not requeue.

**Lock derivation** is `<mount>/<lane>.lock`, a **sibling** of the lane dir,
byte-matching DF's `lane_lock_path()`. `--mount` is the *worktrees dir* — the
value DF already passes to every warm-lane script (`str(self.worktree_base)`;
see `scripts/warm-lane-gc.sh:120-136`). Getting this wrong yields a guard that
reports IDLE forever, so it is pinned by exact string equality plus a decoy test
(`tests/infra/test_warm_lane_lock_guard.sh`, block E).

**Invariants.** This is their single normative statement — the script header
carries a one-line-each summary and points here for the reasoning, so this is
the copy to amend when one of them changes.

- **A1 — non-mutating.** Never creates, truncates, or changes the lock file or
  the mount. Read-only open on an existing path; a missing lock file is IDLE and
  is never brought into existence. No `>`-open, no `touch`, no `mkdir`, and
  deliberately not the `flock <file> <cmd>` convenience form.
- **A2 — shared, non-blocking, point-in-time.** `flock -n -s`, released at once.
  A shared request still detects a live consumer's exclusive hold, but two
  concurrent oracles never contend. The verdict is a *sample*: the lane can be
  taken the instant after IDLE is reported. Accepted, per A4.
- **A3 — fail-open.** Any probe-infrastructure failure (flock missing or broken,
  lock unreadable, mount absent, unrecognised flock status) yields exit 0 with a
  stderr warning and **no sentinel** — never exit 3. A false BUSY would defer
  dispatch indefinitely and wedge the serial merge queue; a false IDLE merely
  restores today's behaviour. This is the *opposite* of disk-guard's fail-closed
  calculus, where an unmeasurable disk must be assumed full to avoid ENOSPC.
- **A4 — advisory backpressure only.** Never requeues, never escalates, and is
  not the correctness mechanism for lane exclusivity. DF's own bounded-wait
  flock remains that.

Telling *would-block* from *tool error* is the load-bearing implementation
detail: `flock -n` returns a bare `1` on contention, indistinguishable from
"flock itself failed". The guard therefore asks for a distinct conflict status
via `-E 124` — mirroring the conflict-status value DF's `_seed_warm_lane`
(`git_ops.py`) assembles from its own `_SEED_WARM_LANE_LOCK_TIMEOUT_RC`
constant (currently 124; DF builds the whole invocation from constants, not a
quoted literal — see §3) — and treats every other non-zero as a degradation.

**Known divergence from `warm-lane-audit.sh`.** Two scripts now probe the same
`<mount>/<lane>.lock` inode with the same read-only shared-`flock` technique —
audit's `_probe_live` and this guard's `_probe` — and they disagree on exactly
one question. Audit uses a bare `flock -n -s` and reads *every* non-zero as LIVE,
conflating a broken or missing `flock` with contention: it fails **closed**. The
guard asks for `-E 124` and fails **open** on anything that is not that exact
status. Both are right for their own consumer — audit's output is advisory prose
a human reads, where over-reporting LIVE merely looks conservative; the guard's
exit 3 gates dispatch, where a false BUSY would wedge the serial merge queue.

The consequence worth knowing: on a host with a degraded `flock`, audit will
report a lane LIVE while the guard reports it IDLE, and no shared code path keeps
them honest. Unifying them behind a tri-state helper (`IDLE` / `BUSY` /
`UNMEASURABLE`, each caller applying its own fail direction to the third) is the
right end state; it needs to touch `scripts/warm-lane-audit.sh`, which is outside
task 5608's lock set, so it is filed as follow-up rather than done here.

There is deliberately **no** `--wait N` mode and **no** holder-PID attribution.
Waiting policy is the contended half of the seam and belongs to DF, which
already owns three waits on this inode; a fourth would recreate the very
asymmetry described in §1. Attribution would need `fuser` / `/proc` scraping,
and DF already writes its own holder pgid.

## 4. Dark-factory wiring still required

This is the half that actually ends the failure mode. Either fix works; (b)
alone stops the escalation, (a) additionally avoids burning the wait.

**(a) Consult the guard before dispatching onto `_merge-verify`, and defer on 3.**

This snippet is the seam's single rendering — the script header points here
rather than carrying a second copy. (It carried one until review: the *same*
errexit bug was present in both copies, which is the argument against hand-syncing
them.)

```sh
# `exit_code=0; ... || exit_code=$?` — NOT a bare assignment followed by
# `exit_code=$?`. An assignment whose value comes from a command substitution is
# a simple command for errexit purposes, so under `set -e` the caller would abort
# AT THE ASSIGNMENT on exit 3 — never reaching the one branch this snippet exists
# to demonstrate. The `||` list exempts it.
exit_code=0
busy=$(bash scripts/warm-lane-lock-guard.sh check --mount "$worktree_base" \
                                                  --lane _merge-verify) || exit_code=$?
if [ "$exit_code" -eq 0 ]; then
    : # IDLE — dispatch the verify onto this lane.
elif [ "$exit_code" -eq 3 ]; then
    : # BUSY — DEFER and retry later. "$busy" carries lane= and lock= for logs.
else
    : # 2 — a wiring bug on our side. Log it and treat as 0: the guard is
      # advisory (A4), so a broken consult must never block the queue.
fi
```

**(b) Give the 30s `reset_persistent_merge_worktree` path the requeue
disposition the 300s lease path already has** — `workflow_types.py`
`BlockDisposition`, so lock contention on this inode is never classified
`merge_error`.

**Proposed knob block — NOT YET ADDED, and must not be added alone:**

```yaml
warm_lane_pool:
  # Consult scripts/warm-lane-lock-guard.sh before dispatching a verify onto
  # the _merge-verify lane; defer the dispatch on exit 3.
  lock_guard_enabled: true
  lock_guard_script: scripts/warm-lane-lock-guard.sh
  lock_guard_lane: _merge-verify
```

Adding this to `dark-factory-orchestrator.yaml` **before** DF's config model
declares it would be silently dropped: `OrchestratorConfig` uses
`extra='ignore'`, so an undeclared key is inert while looking authoritative.
That is the exact phantom-key failure that left top-level `spare_warm_lanes`
inert from 2026-06-29 until task 5358 moved it under `GitConfig`. Land the yaml
key together with the DF code that reads it.

(Adding it early would also drag in `tests/infra/test_warm_lane_pool_config.sh`,
which regex-cross-checks every yaml `warm_lane_pool.*` value against the owning
script's `${REIFY_...:-<literal>}` fallback.)

## 5. Scope bound

**Task 5608 does not by itself eliminate the spurious 30s lock-timeout
`merge_error`.** It ships the reify primitive and pins this contract; the
failure mode ends only when the dark-factory half in §4 lands.

Consequently no test in `tests/infra/test_warm_lane_lock_guard.sh` asserts an
end-to-end merge-queue outcome — that capability needs a live orchestrator, a
real merge queue, and a ~2h contending verify, none of which are reachable from
a reify worktree. Such a test would be permanently red for reasons no reify
implementer could fix, and indistinguishable from a genuine defect. What the
suite does pin is the contract DF will consume: exit code, sentinel grammar,
lock-path derivation, fail-open degradation, and non-mutation.

Follow-up filed for the dark-factory half: ticket
`tkt_0RRT3T4TJKFQY5MCJA9ZF6Z2Z7` (`spawned_from: 5608`).

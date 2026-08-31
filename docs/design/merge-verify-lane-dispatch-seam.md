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
directory. Four dark-factory call sites acquire it, each on its own
independently-tunable wait constant:

| Acquirer | Wait | On timeout |
|---|---|---|
| `GitOps.merge_verify_lease` (`git_ops.py`, `_MERGE_VERIFY_LEASE_WAIT_SECS`) | 300s, then **holds for the whole verify** (1–2h) | `MergeVerifyLeaseContended` → `workflow_types.py` `MergeVerifyLeaseContended: BlockDisposition(requeue_kind=REQUEUE, counts_against_requeue_cap=False)` → **retryable**, no escalation |
| `GitOps.reset_persistent_merge_worktree` (`git_ops.py`, `async def reset_persistent_merge_worktree(`) | 30s (`_RESET_WARM_LANE_LOCK_WAIT_SECS`, `git_ops.py` — DF task 3003 split this out of `_SEED_WARM_LANE_LOCK_WAIT_SECS` at the same 30s value) | **the lock acquire** raises `MergeVerifyLeaseContended` (DF task 3003; fail-CLOSED — tree untouched) → `workflow_types.py` `MergeVerifyLeaseContended: BlockDisposition(category=NONE, escalate_to_human=False, requeue_kind=REQUEUE, counts_against_requeue_cap=False)` → caught by `merge_queue.py` `_run_inflight_verify`'s defer arm, placed **before** its generic `except Exception` → `InflightStatus.REQUEUED` with `req.result` left PENDING → **transient DEFER, not a merge failure — on the merge-worker path**. The same lock acquire, reached instead from `cli.py` `verify-merge` via `acquire_host_verify_worktree`, is never caught there (zero `except`-Lease arms in `cli.py`) and is a terminal bail instead of a requeue — in `cli.py`'s own words, "30s timeout -> RuntimeError -> verify exits without ever building" (§1's closing paragraphs, below). Two bounds: git faults *inside the method body* still raise plain `RuntimeError` and still resolve `blocked` (deliberate — "so a genuine git fault still classifies as blocked"); and continuous contention past `MAX_CONTENDED_LEASE_DEFER_SECS` (`merge_queue.py`, 4h) does terminally resolve `MergeOutcome('blocked')` |
| `_seed_warm_lane` (`git_ops.py`, `async def _seed_warm_lane(`) | `flock -x -w <_SEED_WARM_LANE_LOCK_WAIT_SECS> -E <_SEED_WARM_LANE_LOCK_TIMEOUT_RC>` — assembled as an argv **list** from those two constants (currently 30 / 124). DF's PRODUCTION code never carries this as a quoted literal, so reify must mirror the VALUES and never pattern-match a string. (DF's own `orchestrator/tests/test_ephemeral_worktree.py` *does* carry the expanded literal, as a test-side assertion — see §3.) | fail-CLOSED at the lock: `rc == _SEED_WARM_LANE_LOCK_TIMEOUT_RC` is logged as a distinct diagnosable timeout ("failing closed rather than risk a torn target/") and returned to callers, which read any non-zero as a seed fault and degrade to a **cold** worktree — fail-soft, the lane is never removed and the scheduler never blocks. No retry inside the method. Same VALUE as the reset row (30) but a **separate** constant since DF 3003 |
| `GitOps.task_verify_lease` (`git_ops.py`, `async def task_verify_lease(`) — DF task 3027 | 300s (`_TASK_VERIFY_LEASE_WAIT_SECS`), then **holds for the whole task-lane verify** | **fail-OPEN**: logs a WARNING and yields *without* the hold rather than raising. A task verify must never be aborted by its own lane lease, and proceeding unheld is exactly the pre-3027 baseline, so fail-open is non-regressive. No merge-queue disposition is involved on this path at all |

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

`task_verify_lease` is on the table for inode completeness, but it never touches
`_merge-verify`: its call sites (`workflow.py`, `async with
self.git_ops.task_verify_lease(self.worktree)`) always pass the *task's own*
worktree, so it contends only with reify's per-lane `warm-lane-gc.sh` /
`thin-warm-lane.sh` on a task lane. It is on a different lane from the three
rows above, not a fourth competitor for the merge lane, and the guard's
`--lane` default of `_merge-verify` is what keeps the two populations apart.

A second ancestry-style trap lives in that row, and it is worth naming because a
review of this very document fell into it. `task_verify_lease`'s docstring reads
"Holds the SHARED `<lane_dir>.lock`" — **`SHARED` there means the shared per-lane
*inode*** (the one reify's scripts and DF both serialize on), **not** a `LOCK_SH`
flock mode. Both leases acquire through
`GitOps._acquire_lane_flock_off_thread` → `verify_cancel.py`
`acquire_merge_verify_flock`, which polls `fcntl.flock(fd, fcntl.LOCK_EX |
fcntl.LOCK_NB)` and takes no lock-mode parameter at all; `LOCK_SH` has zero hits
across `orchestrator/src/`. So A2's premise (§3) is exact as written — every
real consumer holds an **exclusive** flock while live, and this guard's shared
`flock -n -s` probe therefore *does* observe a live `task_verify_lease` hold.
Read the flock mode at the acquire helper, not the adjective in the docstring.

Two neighbouring facts are what make that trap easy to fall into, so pin them
here. First, there **is** a real `flock -s` in this subsystem — but on a
*different inode*: `_seed_warm_lane` nests `flock -x <lane_dir>.lock` around
`flock -s <gen_dir>.lock`, a reader-refcount hold on the per-**gen** lock that
lets a concurrent reify GC rewrite defer instead of tearing the `cp`. Grepping
DF for `flock -s` hits that, not a shared hold on the lane lock. Second, on
`task_verify_lease`'s fail-open path nothing is held *at all* (`fd is None`), so
a probe then reads IDLE correctly — that is the documented fail-open, not a
shared-lock blind spot.

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
via `-E 124`, and treats every other non-zero as a degradation.

That 124 is chosen only to be *distinguishable from flock's bare 1*, and it is
consumed exclusively by this script's own probe: the guard passes it to its own
`flock -n -s -E "$FLOCK_CONFLICT_RC"` and compares the result against that same
shell variable. It matches DF's current `_SEED_WARM_LANE_LOCK_TIMEOUT_RC` by
**convention** — both echo `timeout(1)`'s 124 — and that is the whole of the
relationship. There is no coupling in either direction: DF never observes this
guard's exit codes (it is unwired — §4(a)), and the guard never observes DF's
flock rc. **A DF retune of that constant would leave this guard entirely
correct, and must not be chased here** — chasing it would add exactly the
cross-repo pin the rest of this section tells reify not to create.

DF's side of that convention is assembled, in *production*, as an argv list from
`_SEED_WARM_LANE_LOCK_WAIT_SECS` / `_SEED_WARM_LANE_LOCK_TIMEOUT_RC` — never a
quoted literal (§1). That qualifier is load-bearing in both directions. Reify
must not scrape DF for a quoted `flock -x -w 30 -E 124` string, because
production never emits one. But the literal is not absent from the repo either:
`orchestrator/tests/test_ephemeral_worktree.py` pins the **expanded** form
(`assert cmd[:6] == ['flock', '-x', '-w', '30', '-E', '124']`) rather than
deriving it from the two constants — so a retune of either constant needs that
test updated in lockstep. That is precisely the coupling this section warns
reify not to create on its own side; noted here so the next reader sees the
grep hit and does not mistake it for a production literal.

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
already owns four waits on this inode family (§1); a fifth would recreate the
very asymmetry described there. Attribution would need `fuser` / `/proc` scraping,
and DF already writes its own holder pgid.

## 4. Dark-factory wiring still required

This is the half that actually ends the failure mode. Either fix works; (b)
alone stops the escalation, (a) additionally avoids burning the wait. **(b) has
since LANDED upstream** (below), leaving (a) as the only outstanding item.

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

**(b) ~~Give the 30s `reset_persistent_merge_worktree` path the requeue
disposition the 300s lease path already has~~ — LANDED upstream as DF task
3003.** Verified at DF HEAD `7cb0ef2e0c` (2026-08-21): the path's lock acquire
raises `MergeVerifyLeaseContended`, `workflow_types.py` carries its REQUEUE /
no-cap-burn `BlockDisposition` row, and `merge_queue.py` `_run_inflight_verify`
requeues it in a defer arm ahead of the generic handler. Contention on this
inode is therefore no longer classified `merge_error`. Kept here, struck rather
than deleted, because §1's historical chain and esc-5363-5 both refer to it; see
§1 for the four bounds that keep the fix from being unconditional.

Only **(a)** remains outstanding, and it is genuinely unlanded — at the same DF
HEAD, zero hits for `lock_guard_enabled`, `warm-lane-lock-guard` or
`WARM_LANE_LOCK_BUSY` anywhere in DF source, and no `lock_guard` key in reify's
`dark-factory-orchestrator.yaml`. Its value survives (b): a contended dispatch
that is *deferred upstream of the acquire* never burns the 30s bounded wait, nor
the requeue and re-dispatch cycle that now follows it.

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

**Task 5608 does not by itself eliminate the cost of a contended 30s lock
wait.** The spurious `merge_error` it was originally scoped against is gone —
DF task 3003 reclassified that timeout as a defer (§1, §4(b)). What a reify-side
primitive still cannot fix alone is the wait: with no pre-dispatch consult, DF
burns the full 30s bounded wait on an inode it could have known was held, then
pays a requeue and re-dispatch cycle. Task 5608 ships the primitive and pins
this contract; that remaining cost ends only when the §4(a) half lands in
dark-factory.

Consequently no test in `tests/infra/test_warm_lane_lock_guard.sh` asserts an
end-to-end merge-queue outcome — that capability needs a live orchestrator, a
real merge queue, and a ~2h contending verify, none of which are reachable from
a reify worktree. Such a test would be permanently red for reasons no reify
implementer could fix, and indistinguishable from a genuine defect. What the
suite does pin is the contract DF will consume: exit code, sentinel grammar,
lock-path derivation, fail-open degradation, and non-mutation.

Follow-up filed for the dark-factory half: ticket
`tkt_0RRT3T4TJKFQY5MCJA9ZF6Z2Z7` (`spawned_from: 5608`).

# The `_merge-verify` lane-dispatch seam

Cross-repo contract between reify (which ships the primitive) and dark-factory
(which wires the invocation). Task 5608; origin escalation esc-5363-5.

Sibling documents: `warm-lane-ref-visibility-seam.md`,
`warm-lane-degenerate-ref-seam.md`.

---

## 1. Confirmed mechanism

This upgrades esc-5363-5's hypothesis to a code-level finding, traced read-only
through dark-factory.

`<worktree_base>/<lane>.lock` is **one inode** — `verify_cancel.py:373-387`
`lane_lock_path()` is `lane_dir.with_name(lane_dir.name + '.lock')`, i.e. a
*sibling* of the lane directory. Three dark-factory call sites acquire it, on
three different waits:

| Acquirer | Wait | On timeout |
|---|---|---|
| `GitOps.merge_verify_lease` (`git_ops.py:2299-2313`) | 300s, then **holds for the whole verify** (1–2h) | `MergeVerifyLeaseContended` → `workflow_types.py:484-491` `BlockDisposition(requeue_kind=REQUEUE, counts_against_requeue_cap=False)` → **retryable**, no escalation |
| `GitOps.reset_persistent_merge_worktree` (`git_ops.py:9017-9029`) | 30s (`_SEED_WARM_LANE_LOCK_WAIT_SECS`, `git_ops.py:371`) | plain `RuntimeError` → `merge_queue.py:13846-13864` generic handler → `MergeOutcome('blocked', 'Verification error: Timed out after 30s...')` → `workflow.py:8417-8444` `category='merge_error'` → `_mark_blocked` → **escalation. Terminal, never requeued** |
| `_seed_warm_lane` (`git_ops.py:3495-3505`) | `flock -x -w 30 -E 124` | same 30s constant |

**The defect is that asymmetry on one inode**, not speculation about load: a
lease held for ~2h starves a 30s waiter, and the two paths disagree about what a
timeout on the *same lock* means — 300s says "requeue", 30s says "this merge
failed". That is exactly the observed esc-5363-5 signature (task 5384's lease
held the inode for ~2h; task 5363's `reset_persistent_merge_worktree` waited 30s
and died into a `merge_error` escalation).

`_SEED_WARM_LANE_LOCK_WAIT_SECS` is a hardcoded module constant. There is **no**
yaml key and **no** env override for it anywhere: zero hits for
`lock_timeout` / `lock_wait` / `flock_wait` / `lane_lock` across DF `config.py`,
DF `defaults.yaml`, and reify's `dark-factory-orchestrator.yaml`. Raising it
requires a dark-factory code change.

## 2. Hypothesis correction — do not re-file this

Speculative dispatch is **not** required to reproduce the failure, and
provisioning more verify lanes does **not** fix it.

`git.merge_spec_warm_lane_pool: true` has been live since task 4941
(`dark-factory-orchestrator.yaml:627`). With it, `merge_liveness.py:697-708`
routes SPECULATIVE items to `_spec-N` lanes — **not** to `_merge-verify`. Only
the serial-head path (`merge_liveness.py:712-720`) and the periodic safety valve
touch `_merge-verify` at all.

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

**Invariants** (also carried in the script header):

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
via `-E 124` — mirroring DF's own `flock -x -w 30 -E 124` — and treats every
other non-zero as a degradation.

There is deliberately **no** `--wait N` mode and **no** holder-PID attribution.
Waiting policy is the contended half of the seam and belongs to DF, which
already owns three waits on this inode; a fourth would recreate the very
asymmetry described in §1. Attribution would need `fuser` / `/proc` scraping,
and DF already writes its own holder pgid.

## 4. Dark-factory wiring still required

This is the half that actually ends the failure mode. Either fix works; (b)
alone stops the escalation, (a) additionally avoids burning the wait.

**(a) Consult the guard before dispatching onto `_merge-verify`, and defer on 3.**

```sh
busy=$(bash scripts/warm-lane-lock-guard.sh check --mount "$worktree_base" \
                                                  --lane _merge-verify)
exit_code=$?
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

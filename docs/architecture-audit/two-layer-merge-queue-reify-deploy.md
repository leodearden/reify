# Two-layer merge queue (ξ): reify CAPSTONE deploy runbook

**Status (2026-06-24):** Pending restart. κ/4750 (CrateGraphOverlapDetector) is
on main; ν/dark_factory:1897 (two-layer pipeline + startup wiring) is deployed.
One out-of-band orchestrator restart is required to activate the detector.

Design: `docs/prds/two-layer-merge-queue.md` (ξ CAPSTONE)

---

## Overview

ξ (#4751) is the reify-side CAPSTONE of the two-layer merge queue PRD. Restarting
`orchestrator-reify.service` loads κ's `CrateGraphOverlapDetector`
(`scripts/reify_overlap_detector.py`, landed on main via task 4750 / merge
`1aa4f640b5`) ON TOP of ν's already-deployed two-layer pipeline.

`orchestrator-reify.service` is a **long-lived process**: it loads
`orchestrator.yaml` once at startup. Landing κ/4750 on `main` does **not**
activate the detector — "merged ≠ deployed." A single orchestrator restart is
required for ν's `register_for_reify()` startup call (dark_factory:1897) to wire
the `CrateGraphOverlapDetector` under project id `"reify"`.

---

## Preconditions

Before restarting, verify all three are true:

| # | Condition | Check |
|---|---|---|
| 1 | κ/4750 on main | `git -C /home/leo/src/reify log --oneline \| grep 1aa4f640b5` |
| 2 | ν/dark_factory:1897 deployed | `python3 -c 'import orchestrator.overlap_footprint'` exits 0 |
| 3 | `project_root` clean | `git -C /home/leo/src/reify status --porcelain --untracked-files=no` (empty output) |

Condition 3 is enforced by `scripts/orchestrator-redeploy-restart.sh` itself —
it refuses to schedule the restart and exits non-zero if the main checkout has
uncommitted tracked changes.

---

## Restart command

The **only sanctioned path** is `scripts/orchestrator-redeploy-restart.sh`
(schedule mode). Run from any shell that is **not** under the orchestrator:

```bash
bash /home/leo/src/reify/scripts/orchestrator-redeploy-restart.sh
```

**What it does:**

1. Checks `project_root` (`/home/leo/src/reify`) is clean. Exits non-zero if
   dirty — schedules nothing.
2. Schedules a transient systemd unit (default delay: 60 s) that fires after
   the triggering process exits:
   - Stops `orchestrator-reify.service` (blocking, up to `TimeoutStopSec=90`
     graceful window — cancels in-flight tasks, reaps agents, releases the
     fcntl lock).
   - Starts `orchestrator-reify.service`.
3. The transient unit re-checks `project_root` is clean before the stop/start.
   If dirty at fire time, the old orchestrator is left running.

**Never use:**

- `systemctl --user restart orchestrator-reify.service` — `restart`'s start-half
  is cancelled inside the `TimeoutStopSec=90` graceful-stop window, leaving the
  service down.
- `git update-ref refs/heads/main` / raw `commit-tree` plumbing — trips the
  `reference-transaction` tripwire and skips the verify gate.
- Any direct `systemctl restart` from a task agent running under the orchestrator
  — self-kill; causes cancel+requeue livelock (see "Operator action required"
  below).

---

## Verify

After the restart completes, run the deploy-readiness smoke:

```bash
bash tests/infra/test_reify_overlap_deploy_smoke.sh
```

Expected output (orchestrator venv active, γ seam importable):

```
=== test_reify_overlap_deploy_smoke ===
  PASS: python3 is available
  PASS: register_for_reify() causes changesets_overlap(reify, ...) to return True for same-crate pair
  PASS: DEFAULT path detector returns False for same-crate/different-file pair (unregistered project id)

Results: 3 passed, 0 failed
```

### Live heartbeat (the genuine RED → GREEN signal)

The smoke above is hermetic (subprocess, snapshot/restore of `_DETECTORS`) and
passes both before and after the restart once ν is deployed. The genuine
pre/post-restart signal is the **live orchestrator heartbeat**:

- **Before restart:** a same-crate/different-file reify changeset pair (e.g. two
  `crates/reify-eval/src/*.rs` files) routes through the DEFAULT path detector →
  `changesets_overlap("reify", ...)` returns **False** (path members are distinct;
  the default "would miss it").
- **After restart:** ν's `register_for_reify()` call fires during orchestrator
  startup; the same pair routes through `CrateGraphOverlapDetector` → returns
  **True** (shared `crate:reify-eval` footprint member).

Observe this transition in the orchestrator logs or by inspecting the overlap
decision for a same-crate reify PR.

---

## Operator action required

The restart is performed **out-of-band, post-land**. It is NOT auto-fired by the
task agent (see rationale below).

**Step-by-step:**

1. Confirm the task has landed on `main`:
   ```bash
   git -C /home/leo/src/reify log --oneline | grep 4751
   ```

2. Confirm `project_root` is clean:
   ```bash
   git -C /home/leo/src/reify status --porcelain --untracked-files=no
   ```

3. Schedule the detached restart:
   ```bash
   bash /home/leo/src/reify/scripts/orchestrator-redeploy-restart.sh
   ```

4. After ~60 s, verify the service is running:
   ```bash
   systemctl --user status orchestrator-reify.service
   ```

5. Run the smoke:
   ```bash
   bash /home/leo/src/reify/tests/infra/test_reify_overlap_deploy_smoke.sh
   ```

**Why the task agent does not auto-fire the restart:**

An agent running under `orchestrator-reify.service` that schedules
`orchestrator-redeploy-restart.sh` would have the detached restart fire while
the merge-queue verify is still running (merge verifies take minutes; the
script's 60 s delay cannot outrun them). The restart stops the orchestrator
(cancels in-flight tasks), cancelling and requeuing this very task, which
reschedules the restart on re-run: a **self-cancel livelock**. The detector is
already on main (4750) and wired by ν, so a single out-of-band restart suffices.

---

## task_kind=deterministic migration note

Once `df 1898-1904` (deterministic-task-kind auto-deploy infra) lands and
deploys, this one-shot restart can be migrated to `task_kind=deterministic` so
the orchestrator auto-fires the restart post-merge without an operator action.
This runbook is the stopgap until that infrastructure is available.

Reference: dark_factory tasks 1898–1904 (deterministic-task-kind auto-deploy).

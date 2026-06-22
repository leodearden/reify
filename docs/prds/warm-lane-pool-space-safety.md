# PRD — Warm-lane pool space-safety & cold-fallback elimination

## 1. Goal — bound the warm-lane partition so it stops over-filling

The XFS CoW warm-build partition (`/dev/loop25` → `/home/leo/src/warm-lanes`) repeatedly fills to
100% and wedges the whole fleet (jun20 outage, jun21 recovery, jun22 outage — last landing froze
~9h with stewards escalating ENOSPC and the linker `SIGBUS`-ing on `mmap`-backed link output). The
warm-lane pool was supposed to keep usage bounded via re-seed-at-acquire (D10), but two regressions
plus a missing admission control defeat it:

1. **Re-seed-at-acquire is a silent no-op for every recycled lane** — `seed-warm-lane.sh`'s clobber
   guard refuses a non-empty `target/` and exits 1; the dark-factory consumer doesn't pre-remove and
   treats the failure as "degraded warmth, proceed" — so lanes never reset to a thin CoW clone and
   grow unboundedly (observed `_lane-4` at +112 GiB unique). This silently defeats the D10
   always-re-seed-at-acquire invariant.
2. **Cold-lane fallback** — when a warm lane can't be acquired, dispatch cold-creates a full,
   zero-CoW `<task_id>` worktree (~70–95 GiB each), bypassing the pool's space efficiency entirely.
3. **No admission control + no task-side reclaim** — nothing throttles dispatch or reclaims
   task-side space when the partition runs low; builds run straight into raw ENOSPC.

This PRD ships the code fixes that make the pool bounded by design. **Capacity provisioning** (image
size, host volume, physical loop creation, old-base reclaim) is an operator action handled out of
band — this PRD does not size or create the partition; it makes the software live within whatever
partition it's given and fail safe (requeue/escalate) instead of crashing when space is tight.

## 2. Background — what investigation established

A `/deb` root-cause session plus a 3-agent investigation (2026-06-22) established:

- **Single dispatch path.** `workflow.py:1372` → `git_ops.create_worktree`, which tries the pool
  internally and on `None` cold-creates at `git_ops.py:960`. There is **no** un-migrated legacy
  dispatch path; ζ #1788 is fully wired. So cold `<task_id>` worktrees are *live cold-fallback*, not
  partial-migration leftovers — exactly the path the operator's new "no cold fallback" policy targets.
- **The re-seed contract is broken end-to-end.** `seed-warm-lane.sh:281-285` clobber-refuses; the
  consumer (`git_ops.py:1429`, `:1520`) doesn't `rm` first and logs "proceeding with retained target
  — degraded warmth" (fired 10× in 11h). Both modes (`--fresh-checkout`, `--reset-in-place`) reach
  the clone block, and production always passes `--fresh-checkout`.
- **Re-seed alone is not sufficient.** Built lanes share almost nothing with the *current* base gen
  (they were seeded from older gens and rebuild), so a lane's effective footprint is ~a full target
  (~80 GiB), not a thin delta. At peak the live working set is `base + N×full-target`, so an
  admission control that requeues under disk pressure is mandatory, not optional.
- **Inodes are a co-equal ceiling.** Reflink shares extents, **not** inodes — every lane is a full
  inode set. The 1000 GiB image starved at 97% inodes (1.09M) before bytes alone would have. The
  disk guard and provisioning must both treat inodes as first-class.

## 3. Sketch of approach — fix the reset, forbid cold, add disk-pressure admission

Three coordinated changes, on the established **reify-ships-primitives / dark-factory-wires-consumers**
seam:

1. **Reset works (α + β).** `seed-warm-lane.sh --fresh-checkout` becomes *replace-existing*: it
   safely removes a stale non-empty `target/` (rename-to-trash → reflink-clone base → async `rm`)
   instead of refusing. The consumer (`acquire_warm_lane`) relies on that, and on a genuine seed
   fault **blocks & escalates** rather than running on a bloated stale target. This completes the
   D10 always-re-seed-at-acquire invariant the clobber-guard regression defeated.
2. **No cold fallback (β).** `create_worktree` no longer cold-creates when the pool returns no lane.
   `acquire_warm_lane` returns a **discriminated** outcome and dispatch routes: *pool-exhaustion →
   requeue/backpressure* (the scheduler already caps at pool size; it simply waits), *seed/worktree
   fault → escalate (blocked + L1)*, *disk-pressure → requeue (exit-75 transient)*. Zero `<task_id>`
   cold worktrees are ever created while the pool is enabled.
3. **Disk-pressure admission + reclaim (γ + δ + ε).** A reify guard primitive checks free **bytes
   and inodes**; a reify GC primitive reclaims task-side space (reset divergent FREE lanes to thin,
   reap orphaned-landed clean worktrees). The consumer wires them pre-acquire as *check → reclaim →
   if still low, requeue exit-75* — mirroring the existing merge-side guard (prune `_merge-*` →
   skip+escalate). Plus ζ makes provisioning inode-correct so a future image isn't inode-starved.

## 4. Resolved design decisions

- **D1 — `--fresh-checkout` is authoritative "replace existing".** Per the script's own header
  ("MUST always pass `--fresh-checkout` so a staled lane is rescued to warm"), fresh-checkout means
  fresh. It removes the stale target itself; the clobber *refusal* is retained only for misuse
  (target not under the warm-lane mount, or `LANE_DIR == base`). The RUSTFLAGS/invocation
  fail-closed guards still run first. Removal is rename-to-side-path + async `rm` for atomicity and
  recoverability (a crash leaves a recoverable `target.reseed-trash.$$`, never a half-seeded target).
- **D2 — no cold-lane fallback (operator policy).** When `warm_lane_pool` is enabled there is no
  full cold worktree, ever. Pool-exhaustion is normal scheduling backpressure (not an error, not an
  escalation); genuine provisioning faults escalate; disk-pressure requeues as transient.
- **D3 — seed fault blocks & escalates.** The "degraded warmth, proceed on retained target" path is
  deleted. A seed that genuinely cannot produce a warm target is a fault, surfaced to a human, not
  silently masked into bloat + staleness.
- **D4 — disk guard is bytes AND inodes.** Both are first-class thresholds; either below floor
  triggers reclaim-then-requeue.
- **D5 — reclaim before requeue.** Under disk pressure, try reclaiming task-side space first (cheap,
  preserves throughput); only requeue if reclaim doesn't free enough — the merge-guard pattern.
- **D6 — GC preserves WIP unconditionally.** GC never touches a lane/worktree with dirty tracked
  changes, unlanded ahead-of-`main` commits, or a live consumer (inv.2). Only clean+landed cold
  worktrees and divergent FREE lanes are reclaimable.

## 5. Pre-conditions for activating

- Active now. The orchestrator is currently disk-wedged; queued leaves sit `pending` until the
  out-of-band capacity action frees space and dispatch resumes. That is expected and does not block
  filing/wiring.
- No `.ri` substrate or grammar work is involved.

## 6. Substrate verification (G3) — no novel `.ri` substrate; host + code checks

Substrate is shell / dark-factory Python / XFS — the `.ri` grammar/semantic gate is **N/A**
(host-checks only, same as the parent `warm-lane-pool-cow-seeding.md` / `…-activation-seam.md`). All
assumed capabilities were verified live on 2026-06-22:

- Single dispatch path: `grep workflow.py:1372` → `create_worktree`; pool attempt `git_ops.py:789-809`;
  cold add `git_ops.py:960`.
- Clobber guard + clone block: `seed-warm-lane.sh:276-296` (refusal at `:281-285`).
- Requeue plumbing (transient/exit-75) exists: `harness.py:3285-3303`.
- Escalate plumbing (RuntimeError → blocked + L1) exists: `git_ops.py:907-908`, `harness.py:2797`.
- Merge-side disk-guard precedent to mirror: `merge_queue.py:552-611`, `config.py:1182`
  (`merge_verify_min_free_disk_bytes`); prune scope `verify.py:2716`.
- Reflink/XFS proven (the live pool runs on it; `warm-lane-preflight.sh` Check 2 probes it).

## 7. Cross-PRD / cross-repo relationship (G4)

Same seam as the warm-lane-pool series: **reify ships primitives, dark-factory wires consumers.**

| Leaf | Repo | Owns |
|---|---|---|
| α reset primitive | reify | `seed-warm-lane.sh` replace-existing |
| β failure-semantics | dark-factory | `acquire_warm_lane`/`create_worktree` rm-before-seed, no-cold-fallback, discriminated outcome, escalate-on-fault |
| γ disk-guard primitive | reify | `warm-lane-disk-guard.sh` (bytes + inodes) |
| δ GC primitive | reify | `warm-lane-gc.sh` (reclaim FREE-lane divergence + orphan cold worktrees) |
| ε admission wiring | dark-factory | pre-acquire check→reclaim→requeue |
| ζ provisioning | reify | `provision-warm-lane-fs.sh` inode-correct mkfs |
| η integration gate | dark-factory | two-way boundary test (B+H) |

No contested ownership; no reciprocal "the other owns it". Builds on the landed warm-lane-pool-cow
contract (D8/D10) — this PRD *completes* its always-re-seed invariant and adds the missing admission
control. Capacity/provisioning of the physical partition is an **operator action**, explicitly out
of scope here (§12).

## 8. Contract — seam signatures + invariants (B+H §)

### 8.1 Reset primitive — `seed-warm-lane.sh --fresh-checkout` (α)
- **Pre:** `LANE_DIR/target` may be non-empty; `LANE_DIR` is under the warm-lane mount and ≠ base.
  RUSTFLAGS/invocation guards passed.
- **Effect:** stale `target/` removed (rename `target.reseed-trash.$$` → background `rm`), then
  `cp -a --reflink=always <resolved base gen>/target → LANE_DIR/target`; exit 0.
- **Post:** `LANE_DIR/target` is a thin CoW clone of the current base; prior unique extents freed.
- **Refusal retained:** if `LANE_DIR` is not under the mount or equals base → exit 1 (misuse guard).

### 8.2 Failure-semantics — `acquire_warm_lane` / `create_worktree` (β)
- Returns a **discriminated** result: `OK(lane)` | `EXHAUSTED` | `FAULT(reason)` | `DISK_PRESSURE`.
- `create_worktree` with pool enabled: `OK` → use lane; `EXHAUSTED` → requeue/backpressure (no cold
  worktree); `FAULT` → raise (→ blocked + L1); `DISK_PRESSURE` → requeue exit-75.
- **inv.no-cold:** with `warm_lane_pool` enabled, `git worktree add … <worktree_base>/<task_id>` is
  never reached.

### 8.3 Disk guard — `warm-lane-disk-guard.sh` (γ)
- `check` → exit 0 if `free_bytes ≥ min_free_gib` **and** `free_inodes ≥ min_free_inodes`; else a
  backpressure exit code. PSI/df source overridable for testability. Knobs documented for
  `orchestrator.yaml warm_lane_pool` (`min_free_gib`, `min_free_inodes`).

### 8.4 GC — `warm-lane-gc.sh` (δ)
- `reclaim` resets divergent FREE lanes to thin (via the α primitive) and `git worktree remove`s
  orphaned worktrees that are clean **and** landed (`git merge-base --is-ancestor <branch> main`)
  **and** have no live consumer. **inv.preserve:** dirty WIP / unlanded-ahead / live-consumer is
  never touched.

## 9. Boundary-test sketch (two-way; B+H, closes G2 for η)

A dark-factory integration test (η) drives the pool end-to-end and asserts, in one run:
- recycled-lane acquire re-seeds **thin** (target shares base extents; prior divergence freed);
- pool-exhausted dispatch **requeues** and creates **zero** `<task_id>` cold worktrees;
- a forced seed/worktree fault **escalates** (blocked + L1), never proceeds on retained bloat;
- forced low-disk (guard backpressure) triggers **reclaim → if-still-low → requeue exit-75**, never
  an ENOSPC build.
Reify-side primitives carry their own `tests/infra` proofs (α/γ/δ/ζ); η is the cross-repo seam test.

## 10. Decomposition plan — task DAG with observable signals (G2)

- **α [reify]** — `seed-warm-lane.sh --fresh-checkout` replace-existing. *Signal:*
  `tests/infra/test_seed_warm_lane.sh` new case — re-seeding a **non-empty** lane target exits 0 and
  yields a thin CoW clone (extent-shared with base; old divergent extents freed); misuse refusal
  retained. *Deps:* none.
- **β [dark-factory]** — `acquire_warm_lane`/`create_worktree` failure-semantics: rm-before-seed,
  escalate-on-fault, **no cold fallback**, discriminated outcome; update `config.py` docstring +
  inv.6. *Signal:* DF test — recycled acquire re-seeds thin; pool-exhausted → requeue with **zero**
  `<task_id>` worktrees created; seed/add fault → blocked+L1. *Deps:* α.
- **γ [reify]** — `warm-lane-disk-guard.sh` (free bytes + inodes). *Signal:*
  `tests/infra/test_warm_lane_disk_guard.sh` — backpressure exit when bytes **or** inodes below
  threshold (overridable source), exit 0 when both healthy. *Deps:* none.
- **δ [reify]** — `warm-lane-gc.sh` reclaim. *Signal:* `tests/infra/test_warm_lane_gc.sh` — reclaims
  a divergent FREE lane (reset to thin) and an orphaned-landed clean worktree, while **preserving**
  dirty-WIP / unlanded-ahead / live-consumer lanes. *Deps:* α (reuses the reset primitive).
- **ε [dark-factory]** — wire γ + δ pre-acquire: check → reclaim → requeue exit-75 if still low.
  *Signal:* DF test — under simulated low-disk, dispatch reclaims then requeues, never enters an
  ENOSPC build. *Deps:* β, γ, δ.
- **ζ [reify]** — `provision-warm-lane-fs.sh` inode-correct mkfs (`-i maxpct=50`) + size knob.
  *Signal:* a `tests/infra` check provisions a small image with the new args and asserts
  `xfs_info`/`df -i` inode headroom scales (imaxpct=50; inodes-per-GiB ≫ the starved 1.09M/1000G).
  *Deps:* none.
- **η [dark-factory]** — integration gate (B+H two-way boundary test, §9). *Signal:* the end-to-end
  DF integration test green. *Deps:* α, β, γ, δ, ε, ζ.

DAG: α→β; α→δ; {β,γ,δ}→ε; {α,β,γ,δ,ε,ζ}→η.

## 11. Out of scope

- **Capacity / provisioning of the physical partition** — image size (the sizing analysis suggests
  ~3 TiB for N=24), host-volume choice (nvme vs HDD-backed data volume), physical loop creation,
  base migration, and reclaim of the old pre-relocation base. These are **operator actions** done
  out of band; ζ only makes the provisioning *script* inode-correct.
- Lowering `max_concurrent_tasks` (a capacity/throughput knob, operator's call).
- The merge-side disk guard and `_merge-*` prune (already exist; this PRD adds the task-side analogue).
- The in-engine warm-state compute-node eviction pool (`warm-state-eviction.md`) — a different pool.

## 12. Open questions (tactical — surfaced, not blocking)

- α trash-collection cadence: async `rm` per reseed vs a batched sweep (default: per-reseed background `rm`).
- δ GC trigger: invoked only by the ε disk-pressure path (default) vs also a low-frequency timer backstop.
- ε threshold defaults (`min_free_gib`, `min_free_inodes`) — set conservative defaults; tune post-deploy.
- Whether to also reset FREE lanes to thin at **release** (cheaper FREE-lane footprint) vs only at
  acquire (D10) — deferred; acquire-reset + GC reclaim covers it.

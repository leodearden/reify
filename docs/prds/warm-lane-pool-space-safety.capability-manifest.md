# Capability manifest — warm-lane pool space-safety & cold-fallback elimination

Mechanizes G3 + G6 for `docs/prds/warm-lane-pool-space-safety.md` (decompose 2026-06-22). One block
per **leaf**, binding each capability the task's user-observable signal asserts to evidence. Any FAIL
value blocks the batch. **Substrate is shell / dark-factory Python / XFS — the `.ri` grammar/semantic
gate is N/A** (host-checks only, same as the parent `warm-lane-pool-cow-seeding.md`). G6
numeric-domain (FEA) hazards are **N/A** — no closed-form/numeric-bound signals; the only numeric
assertions are configurable thresholds and a provisioned inode floor. All evidence re-verified
against live source on 2026-06-22.

Evidence vocabulary:
- `grep:<file>:<line> wired` — symbol/flag present on main at the named line.
- `host-cap:<facility>` — host facility (XFS reflink, util-linux flock, df/stat) — host-observable.
- `producer:<label> upstream` — capability delivered by an upstream task in the transitive dep closure.

---

## α — reify: `seed-warm-lane.sh --fresh-checkout` replace-existing

**Signal:** `tests/infra/test_seed_warm_lane.sh` new case — re-seeding a non-empty lane target under
`--fresh-checkout` exits 0 and yields a thin CoW clone (extent-shared with base; old divergent
extents freed); misuse refusal retained.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| The clobber guard to convert exists at a known site | `grep:scripts/seed-warm-lane.sh:281-285 wired` (refusal block) | PASS |
| The reflink clone the reset reuses exists | `grep:scripts/seed-warm-lane.sh:290-295 wired` (`cp -a --reflink=always`) | PASS |
| `--fresh-checkout` mode flag is parsed | `grep:scripts/seed-warm-lane.sh:89,299 wired` (`FRESH_CHECKOUT`) | PASS |
| XFS reflink available to produce a thin clone | `host-cap:xfs-reflink` (live pool runs on it; preflight Check 2 probes) | PASS |
| Test harness exists to extend | `grep:tests/infra/test_seed_warm_lane.sh wired` (H3a/H3b cases present) | PASS |

α produces its own signal (it writes the reset). No `producer-downstream`. No FAIL.

---

## β — dark-factory: failure-semantics (rm-before-seed, no cold fallback, discriminated outcome)

**Signal:** DF test — recycled acquire re-seeds thin; pool-exhausted → requeue with **zero**
`<task_id>` cold worktrees created; seed/worktree-add fault → blocked + L1.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Single dispatch path to change | `grep:workflow.py:1372 wired` → `create_worktree` | PASS |
| Pool-attempt + cold-fallthrough site to remove | `grep:git_ops.py:789-809 wired`; cold add `grep:git_ops.py:960 wired` | PASS |
| Recycled re-seed call sites (degraded-warmth path) to fix | `grep:git_ops.py:1429,1520 wired` | PASS |
| Requeue/backpressure plumbing (exhaustion, exit-75) exists | `grep:harness.py:3285-3303 wired` | PASS |
| Escalate plumbing (RuntimeError → blocked + L1) exists | `grep:git_ops.py:907-908 wired`; `harness.py:2797` | PASS |
| Old-policy docstring/inv to update | `grep:config.py:824-826 wired` ("falls back to the cold path — never blocks") | PASS |
| Replace-capable seed it relies on | `producer:α upstream` (reify α; cross-project dep) | PASS |

No FAIL. The only upstream-supplied capability (replace-capable seed) is α, wired as a dep.

---

## γ — reify: `warm-lane-disk-guard.sh` (free bytes + inodes)

**Signal:** `tests/infra/test_warm_lane_disk_guard.sh` — backpressure exit when bytes **or** inodes
below threshold (overridable source), exit 0 when both healthy.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Merge-side guard precedent to mirror (bytes-only) | `grep:merge_queue.py:552-611 wired`; `config.py:1182 wired` | PASS |
| Free-bytes + free-inodes are host-observable | `host-cap:df`/`host-cap:stat -f` (df reports both; `df -i`) | PASS |
| Preflight script to extend or pattern to copy | `grep:scripts/warm-lane-preflight.sh wired` (5 host checks; same fail-closed shape) | PASS |
| Backpressure exit-code convention (75/EX_TEMPFAIL) exists | `grep:harness.py:3285-3303 wired` (exit-75 transient class) | PASS |

γ produces its own signal. No FAIL.

---

## δ — reify: `warm-lane-gc.sh` reclaim

**Signal:** `tests/infra/test_warm_lane_gc.sh` — reclaims a divergent FREE lane (reset to thin) and an
orphaned-landed clean worktree, while **preserving** dirty-WIP / unlanded-ahead / live-consumer lanes.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Reset primitive to reuse for thinning a FREE lane | `producer:α upstream` (reify α; intra-reify dep) | PASS |
| Landed-check is computable | `host-cap:git` (`git merge-base --is-ancestor <branch> main`) | PASS |
| Dirty/ahead detection is computable | `host-cap:git` (`status --porcelain`, `rev-list main..HEAD`) | PASS |
| Existing GC precedent (base `.gen.*`) to mirror | `grep:scripts/refresh-warm-base.sh:353-385 wired` (reader-refcount GC) | PASS |
| Live-consumer detection (inv.2) | `host-cap:flock`/process-cwd scan (refresh-warm-base GC uses per-gen flock) | PASS |

No FAIL. Upstream capability (reset primitive) is α, wired as a dep.

---

## ε — dark-factory: admission wiring (check → reclaim → requeue)

**Signal:** DF test — under simulated low-disk, dispatch reclaims then requeues exit-75, never enters
an ENOSPC build.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Pre-acquire hook point exists | `grep:git_ops.py:789-809 wired` (the create_worktree pool region β also edits) | PASS |
| Disk guard primitive to call | `producer:γ upstream` (reify γ; cross-project dep) | PASS |
| Reclaim primitive to call | `producer:δ upstream` (reify δ; cross-project dep) | PASS |
| Discriminated outcome to extend with DISK_PRESSURE | `producer:β upstream` (DF β; intra-DF dep) | PASS |
| Merge-guard reclaim-then-skip pattern to mirror | `grep:merge_queue.py:584-611 wired` (prune → recheck → skip+escalate) | PASS |

No FAIL. All upstream capabilities (γ, δ, β) wired as deps.

---

## ζ — reify: `provision-warm-lane-fs.sh` inode-correct mkfs

**Signal:** a `tests/infra` check provisions a small image with the new args and asserts
`xfs_info`/`df -i` inode headroom scales (imaxpct=50; inodes-per-GiB ≫ the starved 1.09M/1000G).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| Provisioning script + mkfs.xfs call to parameterize | `grep:scripts/provision-warm-lane-fs.sh wired` (creates loop img + mkfs.xfs) | PASS |
| `mkfs.xfs -i maxpct=` raises inode cap | `host-cap:mkfs.xfs` (standard XFS option) | PASS |
| `df -i` / `xfs_info` report inode headroom | `host-cap:df`/`host-cap:xfs_info` | PASS |
| Inode floor is a real, achievable bound | starved baseline `1.09M / 1000 GiB`; target ≫ that at default density on a larger image — achievable, not a fabricated floor | PASS |

ζ produces its own signal. No FAIL.

---

## η — dark-factory: integration gate (two-way boundary test)

**Signal:** end-to-end DF integration test green (recycled→thin; exhausted→requeue, zero cold;
fault→escalate; low-disk→reclaim→requeue).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| All four behaviors are produced by upstream leaves | `producer:α,β,γ,δ,ε,ζ upstream` (full closure; all wired as deps) | PASS |
| Pool harness exists to drive end-to-end | `grep:orchestrator/warm_lane_pool.py wired`; `git_ops.acquire_warm_lane` | PASS |

η asserts only capabilities its dependency closure produces (it is the integration gate). No
`producer-downstream`. No FAIL.

---

**Result: 0 FAIL across 7 leaves.** Batch clears the manifest gate.

# Warm-lane pool audit runbook — `scripts/warm-lane-audit.sh`

**Task #5177 | 2026-07-12**

Operational digest for the standalone, timer-friendly audit/telemetry report over the warm-lane
CoW pool. Design authority is `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.1 (α) / §10 B1-B2;
this note documents the **landed script interface** (`scripts/warm-lane-audit.sh`, authoritative —
a superset of the PRD's §9.1 sketch), not the PRD sketch itself.

---

## Purpose

`warm-lane-audit.sh` reports, per resident worktree under the warm-lane mount, three **distinct**
questions — plus its backing-task status, whether its work is recoverable, its divergent disk
footprint, and a derived classification, and a trailing pool-wide HEADROOM summary. It exists so
accretion toward ENOSPC is **observable long before** the disk-guard's hard floor trips (which *is*
the wedge).

The three questions, and why they are three:

| Question | Column | Source of truth |
|---|---|---|
| Is a consumer **process running** against this lane? | `live` | non-blocking `flock -n -s` probe on `<dir>.lock` |
| Has the pool **reserved** this lane? | `assigned` | the orchestrator's own record at `<state-dir>/<lane>.json` |
| Is it reserved but **nothing is running**? | `pin` (+ classification `PINNED`) | the derived relation `assigned == ASSIGNED ∧ live == IDLE` |

Until task 5363 the script had **one** column, `assigned=ASSIGNED|FREE`, sourced entirely from the
flock probe. That is a category error: the probe measures *liveness*, and liveness was being
reported under the name *assignment*. It produced two opposite misreads from the same root cause:

- **2026-07-22** — 53 lanes the orchestrator had reserved reported `FREE`, because no consumer
  happened to hold their lock. The pool looked empty while it was exhausted.
- **2026-07-26 (esc-5556-1)** — of 33 reserved lanes only 3 backed a running task; the other 30 were
  held by tasks that were not running (27 `pending`, one `infra-hold`, oldest pin 2026-07-09). A
  standing 30/56 capacity loss the report had no column able to name.

A lane can be reserved with nothing running against it, and that state is neither "live" nor "free".
It is `PINNED`, and it is the pool's most common form of lost capacity.

**It is read-only and non-gating.** It never resets, removes, or reclaims a lane, and it never
blocks dispatch, reclaim, or merge (PRD §9.5 inv.12) — it only informs. The gating primitive is
`scripts/warm-lane-disk-guard.sh` (hard + soft floor); the reclaim primitive is
`scripts/warm-lane-gc.sh` / `scripts/thin-warm-lane.sh`. This script is purely diagnostic.

Consumers: the reify ζ end-to-end integration gate (asserts the ADVISORY relation
`measured_resident_divergent_gib ≤ resident_divergent_budget_gib`), the dark-factory θ soft-floor
dispatch throttle (indirectly, via the disk-guard), and — the primary audience for this runbook —
an operator or agent investigating pool disk pressure, on demand or via a timer (see "Run cadence"
below).

## CLI

```
scripts/warm-lane-audit.sh [--mount DIR] [--format table|json] [--status-cmd CMD]
                            [--stale-age-min N] [--main-ref REF] [--safety N]
```

| Flag | Env override | Default | Meaning |
|---|---|---|---|
| `--mount DIR` | `REIFY_WARM_LANE_MOUNT` | (unset) | Warm-lane worktrees dir (shared with `warm-lane-preflight.sh` / `warm-lane-gc.sh`). A nonexistent/empty mount reports `resident=0` — **not an error** (advisory-only). |
| `--format table\|json` | — | `table` | Output format. |
| `--status-cmd CMD` | `REIFY_LANE_LEAK_STATUS_CMD` | (unset → `unknown`) | Backing-task status oracle, invoked as `<cmd> <task_id>`; expected to print a status (`done`/`cancelled`/`pending`/…) to stdout. Non-zero exit or empty output = `unknown`. Same oracle `warm-lane-preflight.sh` Check 6 and `warm-lane-degenerate-ref-check.sh` consume (D6 — no new status-lookup plumbing; `warm-lane-gc.sh` dropped this consumer in task 5326). |
| `--stale-age-min N` | `REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN` | `60` | Minutes; a LEAKED candidate must have `age_min >= N`. |
| `--main-ref REF` | `REIFY_WARM_LANE_AUDIT_MAIN_REF` | `main` | Git ref treated as "main" for the LANDED recoverability check. |
| `--safety N` | `REIFY_WARM_LANE_AUDIT_SAFETY` | `1.5` | Dimensionless divisor (must be `> 0`) for `budget_gib = floor(free_gib / N)`; mirrors the illustrative safety factor in the sizing-lifecycle PRD §9.2 worked example. |
| `-h`, `--help` | — | — | Print usage and exit 0. |

Additional env-only knobs (no dedicated flag):

| Env var | Default | Meaning |
|---|---|---|
| `REIFY_WARM_LANE_AUDIT_DF` | `df` | `df` command override (mirrors `REIFY_WARM_LANE_DISK_GUARD_DF`; testability seam). |
| `REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB` | `data/queue/*.db*` | Glob (or comma-separated globs) of dirty tracked paths that count as harmless "residue" rather than unrecoverable WIP — today, the `write_queue.db*` fused-memory `DurableWriteQueue` runtime files (sizing-lifecycle §2/D1). |
| `REIFY_WARM_LANE_AUDIT_STATE_DIR` | `<mount>/.lane-state` | Directory holding the orchestrator's durable per-lane assignment records `<lane>.json` (dark-factory's `LANE_STATE_DIRNAME`, written by `orchestrator/src/orchestrator/lane_lifecycle.py`). May point anywhere, including outside the mount. Read-only; a missing dir is **not** an error — every lane simply reports `assigned=UNKNOWN` (A5). |

## Output

**Per-lane row** (table format: `key=value` pairs, one line per resident worktree; JSON format: one
object per lane under `"lanes"`):

| Field | Values | Notes |
|---|---|---|
| `lane` | e.g. `_lane-7` | Basename of the resident worktree dir. |
| `role` | `lane` \| `spec` \| `orphan` | From the basename: `_lane-*` → `lane`, `_spec-*` → `spec`, else `orphan` (mirrors `warm-lane-gc.sh`'s bucketing). |
| `live` | `LIVE` \| `IDLE` | **Liveness only.** Non-blocking `flock -n -s` probe on `<dir>.lock` (A1/A2 — see Invariants): is a consumer *process* holding the lane's exclusive flock right now? Not the assignment state. |
| `assigned` | `ASSIGNED` \| `RELEASED` \| `QUARANTINED` \| `UNKNOWN` | **Reservation only.** Read from `<state-dir>/<lane>.json`, never inferred from the lock. Mapping below. |
| `pin` | raw status, `unknown`, or `-` | For a lane that is `ASSIGNED` but not `LIVE`: the **raw** backing status of the task holding it (`pending` / `infra-hold` / `blocked` / `in-progress` / `done` / …). `unknown` when unresolvable (A3); `-` when the lane is not pinned. The holder is the record's `task_id` when present, falling back to the branch-derived `task/NNNN` id — the record is authoritative, since a lane's branch name can be stale. |
| `branch` | branch name or `(detached)` | Raw `git symbolic-ref --short HEAD`. |

Raw record `state` → reported `assigned` column (raw values are dark-factory's `LaneState` enum):

| Raw `state` | `assigned` | Meaning |
|---|---|---|
| `assigned`, `in_use` | `ASSIGNED` | Reserved for a task. |
| `released`, `seed`, `registered` | `RELEASED` | In the pool, not reserved. |
| `quarantined` | `QUARANTINED` | Withheld from the pool. |
| record missing / unreadable / corrupt / unrecognized value | `UNKNOWN` | Could not be resolved — never an error (A5). |
| `status` | `terminal` \| `non-terminal` \| `unknown` | Backing task's status via `--status-cmd` (A3 — see Invariants). |
| `recoverable` | `LANDED` \| `PUSHED` \| `ORPHAN` | `LANDED`: HEAD is an ancestor of `--main-ref`. Else `PUSHED`: HEAD is an ancestor of `refs/remotes/origin/<branch>`. Else `ORPHAN`. |
| `dirty` | `clean` \| `residue-only` \| `wip` | `git status --porcelain --untracked-files=no`; `residue-only` when every changed path matches `REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB`; a `git status` failure degrades fail-closed to `wip`. |
| `divergent_gib` | integer | `du -sB1 <lane>/target`, floored to GiB; `0` if `target/` is absent or `du` fails (degrades with a stderr warning, never aborts). |
| `age_min` | integer | Whole minutes since the worktree dir's mtime. |
| `classification` | `LIVE` \| `PINNED` \| `QUARANTINED` \| `RECLAIMABLE` \| `LEAKED` \| `PRESERVED-OK` | See Classification below. |

**Trailing HEADROOM line** (table format: one summary line after all per-lane rows; JSON format:
the `"headroom"` object):

```
HEADROOM resident=N live=L pinned=P quarantined=Q free=F assigned=A state_unknown=S reclaimable=R leaked=K leak_unknown=U divergent_gib=D free_gib=G budget_gib=B
PINNED   total=P pending=X infra-hold=Y blocked=Z terminal=T other=O unknown=V
```

The occupancy figures are an **ordered, mutually exclusive partition** of the resident set —
`live ≻ pinned ≻ quarantined ≻ free`, mirroring the classification rank. This identity is
**normative** and holds by construction:

```
resident = live + pinned + quarantined + free
```

`assigned` and `state_unknown` are **cross-cuts**, not partition members: they may overlap any
bucket and must never be added into the identity.

| Field | Meaning |
|---|---|
| `resident` | Count of resident git-worktree dirs under `--mount`. (The dot-prefixed `.lane-state` dir is not a resident.) |
| `live` | Partition: a consumer process holds the lane's exclusive flock. |
| `pinned` | Partition: `ASSIGNED` but not live — reserved by work that is not running. The pool's standing capacity loss. |
| `quarantined` | Partition: withheld from the pool (and not live). Classified `QUARANTINED`, never `LEAKED` — see Classification. |
| `free` | Partition **residue**: neither live, nor reserved, nor withheld. Nothing else. It was formerly `resident - live`, which counted every reserved-but-idle lane as available capacity — that is exactly the 2026-07-22 misread. |
| `assigned` | Cross-cut: lanes the pool has reserved, live or not (`live ∧ ASSIGNED` + `pinned`). `assigned ≫ live` is the esc-5556-1 signature. |
| `state_unknown` | Cross-cut: lanes whose assignment state could not be resolved (A5). Never counted `pinned`, `quarantined` or `assigned` — an *idle* one falls to `free` (the conservative reading), a *live* one is counted `live` like any other. Warned by name on stderr (the warning names the bucket it actually used), and counted here so "no pins" stays distinguishable from "pins could not be evaluated" — the same treatment A3 gives an unresolvable status. |
| `reclaimable` | Count classified `RECLAIMABLE`. |
| `leaked` | Count classified `LEAKED`. |
| `leak_unknown` | Count of FREE/stale/ORPHAN lanes whose LEAKED verdict could **not** be confirmed because the backing-task status is `unknown` (A3). |
| `divergent_gib` | Pool-wide divergent footprint, summed from raw bytes across all lanes and floored **once** at emission (sum-then-floor — never a sum of already-floored per-lane values, which would systematically undercount many small lanes). |
| `free_gib` | Free space on `--mount`, via the stubbable `df` seam. Degrades to `0` (with a stderr warning) on a `df` failure or unparseable output — never aborts. |
| `budget_gib` | `floor(free_gib / safety)` — a derived, recomputed quantity (P4/D8 — never a hardcoded lane-count or GB constant frozen into a test). |

**Trailing PINNED line** (table format: one line after HEADROOM; JSON format: a sibling
`"pinned_by_status"` object carrying the six buckets — their total is `headroom.pinned`, so it is
not repeated). `pinned=N` says how much capacity is standing idle; this says **why**, and therefore
what to do. The bucket vocabulary is **fixed and closed**, emitted in a fixed order, **always
emitted, zeros included** — a zero must be readable, never an absent line.

| Bucket | Backing status | What it means |
|---|---|---|
| `terminal` | `done`, `cancelled` | The holder finished. **Reclaim now.** |
| `pending` | `pending` | A reservation held by work that never started. |
| `blocked` | `blocked` | Ditto — held by work that cannot start. |
| `infra-hold` | `infra-hold` | Ditto — held pending infrastructure. |
| `other` | any status outside the buckets above | The residue, so it is **not** one reading: `in-progress` here means a **likely-crashed** consumer, while `deferred` / `review` are not-running states in the same family as `pending`. Read the per-lane `pin` column before acting. |
| `unknown` | unresolvable | The holder could not be resolved (A3). |

## Classification

Evaluated per lane, most-specific first:

The rank is the HEADROOM occupancy partition's order **exactly**: the first three verdicts are
one-for-one the partition's three occupied buckets, and the FREE ladder below them is one-for-one
its residue. A lane counted `quarantined=` on the summary line while its own row read `LEAKED` would
be the report contradicting itself.

1. **`LIVE`** — `live == LIVE`. A consumer process holds the lane; never touch it.
2. **`PINNED`** — idle, **and** `assigned == ASSIGNED`. The pool has reserved this lane but nothing
   is running against it. Ranked above the FREE ladder because a lane the pool still holds is
   unavailable capacity, not a candidate for reclaim — so **a pinned lane is never additionally
   reported `LEAKED`** and never counted in `leaked=`, even when it satisfies the LEAKED predicate
   exactly (non-terminal + `ORPHAN` + stale). A reservation the pool still holds is a scheduling
   problem, not a leak, and reporting it as one would invite reclaiming state a consumer may return
   to. Use the `pin` column (per lane) and the PINNED breakdown (pool-wide) to triage.
3. **`QUARANTINED`** — idle, unreserved, **and** `assigned == QUARANTINED`. The pool deliberately
   withheld this lane for inspection. Ranked above the FREE ladder for the same reclaim-suppressing
   reason as `PINNED`, and at least as strongly: **a quarantined lane is never reported `LEAKED`**
   and never counted in `leaked=`. Quarantine is a decision someone made about this lane; the audit
   reports it, and reclaim is that operator's call, not a verdict this script invites. Reclaim (and
   un-quarantine) belong to the lane lifecycle, never to this script.
4. **`RECLAIMABLE`** — idle, unreserved and not withheld, **and** (`status == terminal` **or**
   `recoverable ∈ {LANDED, PUSHED}` **or** `dirty == residue-only`). The work is either
   done/cancelled, already safely recorded elsewhere, or its only dirty content is harmless residue.
5. **`LEAKED`** — idle, unreserved and not withheld, **and** `status == non-terminal`, **and**
   `recoverable == ORPHAN` (by construction, always "ahead of main" in the PRD's sense), **and**
   `age_min >= stale_age_min` (A4 — always the declared-knob relation, never an inline literal). A
   stale, unrecoverable, still-active-looking lane nobody is coming back for.
6. **`PRESERVED-OK`** — everything else (the conservative default), including any idle lane whose
   LEAKED verdict is suppressed solely because its status is `unknown` (see `leak_unknown` above
   and Invariant A3) — reported `PRESERVED-OK`, not silently dropped, and separately counted so
   "no leaks" stays distinguishable from "leaks could not be evaluated".

## Invariants

- **A1 — read-only.** Never mutates a lane (no reset/rm/reclaim). This binds **both** on-disk
  surfaces the audit reads, identically: the LIVE/IDLE probe opens an *existing* `<dir>.lock`
  read-only and never creates a missing one; the assignment-state read opens an *existing*
  `<state-dir>/<lane>.json` read-only and never creates the record **or** the directory — neither a
  default `<mount>/.lane-state` nor an explicitly-pointed-at `REIFY_WARM_LANE_AUDIT_STATE_DIR`. No
  `>`-open, `touch`, or `mkdir` occurs anywhere on either path.
- **A2 — non-blocking, non-contending probe.** The `flock -n -s` (shared) probe releases
  immediately. It still correctly detects `LIVE` against a live consumer's exclusive lock, but
  never contends with another concurrent reader (e.g. a second audit run).
- **A3 — fail-safe status lookup.** A status-lookup failure degrades that lane to `unknown` — never
  aborts the report, never reclassifies as `RECLAIMABLE`/`LEAKED`. See `leak_unknown` above.
- **A4 — no frozen staleness literal.** `stale` is always the relation `age_min >= stale_age_min`
  against the declared knob (default 60 min), never an inline/undeclared literal (D8/G6).
- **A5 — fail-safe assignment-state read.** A missing state dir, a missing/unreadable record,
  corrupt JSON, or an unrecognized `state` value all degrade that lane to `assigned=UNKNOWN`. It
  never aborts and never invents an assignment. An UNKNOWN lane is never counted `pinned`,
  `quarantined` or `assigned`; an idle one falls to `free` (the conservative reading), a live one is
  counted `live`. Such lanes are surfaced separately — a stderr warning naming the lane, **which of
  the three causes fired**, and **which bucket it was actually counted in**, plus the HEADROOM
  `state_unknown` field — so "no pins" stays distinguishable from "pins could not be evaluated",
  exactly as A3 treats an unresolvable backing-task status. A state dir that is absent *entirely*
  warns **once for the directory** instead of once per lane, so a per-lane line keeps meaning "this
  lane, unlike its neighbours" rather than "the pool has no records yet". The state, its cause, and
  the record's `task_id` come from a **single read** of the record, so the reported triple always
  describes one instant of one record — the orchestrator rewrites these on every acquire/release.
  See "Reading a PINNED-heavy pool" for the cause vocabulary and what each one means.

## Reading a PINNED-heavy pool

`pinned ≫ live` is **not an audit bug**. It is a standing capacity loss: the pool is holding
reservations for work that is not running, and every pinned lane is a lane dispatch cannot use.
Read `assigned` against `live` first — if reservations far outnumber running consumers, the
bottleneck is scheduling, not disk.

The two incidents, as worked examples:

- **2026-07-22 — the pool looked empty while it was exhausted.** 53 lanes with no live consumer
  reported `FREE` while the orchestrator had them reserved. Under the current accounting that pool
  reports `free=0` and `pinned=53`: the reservations are visible, and `free` no longer absorbs them.
  The tell is `free=0` alongside a large `pinned` — *not* a small `resident`.
- **2026-07-26 (esc-5556-1) — a 30/56 standing loss with no column to name it.** Of 33 reserved
  lanes only 3 backed a running task; the other 30 were pinned by non-running tasks (27 `pending`,
  one `infra-hold`, oldest pin 2026-07-09). Today that reads as `assigned=33 live=3 pinned=30` with
  `PINNED total=30 pending=27 infra-hold=1 …`.

What to do per bucket:

| Bucket | Reading | Action |
|---|---|---|
| `terminal` | The holder is `done`/`cancelled` and still holds a reservation. | **Reclaim now** — this is recoverable capacity, released by the lane lifecycle, not by this script. |
| `pending`, `blocked`, `infra-hold` | A reservation held by work that is not running. | Not a leak. Look at why the backing tasks are not dispatching (blocked deps, infra hold, scheduler admission) — reclaiming the lane does not fix it. |
| `other` | Any status outside the buckets above — the bucket is a residue, not a diagnosis. **Check the per-lane `pin` value first.** | `in-progress`: the task is running but nothing holds the lane's lock — a **likely-crashed consumer**; investigate the agent/process before reclaiming. Anything else (`deferred`, `review`, …): read it as `pending` above — work that is not running, so not a leak and not a crash. |
| `unknown` | The holder could not be resolved (A3). | Check the `--status-cmd` oracle; the count is unverified, not zero. |

A high `state_unknown` means the *assignment* read is failing, so `pinned` itself is an undercount —
resolve that before trusting any of the occupancy figures. Three distinct causes produce it, and the
stderr warning names which one fired per lane (`lane=… assignment state unknown (<cause>) at <path>`)
rather than assuming the file is absent:

| Cause | Reading | Action |
|---|---|---|
| `no-readable-record` | No state dir, or no readable `<state-dir>/<lane>.json`. | The only filesystem/permissions case. Check `--mount` / `REIFY_WARM_LANE_AUDIT_STATE_DIR` resolves to the dir the orchestrator actually writes, and that it is readable. |
| `unparseable-record` | The record is present and readable but no `state` string could be read out of it. | A corrupt, truncated, or reshaped write — inspect the named file; it *is* there. |
| `unrecognized-state:<raw>` | The record parsed and named a state this script does not map. | **Schema drift.** A mass spike carrying one repeated `<raw>` value means dark-factory's `LaneState` gained a member; extend the mapping table above. Nothing is wrong with the pool. |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | **Always**, on every valid invocation. Advisory/observability only — this script must never gate anything (PRD §9.5 inv.12). A status-lookup failure, a `df`/`du` measurement failure, or a nonexistent/empty mount all degrade gracefully rather than aborting. |
| `2` | Usage error: unknown flag, missing flag value, or an invalid `--format` / `--stale-age-min` / `--safety`. |

## Recommended run cadence (§13 Q3)

Resolved as a documentation recommendation only (task 5177, PRD §13 Q3 "Decide during α/ι") — **no
timer unit is wired by this task**; a follow-up may implement one.

- **On-demand (primary use).** The script is read-only and safe to run at any time — reach for it
  first when investigating pool disk pressure or before/after a reclaim.
- **Optional periodic collection.** A systemd timer running the audit and capturing its HEADROOM
  line would give trend history instead of only point-in-time snapshots. The sizing-lifecycle PRD
  names `reify-warm-base-health.service` as the pattern to mirror; note that unit is itself a
  boot-time `Type=oneshot` (`WantedBy=default.target`, no `OnCalendar`/`OnUnitActiveSec`) rather
  than a recurring timer. The closer **structural** template already in the repo for an actually
  periodic sweep is `deploy/systemd/reify-warm-lane-gc.timer` (`OnBootSec=5min`,
  `OnUnitActiveSec=15min`, `Persistent=true`) pairing a `.timer` with a oneshot `.service`
  (`reify-warm-lane-gc.service`, `StandardOutput=journal`) — a future audit timer would likely
  follow that same two-unit shape.
- **Tee into the GC sweep log for trend history.** Today `reify-warm-lane-gc.service` has no
  dedicated log file — its output goes to the systemd journal (`journalctl -u
  reify-warm-lane-gc.service`). Emitting the audit's HEADROOM line alongside the GC sweep's own
  `reclaim: reset=N removed=M preserved=K preserved_live_ref=L` summary (either into the same
  journal unit or a shared log file) is the suggested mechanism for building headroom trend history
  over time; unimplemented as of this task.

  `preserved_live_ref=L` (task 5572) is **the share of K** held back by a live process reference — a
  process with its cwd, an open fd, or an mmap at or under the lane dir. It is a breakdown of
  `preserved`, **not** an additional bucket: L ≤ K, and K + L double-counts. Worth watching, because
  it is the only preserve reason that can shield an entry indefinitely — the dirty and unlanded
  reasons clear when the work lands, so a persistently non-zero L is the signal to go look for a
  stuck process holding a lane. New fields are APPENDED to this line, never interposed, so consumers
  matching the `reset=`/`removed=`/`preserved=` prefix keep working (`scripts/warm-lane-gc.sh`
  stdout contract).

## Pointers

| Topic | Source |
|---|---|
| Full design (α pillar, invariants, boundary tests B1/B2) | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.1, §10 |
| Sizing/budget formula consuming this script's `free_gib`/`budget_gib` | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.2 |
| The landed script (authoritative CLI/behavior) | `scripts/warm-lane-audit.sh` |
| Hard/soft-floor admission gating (the script that actually blocks dispatch) | `scripts/warm-lane-disk-guard.sh` |
| Reclaim primitives this script's classification informs | `scripts/warm-lane-gc.sh`, `scripts/thin-warm-lane.sh` |
| Pool lifecycle & invariants (acquire/reset/release) | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5 |

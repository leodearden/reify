# Warm-lane pool audit runbook — `scripts/warm-lane-audit.sh`

**Task #5177 | 2026-07-12**

Operational digest for the standalone, timer-friendly audit/telemetry report over the warm-lane
CoW pool. Design authority is `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.1 (α) / §10 B1-B2;
this note documents the **landed script interface** (`scripts/warm-lane-audit.sh`, authoritative —
a superset of the PRD's §9.1 sketch), not the PRD sketch itself.

---

## Purpose

`warm-lane-audit.sh` reports, per resident worktree under the warm-lane mount, whether it is
ASSIGNED or FREE, its backing-task status, whether its work is recoverable, its divergent disk
footprint, and a derived classification — plus a trailing pool-wide HEADROOM summary. It exists so
accretion toward ENOSPC is **observable long before** the disk-guard's hard floor trips (which *is*
the wedge).

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
| `--status-cmd CMD` | `REIFY_LANE_LEAK_STATUS_CMD` | (unset → `unknown`) | Backing-task status oracle, invoked as `<cmd> <task_id>`; expected to print a status (`done`/`cancelled`/`pending`/…) to stdout. Non-zero exit or empty output = `unknown`. Same oracle `warm-lane-preflight.sh` Check 6 and `warm-lane-gc.sh` consume (D6 — no new status-lookup plumbing). |
| `--stale-age-min N` | `REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN` | `60` | Minutes; a LEAKED candidate must have `age_min >= N`. |
| `--main-ref REF` | `REIFY_WARM_LANE_AUDIT_MAIN_REF` | `main` | Git ref treated as "main" for the LANDED recoverability check. |
| `--safety N` | `REIFY_WARM_LANE_AUDIT_SAFETY` | `1.5` | Dimensionless divisor (must be `> 0`) for `budget_gib = floor(free_gib / N)`; mirrors the illustrative safety factor in the sizing-lifecycle PRD §9.2 worked example. |
| `-h`, `--help` | — | — | Print usage and exit 0. |

Additional env-only knobs (no dedicated flag):

| Env var | Default | Meaning |
|---|---|---|
| `REIFY_WARM_LANE_AUDIT_DF` | `df` | `df` command override (mirrors `REIFY_WARM_LANE_DISK_GUARD_DF`; testability seam). |
| `REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB` | `data/queue/*.db*` | Glob (or comma-separated globs) of dirty tracked paths that count as harmless "residue" rather than unrecoverable WIP — today, the `write_queue.db*` fused-memory `DurableWriteQueue` runtime files (sizing-lifecycle §2/D1). |

## Output

**Per-lane row** (table format: `key=value` pairs, one line per resident worktree; JSON format: one
object per lane under `"lanes"`):

| Field | Values | Notes |
|---|---|---|
| `lane` | e.g. `_lane-7` | Basename of the resident worktree dir. |
| `role` | `lane` \| `spec` \| `orphan` | From the basename: `_lane-*` → `lane`, `_spec-*` → `spec`, else `orphan` (mirrors `warm-lane-gc.sh`'s bucketing). |
| `assigned` | `ASSIGNED` \| `FREE` | Non-blocking `flock -n -s` probe on `<dir>.lock` (A1/A2 — see Invariants). |
| `branch` | branch name or `(detached)` | Raw `git symbolic-ref --short HEAD`. |
| `status` | `terminal` \| `non-terminal` \| `unknown` | Backing task's status via `--status-cmd` (A3 — see Invariants). |
| `recoverable` | `LANDED` \| `PUSHED` \| `ORPHAN` | `LANDED`: HEAD is an ancestor of `--main-ref`. Else `PUSHED`: HEAD is an ancestor of `refs/remotes/origin/<branch>`. Else `ORPHAN`. |
| `dirty` | `clean` \| `residue-only` \| `wip` | `git status --porcelain --untracked-files=no`; `residue-only` when every changed path matches `REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB`; a `git status` failure degrades fail-closed to `wip`. |
| `divergent_gib` | integer | `du -sB1 <lane>/target`, floored to GiB; `0` if `target/` is absent or `du` fails (degrades with a stderr warning, never aborts). |
| `age_min` | integer | Whole minutes since the worktree dir's mtime. |
| `classification` | `LIVE` \| `RECLAIMABLE` \| `LEAKED` \| `PRESERVED-OK` | See Classification below. |

**Trailing HEADROOM line** (table format: one summary line after all per-lane rows; JSON format:
the `"headroom"` object):

```
HEADROOM resident=N assigned=A free=F reclaimable=R leaked=L leak_unknown=U divergent_gib=D free_gib=G budget_gib=B
```

| Field | Meaning |
|---|---|
| `resident` | Count of resident git-worktree dirs under `--mount`. |
| `assigned` | Count with `assigned=ASSIGNED`. |
| `free` | Count with `assigned=FREE` (`resident - assigned`). |
| `reclaimable` | Count classified `RECLAIMABLE`. |
| `leaked` | Count classified `LEAKED`. |
| `leak_unknown` | Count of FREE/stale/ORPHAN lanes whose LEAKED verdict could **not** be confirmed because the backing-task status is `unknown` (A3). |
| `divergent_gib` | Pool-wide divergent footprint, summed from raw bytes across all lanes and floored **once** at emission (sum-then-floor — never a sum of already-floored per-lane values, which would systematically undercount many small lanes). |
| `free_gib` | Free space on `--mount`, via the stubbable `df` seam. Degrades to `0` (with a stderr warning) on a `df` failure or unparseable output — never aborts. |
| `budget_gib` | `floor(free_gib / safety)` — a derived, recomputed quantity (P4/D8 — never a hardcoded lane-count or GB constant frozen into a test). |

## Classification

Evaluated per lane, most-specific first:

1. **`LIVE`** — `assigned == ASSIGNED`. A live consumer holds the lane; never touch it.
2. **`RECLAIMABLE`** — FREE, **and** (`status == terminal` **or** `recoverable ∈ {LANDED, PUSHED}`
   **or** `dirty == residue-only`). The work is either done/cancelled, already safely recorded
   elsewhere, or its only dirty content is harmless residue.
3. **`LEAKED`** — FREE, **and** `status == non-terminal`, **and** `recoverable == ORPHAN` (by
   construction, always "ahead of main" in the PRD's sense), **and** `age_min >= stale_age_min`
   (A4 — always the declared-knob relation, never an inline literal). A stale, unrecoverable,
   still-active-looking lane nobody is coming back for.
4. **`PRESERVED-OK`** — everything else (the conservative default), including any FREE lane whose
   LEAKED verdict is suppressed solely because its status is `unknown` (see `leak_unknown` above
   and Invariant A3) — reported `PRESERVED-OK`, not silently dropped, and separately counted so
   "no leaks" stays distinguishable from "leaks could not be evaluated".

## Invariants

- **A1 — read-only.** Never mutates a lane (no reset/rm/reclaim). The ASSIGNED/FREE probe opens an
  *existing* `<dir>.lock` read-only and never creates a missing one.
- **A2 — non-blocking, non-contending probe.** The `flock -n -s` (shared) probe releases
  immediately. It still correctly detects `ASSIGNED` against a live consumer's exclusive lock, but
  never contends with another concurrent reader (e.g. a second audit run).
- **A3 — fail-safe status lookup.** A status-lookup failure degrades that lane to `unknown` — never
  aborts the report, never reclassifies as `RECLAIMABLE`/`LEAKED`. See `leak_unknown` above.
- **A4 — no frozen staleness literal.** `stale` is always the relation `age_min >= stale_age_min`
  against the declared knob (default 60 min), never an inline/undeclared literal (D8/G6).

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
  `reclaim: reset=N removed=M preserved=K` summary (either into the same journal unit or a shared
  log file) is the suggested mechanism for building headroom trend history over time; unimplemented
  as of this task.

## Pointers

| Topic | Source |
|---|---|
| Full design (α pillar, invariants, boundary tests B1/B2) | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.1, §10 |
| Sizing/budget formula consuming this script's `free_gib`/`budget_gib` | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.2 |
| The landed script (authoritative CLI/behavior) | `scripts/warm-lane-audit.sh` |
| Hard/soft-floor admission gating (the script that actually blocks dispatch) | `scripts/warm-lane-disk-guard.sh` |
| Reclaim primitives this script's classification informs | `scripts/warm-lane-gc.sh`, `scripts/thin-warm-lane.sh` |
| Pool lifecycle & invariants (acquire/reset/release) | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5 |

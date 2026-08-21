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

A **fourth**, orthogonal question was added by task 5876 (esc-5866-8) — it is about *ref
integrity*, not occupancy, so it is neither a fourth bucket nor a classification verdict:

| Question | Column | Source of truth |
|---|---|---|
| Does the lane's ref still hold the work its **plan** says it committed? | `plan_sync` | `<lane>/.task/plan.json` step SHAs vs the lane's `HEAD` |
| Is the lane on the **branch its plan names**? | `plan_task` | the plan's own `task_id` vs the branch-derived `task/NNNN` id |

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
| `REIFY_WARM_LANE_AUDIT_STASH_REPO` | (unset → the **first resident lane** that resolves as a git worktree) | Repo to query for the shared stash stack (see "Trailing STASH block"). `refs/stash` lives in the shared common git dir, so **one** query answers for the whole pool and any resident lane will do. Deliberately **no** fallback to this script's own repo root — that would make the audit's own hermetic tests read the real, live shared stack. Read-only; unresolvable (no resident lane is a git worktree, or the pointed-at path is not one) or a failed query degrades to `stash_entries=0` plus a stderr note, exit still 0. |

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
| `plan_sync` | `OK` \| `REWRITTEN` \| `STRANDED` \| `UNKNOWN` \| `-` | Ref integrity against the **anchor**: the plan-order-**last** `done` entry carrying a `commit`, scanned over `prerequisites` then `steps`. Resolved against **`HEAD`** (not `refs/heads/task/N`) — `HEAD` is what a clobbered symbolic ref resolves *through*, and it keeps the column meaningful on a detached lane. See the verdict table below. |
| `plan_task` | `MATCH` \| `MISMATCH` \| `-` | Does the plan's top-level `task_id` equal the branch-derived `task/NNNN` id? `-` when there is no comparison to make: detached HEAD, a non-`task/` branch, a non-numeric id, or a plan carrying no `task_id`. A comparison with no left-hand side is **not** a mismatch. |

### `plan_sync` verdicts

The anchor is one entry, not every entry: steps execute in order and each commits atop the
previous, so if the deepest `done` commit is reachable, every earlier one is too. Two git calls per
lane, which matters for a pool walked by a timer.

| Verdict | Meaning | What to do |
|---|---|---|
| `-` | Nothing recorded yet: no `.task/plan.json` (including the usual *dangling* symlink into `.task-meta/`), or no `done` entry carrying a commit. The common, uninteresting case — most residents. | Nothing. |
| `OK` | The anchor **is** an ancestor of `HEAD`. | Nothing. |
| `REWRITTEN` | The anchor is **not** an ancestor, but an **equivalent patch** (same patch id) **is** in `HEAD`'s history. A routine rebase — requeue, base refresh — which rewrites every recorded sha while preserving every patch. | Nothing. **Expected to dominate a healthy pool.** |
| `STRANDED` | The anchor is not an ancestor **and no equivalent patch exists anywhere in `HEAD`'s history**. The recorded work is genuinely not on this branch. | **Investigate, then escalate.** Never auto-repair — see "Reading a STRANDED lane". |
| `UNKNOWN` | Could not be evaluated. Causes, reported verbatim: `no-readable-record`, `unparseable-record`, `anchor-object-absent:<sha>`, `equivalence-undecidable:<sha>` (a root-commit anchor has no parent to diff against; a merge-commit anchor is skipped by the patch-id comparison). | Read the cause. A **mass spike** carrying one repeated shape is its own signal — e.g. `anchor-object-absent` across many lanes suggests an over-aggressive pool-wide `git gc`. |

A non-ancestor anchor is **not, by itself, evidence of lost work** — the workflow rebases
routinely, so non-ancestry is the *steady state* of a healthy pool. That is why the verdict is
split, and why only `STRANDED` warns per lane while `REWRITTEN` is counted and otherwise silent.

**Residual false positive, recorded honestly:** a rebase whose conflict was resolved with a
*different* diff changes the patch id, and will read `STRANDED`. This is the reason `STRANDED` is
an investigate-then-escalate signal and never an auto-repair trigger.

**Trailing HEADROOM line** (table format: one summary line after all per-lane rows; JSON format:
the `"headroom"` object):

```
HEADROOM resident=N live=L pinned=P quarantined=Q free=F assigned=A state_unknown=S reclaimable=R leaked=K leak_unknown=U divergent_gib=D free_gib=G budget_gib=B plan_stranded=X plan_unknown=Y plan_rewritten=Z plan_mismatch=M stash_entries=E
PINNED   total=P pending=X infra-hold=Y blocked=Z terminal=T other=O unknown=V
STASH    total=E
STASH    entry ref=… branch=… message=…          (one line per entry)
```

The occupancy figures are an **ordered, mutually exclusive partition** of the resident set —
`live ≻ pinned ≻ quarantined ≻ free`, mirroring the classification rank. This identity is
**normative** and holds by construction:

```
resident = live + pinned + quarantined + free
```

`assigned` and `state_unknown` are **cross-cuts**, not partition members: they may overlap any
bucket and must never be added into the identity. So are the four `plan_*` fields and
`stash_entries` — the last of which is not a count of *lanes* at all (see below), and so can exceed
`resident`.

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
| `plan_stranded` | **Cross-cut**: lanes whose `plan_sync` is `STRANDED`. May overlap any bucket and must **never** be added into `resident = live + pinned + quarantined + free`. Each is warned by name on stderr. |
| `plan_unknown` | **Cross-cut**: lanes whose `plan_sync` could not be evaluated (A6). Same prohibition. Warned by name, with the cause. |
| `plan_rewritten` | **Cross-cut**: lanes whose anchor was rewritten by a rebase. Same prohibition. **Counter-only — never warned**: it is the expected steady state, and a per-lane line for each would bury the stranded lane under the pool's own background noise. Emitted so `plan_stranded` is readable *in proportion to it* rather than in a vacuum. |
| `plan_mismatch` | **Cross-cut**: lanes whose `plan_task` is `MISMATCH`. Same prohibition. |
| `stash_entries` | **Cross-cut, and the only HOST-scoped figure on this line**: entries on the shared `refs/stash` stack, not a count of lanes. Same prohibition — never added into the identity, and it may exceed `resident`. See "Trailing STASH block". |

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

**Trailing STASH block** (table format: a `STASH total=E` count line after PINNED, then one
`STASH entry ref=… branch=… message=…` detail line per entry — message last, so its spaces are
unambiguous; JSON format: `headroom.stash_entries` plus a sibling `"stash_stack"` array of
`{ref, branch, message}` objects). Like PINNED, **always emitted, zeros included** — an empty stack
must read as a value an operator can see, never as an absent line. The count lives in exactly one
place: the array carries no total of its own.

`branch` is parsed from the reflog subject git writes (`On <branch>: <msg>` for `stash push -m`,
`WIP on <branch>: <sha> <subject>` for a bare push). A subject in neither shape reports `branch=-`
with the raw subject as `message` — never guessed, never dropped.

**How to read it.** `refs/stash` is **ONE ref in the shared `.git`**, not per-worktree (git treats
only `HEAD`, `refs/worktree/*`, `refs/bisect/*` and `refs/rewritten/*` as per-worktree), so:

- The figure is **HOST-SCOPED**, not per-lane. One query answers for the whole pool, and every lane
  would report the same number. It is not attributable to the lane it was read through.
- It **does not self-drain**. Entries accrete until an operator drains them — which is exactly the
  shape the original incident took (esc-5785-6: nine entries over ~1 month before the 2026-08-02/03
  drain).
- It is deliberately **NON-GATING** (A1, PRD §9.5 inv.12). A non-zero value never blocks dispatch,
  reclaim or merge, and never requeues a task: the condition is host-scoped, so bouncing N tasks
  one at a time would spin the whole fleet on a warning nobody reads.

A **non-zero value means agents are still stashing in shared checkouts**, which is usually one of
two things: the never-stash rule (CLAUDE.md → "Warm lanes") is not reaching them, or the
`hooks/reference-transaction` guard has been **disarmed** by the `core.hooksPath` clobber that
Claude Code's worktree feature performs on every worktree enter (CLAUDE.md → "Landing on main").
That second case is why this field exists at all: it is the **backstop that makes a silently
disarmed guard visible**. Triage in that order — check the guard is live before concluding the rule
is being ignored.

For the guard's measured reach — including that it covers the **push direction only** — read
`hooks/reference-transaction`'s header and `tests/infra/test_stash_guard.sh`, which are
authoritative. Do not re-derive it here.

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

`plan_sync` and `plan_task` are deliberately **not** classification verdicts. Ref integrity is
orthogonal to occupancy: a stranded lane can be `LIVE`, `PINNED` or `RECLAIMABLE` at the same time.
Folding `STRANDED` into the ranked verdict would either hide it beneath `LIVE` or corrupt the
`resident = live + pinned + quarantined + free` partition this document calls normative.

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

- **A6 — fail-safe, read-only plan read.** A missing, unreadable or corrupt `<lane>/.task/plan.json`
  (including the **dangling** absolute symlink into `<worktree_base>/.task-meta/<lane>/` that every
  lane carries until its architect writes the plan), an anchor commit absent from the object DB, or
  an anchor whose patch equivalence cannot be decided, all degrade that lane to
  `plan_sync=UNKNOWN`. It never aborts, and — exactly as A1 binds the other two surfaces — never
  creates the `.task/` dir or the record: no `>`-open, `touch` or `mkdir` anywhere on that path, and
  every git call is a pure reader (`cat-file -e`, `merge-base --is-ancestor`, `cherry`). It
  **never accuses a strand it cannot evidence**: an absent anchor object is `UNKNOWN`, never
  `STRANDED`, and a non-ancestor anchor whose patch survives in `HEAD` is `REWRITTEN`, never
  `STRANDED`. "Nothing recorded yet" stays the distinct `-` sentinel, so a fresh lane's all-pending
  plan never inflates the unknown count. Verdict, cause, anchor and `task_id` all come from a
  **single read**, so one row never describes two instants of a record the architect and the
  implementer both rewrite mid-run. The script **never repairs** what it finds (A1; PRD §9.5
  inv.12).

## "A plan.json done-step commit SHA is dangling / unreachable" — EXPECTED, not a defect

**If you arrived here because a `plan.json` recorded `done` step's commit SHA does not resolve,
or is not an ancestor of the lane's `HEAD` — stop before escalating.** That observation, by
itself, is the steady state of a healthy pool, not evidence that recorded work is missing.

**Why.** A warm-lane reclaim, or a requeue / inter-iteration rebase, replays the task branch
onto a newer `main`. Every replayed commit gets a fresh SHA; `plan.json` was written against the
pre-replay ones and is never rewritten to match. The recorded SHA going dangling — or failing an
ancestor check — is the mechanism **working as designed**, not work being lost: `acquire_lane`
always re-seeds from the base (CLAUDE.md → "Warm lanes").

**The discriminator is patch-id, never reachability.** `git cherry HEAD <anchor> <anchor>^`: a
leading `-` means the patch **is** present in `HEAD`'s history under a new SHA (benign —
`REWRITTEN`); a leading `+` means the patch is genuinely absent (`STRANDED`).
`scripts/warm-lane-audit.sh` already implements exactly this discriminator — see its `plan_sync`
column, the "`plan_sync` verdicts" table above, and invariant A6. Dark-factory's equivalent path
is `_reconcile_done_step_commits` → `find_equivalent_commit`, which files only `severity='info'`.
This section does not restate either's verdict semantics — read those directly.

A **subject-only** match (comparing commit messages instead of patch content) is *not* a
substitute for patch-id: on one measured branch, subject-matching scored 0/18 where patch-id
scored 18/18.

**Evidence, dated and attributed — not a threshold, not an expected value, and not a number to
diff a fresh run against:**

- **Measured 2026-08-19 on the live reify pool:** of 71 tasks holding 530 unique SHAs still on
  disk, 52 of 71 had at least one dangling SHA and 43 of 71 were all-dangling (~60% dangle rate).
  Of 27 non-`done` tasks holding 205 dangling SHAs, 204 were patch-id-present on their own
  branch; the single miss was an **empty commit** (`git patch-id` emits nothing for a zero-diff
  commit) whose replayed twin existed with an identical subject. **Zero** real strands.
  `scripts/warm-lane-audit.sh` reported `plan_stranded=0 plan_rewritten=28 plan_unknown=1
  plan_mismatch=0`.
- **Corpus-wide (dark-factory task 4032 §4, D6/INV-5; corpus measured by dark-factory task
  3157's 2026-08-05 addendum):** 991 of 1,973 recorded done-step SHAs (50.2%) no longer exist as
  git objects; a bare `merge-base --is-ancestor` fires on 185 of 200 live task branches (92.5%);
  and of those 1,973 done steps, **zero** were confirmed "recorded done, work nowhere" — four
  candidates, all hand-falsified.

  Dark-factory task 4032's own ruling, quoted verbatim: **"SHA UNRESOLVABLE IS AN EXPECTED
  STATE, NEVER A DEFECT SIGNAL"** and **"DO NOT write a second, parallel reachability
  mechanism"**. (Its status is `pending` as of this writing — cite it as a ruling/decision
  record, not as landed code.)

**The `reify-audit` correction** — the specific false belief that keeps getting re-escalated:
`crates/reify-audit/src/` has **zero** code references to `plan.json`, `.task-meta`, or
`steps[].commit` (verified 2026-08-19). An escalation claiming "the reify audit sweep would read
this as phantom-done" is describing a detector **that does not exist**.

**When to escalate anyway.** This section rules out one specific false alarm; it does not mean
every `plan_sync` reading is safe to ignore. A non-zero `plan_stranded` (the patch is genuinely
absent) or a `plan_task=MISMATCH` remains an investigate-then-escalate signal — work the triage
order in "Reading a STRANDED lane" immediately below.

**Re-escalation history.** Independently rediscovered and re-escalated at least three times:
esc-5344-3; esc-5866-2 through esc-5866-7 (six auto-filed at once); and esc-5937-5/-6/-7, which
sat at L2 from 2026-08-09 to 2026-08-19 — esc-5937-5 was filed **ten days after** reify task 5876
had already shipped the detector that answers it.

## Reading a STRANDED lane

`STRANDED` means: the plan records a `done` step at commit `<sha>`, that object still exists, it is
**not** an ancestor of the lane's `HEAD`, and **no commit in `HEAD`'s history carries an equivalent
patch**. The recorded work is not on the branch under any sha.

**Worked example — esc-5866-8 / task 5876.** `refs/heads/task/5866` kept its *name* while its
*tip* was clobbered to `task/5632`'s. Every commit that lane's plan recorded as done became
unreachable from `HEAD` — dangling but still present in the object DB. No pre-existing column saw
it: `live`/`assigned`/`pin` answer occupancy questions, and `recoverable` asks only whether `HEAD`
is reachable from main, which a clobbered tip satisfies exactly as well as a healthy one.

**Do NOT auto-repair the ref.** Recovery requires knowing *which dangling commits belong to which
task*, and a wrong reflog-based repoint destroys the very evidence a root-cause depends on. Ref
repair belongs to the lane lifecycle and to a human (A1; PRD §9.5 inv.12). The audit reports and
stops, by design.

Triage order:

1. Read the warning — it names the lane, the anchor entry id, and the anchor commit.
2. Check `plan_task` on the same row. `MISMATCH` points at a lane→branch **binding** failure (the
   lane is on the wrong branch); `MATCH` with `plan_sync=STRANDED` is the observed **tip-clobber**
   shape. They are separate signatures and route differently.
3. Confirm the anchor is really unreachable and really patch-absent before concluding anything:
   `git -C <lane> cat-file -e <sha>^{commit}`, `git -C <lane> merge-base --is-ancestor <sha> HEAD`,
   `git -C <lane> cherry HEAD <sha> <sha>^` (a leading `+` = patch absent; a leading `-` = present
   and the verdict would have been `REWRITTEN`).
4. Rule out the residual false positive above (a conflict resolved with a different diff).
5. Escalate with the raw observations. Do not move refs.

### Relationship to `scripts/warm-lane-degenerate-ref-check.sh` (task 5006)

Complementary, not redundant. That classifier's discriminant is
`rev-list --count main..task/N == 0` **and** the tip does not cite `N` — it detects a ref parked on
a **main ancestor**. A ref clobbered to a *different task's live tip* has count > 0 and classifies
`live` there, so the existing classifier does **not** cover this shape. Conversely `plan_sync` does
subsume the degenerate class whenever `plan.json` records a `done` step, since such a ref fails the
ancestor test and carries no equivalent patch.

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

### Cross-pool strandedness sweep

Safe to run at any time — read-only, never gates, exit 0 regardless of what it finds.

```bash
# Pool-wide counters (the four cross-cuts):
scripts/warm-lane-audit.sh --mount "$REIFY_WARM_LANE_MOUNT" --format json \
  | python3 -c 'import json,sys; h=json.load(sys.stdin)["headroom"]; print({k:h[k] for k in ("plan_stranded","plan_unknown","plan_rewritten","plan_mismatch")})'

# The per-lane rows behind them:
scripts/warm-lane-audit.sh --mount "$REIFY_WARM_LANE_MOUNT" \
  | grep -E 'plan_sync=(STRANDED|UNKNOWN)|plan_task=MISMATCH'
```

Expect `plan_rewritten` to dominate — that is a healthy pool, not a finding. A non-zero
`plan_stranded` is an **investigate-then-escalate** signal, never an auto-repair trigger; work the
triage order in "Reading a STRANDED lane" above. No measured counts are recorded here on purpose: a
point-in-time number frozen into a runbook is the frozen-constant antipattern D8/G6 reject — the
recipe belongs in tracked docs, its output does not.

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
  it is the only *reclaim-eligible* preserve reason that can shield an entry indefinitely — the
  dirty and unlanded reasons clear when the work lands, so a persistently non-zero L is the signal
  to go look for a stuck process holding a lane. (K also lumps in protect-glob skips, which shield
  an entry for as long as the glob is passed — permanent under the default `--protect-glob`, but
  deliberate and operator-selected, so they never enter reclaim at all. Read a persistently high K
  with a zero L as that, not as a leak.) New fields are APPENDED to this line, never interposed, so
  consumers matching the `reset=`/`removed=`/`preserved=` prefix keep working
  (`scripts/warm-lane-gc.sh` stdout contract).

  `stash_entries=E` is worth trending in the same place, and is the HEADROOM field most improved
  by history: it is host-scoped and does not self-drain, so a single snapshot cannot
  distinguish "one entry, pushed a minute ago" from "one entry that has sat there for a month".
  **Slow accretion is precisely the shape the original incident took** — nine entries over ~1 month
  (esc-5785-6) — so a series that only ever climbs is the signal, and any decline is an operator
  drain rather than the pool healing itself. Trend it; do not gate on it.

## Pointers

| Topic | Source |
|---|---|
| Full design (α pillar, invariants, boundary tests B1/B2) | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.1, §10 |
| Sizing/budget formula consuming this script's `free_gib`/`budget_gib` | `docs/prds/warm-lane-pool-sizing-lifecycle.md` §9.2 |
| The landed script (authoritative CLI/behavior) | `scripts/warm-lane-audit.sh` |
| Hard/soft-floor admission gating (the script that actually blocks dispatch) | `scripts/warm-lane-disk-guard.sh` |
| Reclaim primitives this script's classification informs | `scripts/warm-lane-gc.sh`, `scripts/thin-warm-lane.sh` |
| Pool lifecycle & invariants (acquire/reset/release) | `docs/prds/warm-lane-pool-cow-seeding.md` §9.3/§9.5 |
| The shared-stash guard `stash_entries` backstops (measured reach, push-direction-only) | `hooks/reference-transaction` header, `tests/infra/test_stash_guard.sh` |

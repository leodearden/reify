# Closure-staleness sweep runbook — `scripts/deterministic-gate-closure-staleness-sweep.sh`

**Task #5321 | 2026-07-26**

Operational digest for the recurring, read-only sweep that detects tasks stranded in `blocked` /
`in-progress` after their block premise has already resolved, and emits a machine-readable
re-dispatch request for each confirmed hit.

The **normative source is the script itself** — its `-h` usage block and its header `# Invariants:`
list (L1–L6). This note is a digest that points at them; where the two disagree, the script wins.

---

## Purpose

One sweep covering all three stranding premises, rather than a mechanism per premise.

Task #5316 shipped `docs/notes/offline-lane-red-corruption-remediation.md` — a **documentation
runbook, with no script and no timer**. Its own "Purpose and scope" says so, and its
Cross-references row for #5321 records the standing sweep as *"Not yet delivered"*. This script is
therefore the family's **first executable artifact**: #5316's detection procedure is a human
re-run trigger, and §5's "close the loop — do not leave it `blocked`" step was, until now,
performed by hand.

The sweep is **advisory**. It never gates anything, exits 0 on every valid invocation, and
degrades a row it cannot adjudicate to `unknown` rather than aborting.

## CLI

Run `scripts/deterministic-gate-closure-staleness-sweep.sh -h` for the flag table — it is not
restated here. Every flag has an env counterpart, and an explicit flag always overrides its env
knob:

| Flag | Env knob |
|---|---|
| `--db PATH` | `REIFY_LANE_TASK_DB` |
| `--tag TAG` | `REIFY_LANE_TASK_TAG` |
| `--escalations DIR` | `REIFY_GATE_STALENESS_ESCALATIONS_DIR` |
| `--repo DIR` | `REIFY_GATE_STALENESS_REPO` |
| `--main-ref REF` | `REIFY_GATE_STALENESS_MAIN_REF` |
| `--stale-heartbeat-min N` | `REIFY_GATE_STALENESS_HEARTBEAT_MIN` |
| `--emit-requests DIR` | `REIFY_GATE_STALENESS_REQUESTS_DIR` |

`REIFY_LANE_TASK_DB` / `REIFY_LANE_TASK_TAG` are **reused verbatim** from
`scripts/lane-task-status.sh`'s contract — one task-DB plumbing contract in the repo, not a
parallel one. Task ids are unique only within a tag (`tasks` is `PRIMARY KEY (tag, id)`), so every
query the sweep issues is tag-scoped.

## Trigger classes

| Class | Scope | Premise-resolved predicate | Action |
|---|---|---|---|
| `gate_closure` | `blocked` only | `metadata.task_kind = deterministic` ∧ `metadata.always_escalates` truthy ∧ **every live `esc-<id>-*.json` is terminal (`resolved`/`dismissed`), or there are none** — see the terminal allowlist under Verdict vocabulary | `close` |
| `merge_verify_red` | `blocked` **and** `in-progress` | newest `metadata.dry_run_proposals` entry is a post-merge-verify red ∧ its `main_sha` is an ancestor of `--main-ref` ∧ main has advanced past it ∧ ≥1 `files_referenced` path was touched in `main_sha..main-ref` | `reverify` |
| `unmet_dependency` | `blocked` only | ≥1 `dependencies` row ∧ **every** `depends_on` resolves, under the same tag, to a terminal status (`done` / `cancelled`) | `redispatch` |

Notes that are easy to get wrong:

- **`gate_closure` emits `close`, not `redispatch`.** Per #5316 §5 the correct closure for a
  satisfied deterministic gate is a transition to `cancelled`, not a re-run.
- **`gate_closure` reads the LIVE escalation dir only** — never `archive*/`. An archived
  escalation is by construction no longer gating, and the live-dir absence *is* the signal. The
  glob matches `*.json` and never the `*.json.lock` sidecars the store also holds.
- **`merge_verify_red` keys primarily off the `block_reason` prose prefix**
  (`^Post-merge verification failed`), with `block_class == "merge_verify_red"` as a *confirming
  hint*: `block_class` is present on only 2 of the 55 live `dry_run_proposals` entries, so keying
  on it alone would miss almost every real case.
- **Recency is keyed on `investigated_at` then `timestamp`, not on array position.** The store
  appends without reordering and a re-investigation can rewrite an earlier entry in place, so
  `proposals[-1]` is not reliably the newest.
- **Reachability is checked BEFORE any diff** (#5316 §3) — see Corruption suppressors.
- **Class precedence** is the fixed order `gate_closure > merge_verify_red > unmet_dependency`.
  The first match is the row's primary class and the only one adjudicated; any further match is
  disclosed in `evidence` as `also:<class>` without incrementing a counter (invariant L3). The
  order is load-bearing, not alphabetical: letting C win over A would re-run a gate task that
  should simply be closed.
- **An empty dependency set is not "all satisfied"** — a `blocked` task with zero `dependencies`
  rows is not class C at all.

## Verdict vocabulary

| Verdict | Meaning | Emits a request? |
|---|---|---|
| `STALE` | premise resolved; a confirmed hit | **yes — the only verdict that does** |
| `UNRESOLVED` | the class matched, its premise has not resolved | no |
| `GATED` | class A only: a live `status=pending` escalation still gates it | no |
| `LIVE` | the liveness guard fired; no class predicate ran at all | no |
| `CORRUPT-HOLD` | an otherwise-confirmed hit carrying a #5316 corruption flag | no — `action=human_gate` |
| `NO-CLASS` | no trigger class matched the row at all | no |
| `unknown` | a class matched but its **oracle** could not be read; never upgraded to `STALE` | no |

Class A's escalation-status predicate is a **terminal allowlist**, not a pending-only match:
`pending` → `GATED`, `resolved`/`dismissed` → clear, and **every other value** — a status outside
the store's vocabulary (a schema addition on the escalation side), a JSON `null`, an empty or
absent key, an unparseable file — is a failed oracle read that degrades the task to `unknown` with
a `[warn]` naming the task and the offending status. A pending-only match would sink all of those
into "clear", manufacturing `STALE` / `close` / a CANCEL request for a task whose gate may still be
live — inverting L2 toward the sweep's single most destructive action.

The allowlist only defends rows the oracle actually **reaches**, so the escalations dir is checked
for `-r`/`-x`, not just `-d`. A dir that exists but cannot be enumerated (mode 000, a root-owned
dir swept under a different uid, a stale mount) passes `-d` — `stat` needs `+x` on the *parent*, not
on the dir — but then silently yields an empty glob, which would read as "zero live escalations" and
land in "clear" without the loop body ever running. Such a dir degrades exactly like a missing one.

`NO-CLASS` and `unknown` are **different answers and are counted separately.** `NO-CLASS` is a
*complete* adjudication with a negative result — nothing failed, the row simply matches no trigger
class. `unknown` means a class **matched** and its oracle then could not be read (a missing
escalations dir, an unresolvable SHA, a `depends_on` that resolves to no row under the tag). On the
live store the great majority of `blocked` / `in-progress` rows match no class at all, so folding
them into `unknown` would swamp precisely the signal that counter exists to carry.

The trailing `SWEEP:` line (table) / `summary` object (json) carries `candidates`, `gate_closure`,
`merge_verify_red`, `unmet_dependency`, `corrupt_hold`, `live_skipped`, `no_class`, `unknown`.
`live_skipped` and `unknown` exist so **"no hits" stays distinguishable from "could not tell"** —
the same reason `warm-lane-audit.sh` reports `leak_unknown`.

## Corruption suppressors

#5316's two catalogued signatures are wired in as **flags that demote a hit**, not as a fourth
class with its own auto-action. The catalog itself lives in
`docs/notes/offline-lane-red-corruption-remediation.md` and is deliberately not restated here —
one source of truth for a destructive remediation.

| Flag | Signature | Check |
|---|---|---|
| `corrupt_autofile` | Sig 1 — help-text-as-failing-tests | any `metadata.failing_tests` entry containing a marker from `_CORRUPT_AUTOFILE_MARKERS` (the single source of truth for the marker set, in the script) |
| `misattributed_provenance` | Sig 2 | `metadata.done_provenance.commit` resolves but is **not an ancestor** of `--main-ref`. The ancestry probe runs against the **pre-resolved** `--main-ref` SHA, and an unresolvable `--main-ref` yields **no provenance flag at all** — a `[warn]` and a degrade. `merge-base --is-ancestor` exits non-zero both for genuine non-ancestry and for a second argument that does not resolve, so handing it a raw ref name would let a *missing* oracle read as positive evidence of corruption |
| `provenance_unresolvable` | Sig 2 | `metadata.done_provenance.commit` does not resolve in `--repo` at all — held conservatively, not cleared. A `--repo`-only check: unaffected by `--main-ref`, and still flagged when there is no ancestry oracle |

⚠️ **Reachability, never diff inspection.** #5316 records that `git show --stat` alone
**mis-cleared #5264**: a discarded duplicate merge shows a perfectly plausible diff and fails only
the ancestor test. The sweep therefore runs `git rev-parse --verify` and then
`git merge-base --is-ancestor` — and nothing else clears a recorded provenance SHA.

A flag on a row whose verdict would otherwise be `STALE` rewrites it to `CORRUPT-HOLD` /
`human_gate`, counts it in `corrupt_hold` **instead of** its class counter, and suppresses its
request (invariant L5). Rationale: a corrupt record has an untrustworthy block premise, so
auto-re-dispatching it would act on a false premise, and #5316 §4 establishes that remediation
there is a mandatory human git-history adjudication a detector "could flag but not perform".

Flags are still computed and reported on **non-stale** rows, so #5316's audit coverage is not lost
for records that are corrupt but not yet stranded.

## Invariants

Keyed to the script's header `# Invariants:` block, which is authoritative:

- **L1** — the liveness guard is the first predicate for every candidate and short-circuits: a
  fresh heartbeat is never a hit and never yields a request, and an **unparseable** heartbeat
  degrades to `LIVE`, never to eligible.
- **L2** — an unreadable escalation oracle degrades to `unknown`, never to `STALE`; a row that
  simply matches no class is `NO-CLASS`, which is a different thing and a different counter.
- **L3** — every candidate contributes to exactly one class counter and appears exactly once;
  `--class all` never double-counts a multi-class row.
- **L4** — classes A and C are `blocked`-only; only class B spans `in-progress`.
- **L5** — a corruption flag suppresses auto-re-dispatch; a flagged hit is held for a human gate.
- **L6** — read-only on all task state: sqlite is opened `-readonly` / `mode=ro`, the escalation
  store is only read, and the only side effect of any invocation is request files under
  `--emit-requests`.

## Exit codes

- **0** — always, on every valid invocation (advisory-only; degrade, never abort).
- **2** — usage error only: unknown flag, missing flag value, invalid `--format` / `--class`, or a
  non-integer / negative `--stale-heartbeat-min`.

## `--emit-requests` consumer contract

One file per confirmed hit, `redispatch-<task_id>-<class>.json`, holding `schema_version`,
`task_id`, `class`, `verdict`, `action`, `evidence`, `main_ref_sha`, `emitted_by`.

- **Atomic** — a `mktemp` intermediate in the same directory followed by `mv` (a rename within one
  filesystem), removed on every failure path. A consumer polling the directory never observes a
  partial file.
- **Idempotent** — the body carries no wall-clock field, deliberately, so re-emission is
  byte-identical and a consumer can diff the directory instead of re-processing it. Read the file
  mtime if recency is needed.
- **Never gating** — an uncreatable or unwritable directory warns on stderr; the report on stdout
  is still complete and the exit code is still 0. The directory is created on demand only when its
  parent already exists, so a typo'd `--emit-requests` surfaces as a warning rather than silently
  materializing a path and reporting "0 requests emitted".

**The sweep does not perform the task-state write, by design.** `CLAUDE.md` is categorical that
all task operations go through the fused-memory MCP tools; writing `tasks.db` directly would
bypass the reconciliation that status transitions trigger, turning an advisory sweep into an
unaudited mutator of the canonical task store. This is the house cross-repo seam verbatim: **reify
ships the primitive, dark-factory wires the invocation** that performs the `set_task_status` /
`update_task` write.

## Recommended run cadence

Extends #5316's "Re-run trigger" section. Run the sweep:

- on the standing-audit cadence;
- after any escalation-watcher resolution/dismissal sweep (that is precisely what strands a class-A
  gate task);
- after any merge-gate red on `main` — and again once `main` advances past it (class B);
- after any task reaches a terminal status, which may satisfy a dependent's last dependency
  (class C).

**The timer itself is not wired by this task.** It belongs in dark-factory:
`dark-factory-orchestrator.yaml` loads once at startup and a task running under the orchestrator
must not restart it, so wiring the recurring invocation from inside a reify task is not possible.
A follow-up is filed for it; until it lands, the sweep is a manual/ad-hoc run.

## First sweep findings (2026-07-26)

The live measurements this design was derived from — recorded as the evidentiary basis for the
predicates, not as live assertions (the suite is hermetic; see below):

- **Class A: 5537 / 5549 / 5559** — `always_escalates=true`, still `blocked`, with **zero** live
  `esc-<id>-*.json`; their escalations are archived `dismissed` (~08:10Z). Three live instances of
  #5316 §5's un-closed-gate gap. These are other tasks' records, so a follow-up is filed rather
  than acting on them here.
- **Class C shape: 5372** — `blocked`, one dependency on 5271, which is `done`.
- **A claimant-less stale `in-progress`: 5196** — NULL `claimant_run_id`, `updated_at` ~3h stale.
- **The measured false-positive hazard:** all **ten** live `in-progress` tasks had every dependency
  `done` — **task 5321 itself among them**. A naive "dependency premise resolved ⇒ re-dispatch"
  rule would have targeted ten actively-running agents: the capability would have *destroyed* work
  rather than recovered it. That measurement is why the liveness guard is L1 and why class C is
  `blocked`-only.

## Human-gate seed batch status

The four instances named in this task's brief were **already redispatched or closed** as of
2026-07-26: 5236 `pending`, 5271 `done`, 5316 `done`, 5373 `pending`. They therefore serve as the
suite's **frozen fixture shapes**, not as live detections — a live-DB assertion on them would be
both non-hermetic and already false. The whole suite is hermetic by construction: a synthetic
`tasks.db` built from the production DDL, a synthetic escalation dir, and a synthetic git repo per
block; every SHA in an assertion is computed from the fixture repo, never frozen.

## Pointers

| Topic | Source |
|---|---|
| Corruption signature catalog + remediation recipe (#5316) | `docs/notes/offline-lane-red-corruption-remediation.md` |
| Read-only task-DB access contract (`REIFY_LANE_TASK_*`) | `scripts/lane-task-status.sh` |
| The advisory-observability script family this clones | `scripts/warm-lane-audit.sh`, `docs/notes/warm-lane-audit-runbook.md` |
| Hermetic test suite | `tests/infra/test_deterministic_gate_closure_staleness_sweep.sh` |
| Test-bucket registration (`pool`) | `tests/infra/run-all-classification.manifest` |
| Verify-pipeline artifact↔test mapping | `scripts/verify-pipeline-infra-tests.txt` |

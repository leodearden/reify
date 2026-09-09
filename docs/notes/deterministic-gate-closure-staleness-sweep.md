# Closure-staleness sweep runbook — `scripts/deterministic-gate-closure-staleness-sweep.sh`

**Task #5321 | 2026-07-26** · *two trigger classes retired by #7349, 2026-09-09*

Operational digest for the recurring, read-only sweep that detects tasks stranded in `blocked` /
`in-progress` after their block premise has already resolved, and emits a machine-readable
re-dispatch request for each confirmed hit.

The **normative source is the script itself** — its `-h` usage block and its header `# Invariants:`
list (L1–L6). This note is a digest that points at them; where the two disagree, the script wins.

---

## Purpose

One sweep covering the stranding premises reify owns, rather than a mechanism per premise. It
shipped with three trigger classes; #7349 retired two of them — see **Retired classes** below.

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
| `--repo DIR` | `REIFY_GATE_STALENESS_REPO` |
| `--main-ref REF` | `REIFY_GATE_STALENESS_MAIN_REF` |
| `--stale-heartbeat-min N` | `REIFY_GATE_STALENESS_HEARTBEAT_MIN` |
| `--emit-requests DIR` | `REIFY_GATE_STALENESS_REQUESTS_DIR` |

`REIFY_LANE_TASK_DB` / `REIFY_LANE_TASK_TAG` are **reused verbatim** from
`scripts/lane-task-status.sh`'s contract — one task-DB plumbing contract in the repo, not a
parallel one. Task ids are unique only within a tag (`tasks` is `PRIMARY KEY (tag, id)`), so every
query the sweep issues is tag-scoped.

One knob is **env-only, with no flag**: `REIFY_GATE_STALENESS_SQLITE_BIN` overrides the sqlite3-CLI
probe, and an explicitly *empty* value forces the python3 engine. It exists because the probe
resolves `/usr/bin/sqlite3` by absolute path, so stripping `PATH` cannot reach it — which makes it
both the test seam for the fallback and the break-glass for an incompatible CLI.

## Engines and cost

The sweep issues **one query per run**, not one per candidate per oracle. Everything the
classifier needs — two `metadata` scalars (`dry_run_proposals`, `done_provenance.commit`) and the
Signature-1 marker count — is selected alongside the candidate row, so a candidate costs zero
further DB opens. Measured on a 50-candidate fixture at the time the shape landed: **301 sqlite3
processes before, 1 after** (the earlier shape re-opened the store and re-ran
`WHERE tag=… AND id=… LIMIT 1` against the *same* metadata blob once per field, plus a `json_each`
query and a dependency join). #7349 has since removed two more selected columns and the
**dependency roll-up**, which was the query's only **join** — the sweep no longer reads the
`dependencies` table at all.

Three properties of that query are load-bearing:

- `json_extract` / `json_each` **raise** on malformed JSON, and in a whole-table query one raised
  error kills every row rather than one. Metadata is read through a `json_valid` guard that
  substitutes `{}`, so an unreadable blob yields empty fields — the same outcome the per-row
  version got from a swallowed error.
- a scalar `json_extract` of a JSON string returns the **decoded** text, which can contain a
  newline (and in principle the US field separator). Every free-form column is flattened, so no
  value can forge a field or row boundary in the US-separated stream.
- every clause is tag-scoped. `tasks` is `PRIMARY KEY (tag, id)`, so an unqualified lookup would
  silently conflate tags the moment a second one exists.

The sqlite3 CLI and the python3 stdlib engine run that **identical SQL string**, so they are
genuinely interchangeable rather than one being a stub that enumerates rows nothing can then
adjudicate. If neither is present — or if python3 alone is missing, which disables the proposal
parser and `--format json` — the sweep says so once, at startup, instead of silently reporting a
whole run as `unknown`.

## Trigger classes

One class remains. Two were retired by #7349 — see **Retired classes**.

| Class | Scope | Premise-resolved predicate | Action |
|---|---|---|---|
| `merge_verify_red` | `blocked` **and** `in-progress` | newest `metadata.dry_run_proposals` entry is a post-merge-verify red ∧ its `main_sha` is an ancestor of `--main-ref` ∧ main has advanced past it ∧ ≥1 `files_referenced` path was touched in `main_sha..main-ref` | `reverify` |

The `blocked` **and** `in-progress` scope is why the enumeration filters
`status IN ('blocked','in-progress')`; that filter looks over-broad now that the two `blocked`-only
classes are gone, and must **not** be narrowed.

Notes that are easy to get wrong:

- **`merge_verify_red` keys primarily off the `block_reason` prose prefix**
  (`^Post-merge verification failed`), with `block_class == "merge_verify_red"` as a *confirming
  hint*: `block_class` is present on only 2 of the 55 live `dry_run_proposals` entries, so keying
  on it alone would miss almost every real case.
- **Recency is keyed on `investigated_at` then `timestamp`, not on array position.** The store
  appends without reordering and a re-investigation can rewrite an earlier entry in place, so
  `proposals[-1]` is not reliably the newest.
- **Reachability is checked BEFORE any diff** (#5316 §3) — see Corruption suppressors.
- **There is no class precedence any more.** With one class there is nothing to order and no
  `also:<class>` disclosure to make; what invariant L3 still asserts is the counting property — at
  most one class counter per candidate, exactly one report row per candidate.

## Verdict vocabulary

| Verdict | Meaning | Emits a request? |
|---|---|---|
| `STALE` | premise resolved; a confirmed hit | **yes — the only verdict that does** |
| `UNRESOLVED` | the class matched, its premise has not resolved | no |
| `LIVE` | the liveness guard fired; no class predicate ran at all | no |
| `CORRUPT-HOLD` | an otherwise-confirmed hit carrying a #5316 corruption flag | no — `action=human_gate` |
| `NO-CLASS` | no trigger class matched the row at all | no |
| `unknown` | a class matched but its **oracle** could not be read; never upgraded to `STALE` | no |

`NO-CLASS` and `unknown` are **different answers and are counted separately.** `NO-CLASS` is a
*complete* adjudication with a negative result — nothing failed, the row simply matches no trigger
class. `unknown` means a class **matched** and its oracle then could not be read (a recorded
`main_sha` that does not resolve in `--repo` or is not an ancestor of `--main-ref`, a `--main-ref`
that does not resolve at all, a proposal recording no `files_referenced` or no `main_sha`). On the
live store the great majority of `blocked` / `in-progress` rows match no class at all, so folding
them into `unknown` would swamp precisely the signal that counter exists to carry.

The trailing `SWEEP:` line (table) / `summary` object (json) carries `candidates`,
`merge_verify_red`, `corrupt_hold`, `live_skipped`, `no_class`, `unknown`.
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
  degrades to `LIVE`, never to eligible. The guard does not key on the heartbeat alone — with
  **no** heartbeat at all it consults `claimant_run_id`, scoped to `in-progress`: such a row that
  holds a claimant is a claimed runner which has not yet written (or has lost) its heartbeat, and
  is `LIVE`. A `blocked` row with no heartbeat stays eligible whatever its claimant, because
  `blocked` rows legitimately carry no heartbeat and would otherwise become invisible wholesale.
  When the claimant rule landed the
  claimant-without-heartbeat shape was measured at **zero** occurrences on the live store, so it
  is fail-safe hardening against a shape the sweep must survive, not a change to observed
  behaviour.
- **L2** — an unreadable oracle degrades to `unknown`, never to `STALE`; a row that simply matches
  no class is `NO-CLASS`, which is a different thing and a different counter.
- **L3** — every candidate contributes to **at most one** class counter and appears exactly once.
  With a single class this is a counting property, not a precedence rule.
- **L4** — `merge_verify_red` spans `blocked` **and** `in-progress`, which is why the enumeration's
  `status IN ('blocked','in-progress')` filter is correct as written.
- **L5** — a corruption flag suppresses auto-re-dispatch; a flagged hit is held for a human gate.
- **L6** — read-only on all task state: sqlite is opened `-readonly` / `mode=ro`, and the only side
  effect of any invocation is request files under `--emit-requests`.

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
- **A snapshot, not an append-only log** — each run **retracts** every
  `redispatch-<digits>-<class>.json` that is no longer a confirmed hit, *before* it emits. Without
  that, a remediated task's request would advertise an actionable request forever. Retraction is
  deliberately narrow:
  - only files this sweep could itself have emitted are touched, so a consumer's own bookkeeping in
    the same directory is left alone;
  - a `--class`-restricted run retracts **only that class** — it never deletes the requests of a
    class it did not adjudicate;
  - a run whose **DB read failed retracts nothing**. An unreadable DB reports zero candidates too,
    and absence of a hit is evidence only when the query actually ran; wiping on that would be the
    sweep destroying its own output on a transient fault.
- **Retired-class DRAIN** — `gate_closure` and `unmet_dependency` are names a consumer may still
  **see** in the directory but will never see **emitted** again. Both stay recognised by the
  retraction loop purely so a leftover from a pre-#7349 run is removed rather than orphaned: the
  consumer is request-**driven**, so an orphaned `redispatch-<id>-gate_closure.json` would keep
  producing `set_task_status('cancelled')` on every pass and the loop would outlive the fix. A
  retired class can never enter the keep-set (no row classifies into one), so a `--class all` run
  always drains it, inheriting the three scoping rules above unchanged. Each drain gets its own
  `[info]` line naming #7349, worded differently from an ordinary supersession, so an operator
  reading the nightly journal can tell the two apart.
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
- after any merge-gate red on `main` — and again once `main` advances past it.

The two cadences the retired classes motivated (after an escalation-watcher resolution sweep;
after a task reaches a terminal status) no longer apply to this sweep. The second is served by
dark-factory's own `redispatch_stranded_blocked` tick phase — see **Retired classes**.

**The timer itself is not wired by this task.** It belongs in dark-factory:
`dark-factory-orchestrator.yaml` loads once at startup and a task running under the orchestrator
must not restart it, so wiring the recurring invocation from inside a reify task is not possible.
A follow-up is filed for it; until it lands, the sweep is a manual/ad-hoc run.

## Retired classes (#7349, 2026-09-09)

Two of the three original trigger classes were retired **at the source**, rather than defused in
the consumer. Both had already fired on live tasks, and every firing in the retained journal was
collateral.

### `gate_closure` — nine collateral firings, zero correct ones

Predicate: a `blocked` task with `metadata.task_kind = deterministic` and
`metadata.always_escalates` truthy, whose live escalation dir held no `status=pending`
`esc-<id>-*.json`. Action `close`, which the consumer turned into
`set_task_status('cancelled')` — the single most destructive thing this family can do.

The retained journal holds **nine firings across six tasks, all collateral**. The absence of a
live pending escalation is simply not evidence that a deterministic gate task is finished: an
escalation gets resolved routinely while the work it was filed against is still open.

Retiring it removed the sweep's **only** read of the escalation store, and with it the
`--escalations` flag, `REIFY_GATE_STALENESS_ESCALATIONS_DIR`, the `<repo>/data/escalations`
default, the `GATED` verdict and the `task_kind` / `always_escalates` enumeration columns. The
sweep now reads exactly two things: the task DB (read-only) and the git repo.

### `unmet_dependency` — retired for OWNERSHIP, not correctness

Predicate: a `blocked` task with ≥1 `dependencies` row where every `depends_on` had reached a
terminal status. Action `redispatch`.

The predicate was right. The problem was that it had a **second owner**, which was verified before
removal:

- dark-factory's `Scheduler._phase_redispatch_stranded_blocked`
  (`orchestrator/src/orchestrator/scheduler.py:6458`) is a registered scheduler **tick** phase
  (`'redispatch_stranded_blocked'`, :1887), on by default
  (`config.py: stranded_blocked_redispatch_enabled = Field(default=True)`). Its predicate is
  `status == blocked` (:6559) ∧ no live claimant (`is_stranded_blocked`, :6583) ∧ `_deps_satisfied`,
  which accepts **both** terminal statuses (:4545) (:6585). Its action is `set_task_claimant(None)`
  then `set_task_status('pending')` (:6648-6649) — byte-identical to what the consumer's
  `_apply_repend` did for a class-C request. So the overlap was owned at **tick** cadence, not
  nightly.
- Class C's entire **non-overlapping** delta was DF's two **deliberate refusals**.
  `task_kind == 'deterministic'` is skipped at :6572 under an explicit *"DESIGN GAP carve-out …
  owned exclusively by the deterministic gate flow … redispatching it here would race/duplicate
  that flow"*, and an open pending escalation vetoes at :6621 with `LeaveReason.escalation_pinned`,
  whose comment reads *"a false 'no open escalation' would redispatch a deliberately parked task
  (the esc-3163 lesson)"*. reify's class C had **neither** check — so it was not filling a coverage
  gap, it was a second owner overriding those refusals.
- The one firing that had been cited as **correct** was collateral too.
  `data/redispatch-requests/consumed/redispatch-5318-unmet_dependency.json`
  ("all 1 dependency(ies) terminal: 5214=done") has mtime **2026-09-08 03:32:41 UTC**;
  `esc-5318-7` (level 2, `blocking`, `design_concern`) has `resolved_at`
  **2026-09-08T12:49:53Z**. The escalation was therefore still **pending** when class C fired:
  5318 was parked awaiting a human ruling and the sweep re-pended it ~9h early. It is
  `task_kind: normal` with its dependency `done`, so once the escalation closed DF's own tick
  sweep would have re-pended it correctly and unaided — as it did; the task is `pending` today.

Retiring it removed the enumeration query's **only join** (the correlated `group_concat` roll-up
over `dependencies` with its tag-scoped `LEFT JOIN tasks`), and collapsed the class-precedence
dispatcher, since one class cannot have a precedence order or an `also:<class>` disclosure.

### Why the drain exists

Retiring a class in the classifier alone would **not** have stopped it. The consumer is
request-**driven**: a `redispatch-<id>-gate_closure.json` left in the `--emit-requests` directory
by a pre-#7349 run keeps routing through `CLASS_ACTION` → `_apply_close` →
`set_task_status('cancelled')` on every consumer pass, forever. Both retired names therefore stay
**recognised** by the retraction loop, as an explicit drain set — see the `--emit-requests`
consumer contract above. An operator who finds a stale retired-class request file should expect the
next `--class all` sweep to remove it and to say so in the journal.

### A counter-example to the usual seam

The house convention is **reify ships the primitive, dark-factory wires the invocation**. This is a
genuine counter-example, and worth recording as one: the primitive and the actuator **disagreed
about policy** across the seam. reify's class C had no deterministic carve-out and no
escalation-pinned veto; DF's tick-cadence owner had both, deliberately. Because the seam carries
requests rather than decisions, reify's policy silently won on the nights the sweep ran — a
primitive can override a wired invocation's refusals without either side noticing. The consumer
still carries `gate_closure` and `unmet_dependency` in its `CLASS_ACTION` and `LEGAL_STATUSES`
tables; removing them is filed against dark-factory, not done from here.

## First sweep findings (2026-07-26)

The live measurements this design was derived from — recorded as the evidentiary basis for the
predicates, not as live assertions (the suite is hermetic; see below). **Historical:** the class-A
and class-C findings below motivated predicates that #7349 has since retired; the class-C
false-positive measurement still stands, and is now the reason `merge_verify_red`'s
`in-progress` scope is guarded by L1 rather than by a narrower status filter.

- **Class A: 5537 / 5549 / 5559** — `always_escalates=true`, still `blocked`, with **zero** live
  `esc-<id>-*.json`; their escalations are archived `dismissed` (~08:10Z). Three live instances of
  #5316 §5's un-closed-gate gap. These are other tasks' records, so a follow-up is filed rather
  than acting on them here.
- **Class C shape: 5372** — `blocked`, one dependency on 5271, which is `done`.
- **A claimant-less stale `in-progress`: 5196** — NULL `claimant_run_id`, `updated_at` ~3h stale.
- **The measured false-positive hazard:** all **ten** live `in-progress` tasks had every dependency
  `done` — **task 5321 itself among them**. A naive "dependency premise resolved ⇒ re-dispatch"
  rule would have targeted ten actively-running agents: the capability would have *destroyed* work
  rather than recovered it. That measurement is why the liveness guard is L1, and why the retired
  class C was `blocked`-only.

## Human-gate seed batch status

The four instances named in this task's brief were **already redispatched or closed** as of
2026-07-26: 5236 `pending`, 5271 `done`, 5316 `done`, 5373 `pending`. They therefore serve as the
suite's **frozen fixture shapes**, not as live detections — a live-DB assertion on them would be
both non-hermetic and already false. The whole suite is hermetic by construction: a synthetic
`tasks.db` built from the production DDL and a synthetic git repo per block; every SHA in an
assertion is computed from the fixture repo, never frozen. (Before #7349 each block also built a
synthetic escalation dir; the sweep no longer reads one.)

## Pointers

| Topic | Source |
|---|---|
| Corruption signature catalog + remediation recipe (#5316) | `docs/notes/offline-lane-red-corruption-remediation.md` |
| Read-only task-DB access contract (`REIFY_LANE_TASK_*`) | `scripts/lane-task-status.sh` |
| The advisory-observability script family this clones | `scripts/warm-lane-audit.sh`, `docs/notes/warm-lane-audit-runbook.md` |
| Hermetic test suite | `tests/infra/test_deterministic_gate_closure_staleness_sweep.sh` |
| Test-bucket registration (`pool`) | `tests/infra/run-all-classification.manifest` |
| Verify-pipeline artifact↔test mapping | `scripts/verify-pipeline-infra-tests.txt` |

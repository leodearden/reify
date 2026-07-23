# Offline-lane (spawn_context=offline_lane_red) auto-filer corruption — audit + remediation

**Task #5316 | 2026-07-23**

---

## Purpose and scope

The offline-deep-test lane auto-files a reify task whenever it detects a red run
(`metadata.spawn_context = "offline_lane_red"`, set by the dark-factory
`orchestrator/offline_lane.py` worker). Two independent bugs have been found to
corrupt an auto-filed task's record after the fact — see the Corruption
Signature Catalog below. This note is a two-part deliverable mirroring task
#5309's precedent:

1. **A one-time systematic audit** ([Audit Findings](#audit-findings-part-a))
   of every task that has ever carried `spawn_context=offline_lane_red`,
   checked against both signatures.
2. **A reusable remediation recipe**
   ([Remediation Recipe](#remediation-recipe-part-b)) codifying how a
   confirmed corruption on a `done`/`cancelled` task gets corrected, so a
   future recurrence — before #5308's parser fix lands, or from a
   not-yet-seen third corruption source — doesn't need a bespoke
   investigation each time.

This is a documentation runbook, not a new automated detector: the audit
surface is two tasks and both are already clean (see Audit Findings), and
Signature 1's root cause is being eliminated at the source by task #5308. See
"Automation home" at the end of the Detection Procedure below for what to
build instead, if a permanent detector ever becomes warranted.

## Corruption signature catalog

### Signature 1 — `--help` usage-text ingested as failing tests

`scripts/verify.sh`'s argument-error path prints the full `--help` usage text
to stdout instead of a clean one-line error (triggered e.g. by an
as-yet-unsupported flag such as `--test-threads=1`). The offline-lane
auto-filer (`orchestrator/offline_lane.py` in dark-factory; root-cause fix
tracked by LIVE task **#5308**, Stage 1 finding
`1ff0eacb-496a-4298-bad6-84976a8edf9b`) mis-parsed that dump, ingesting each
usage/help line as if it were an individual failing-test identifier —
fabricating dozens of bogus `metadata.failing_tests` entries and folding them
into the task's title and description verbatim.

**Detection markers:**
- Any `metadata.failing_tests` entry containing `Usage:`, `Options:`, or the
  `verify.sh: ERROR — unknown argument` banner text.
- More generally: any `failing_tests` entry that doesn't look like a test
  name/path shape (a real entry looks like `test_foo.sh` or a
  `tests/infra/...` path; a fabricated one looks like a CLI usage/flag-help
  line).
- Clean comparison sample: task #5295's `metadata.failing_tests` (see Audit
  Findings) shows the parser working correctly for a normal per-test-failure
  report — the bug is specific to the argument-parsing-error path, not the
  general failing-test path.

### Signature 2 — misattributed `done_provenance.commit` / `metadata.files`

A task's `metadata.files` and/or `metadata.done_provenance.commit` point at a
file set / commit that actually belongs to a **different** task, rather than
the commit that shipped this task's own fix.

**Detection markers:**
- `metadata.files` has an empty (or near-empty) intersection with the file set
  of `git show --name-only <done_provenance.commit>`.
- `metadata.files` is inconsistent with the task's own title/description
  (e.g. the title names one test file but `metadata.files` names an unrelated
  one).
- Precedent (task #5309, Stage 1 finding
  `6c5e15cb-20b8-4c8f-a994-9960f38b26ae`) shows final adjudication needs a
  human/git-history judgment call — comparing candidate SHAs to see which one
  actually shipped the fix — not just a mechanical field diff. See the
  Remediation Recipe's git-history verification step below.

## Detection procedure

Read-only, hand-runnable, no new tooling required.

**Step 1 — enumerate every task that has ever carried the spawn context:**

```bash
sqlite3 "file:/home/leo/src/reify/.taskmaster/tasks/tasks.db?mode=ro" \
  "SELECT id,status FROM tasks WHERE json_extract(metadata,'$.spawn_context')='offline_lane_red';"
```

MCP equivalent: `get_tasks`/`search_tasks` filtered client-side on
`metadata.spawn_context == "offline_lane_red"`, or `get_task(id=<id>)` for a
single already-known candidate.

**Step 2 — pull the fields each signature needs, per candidate task:**

```bash
DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db
ID=<task id>
sqlite3 "file:$DB?mode=ro" "SELECT json_extract(metadata,'\$.failing_tests') FROM tasks WHERE id=$ID;"
sqlite3 "file:$DB?mode=ro" "SELECT json_extract(metadata,'\$.files') FROM tasks WHERE id=$ID;"
sqlite3 "file:$DB?mode=ro" "SELECT json_extract(metadata,'\$.done_provenance') FROM tasks WHERE id=$ID;"
```

(The `\$` escaping above is only needed because these three lines are
themselves embedded in a `DB=`/`ID=` variable-bearing shell snippet; run
standalone — as in Step 1 — a bare `$.foo` needs no escaping.) MCP
equivalent: `get_task(id=<id>)` returns all three fields under `metadata` in
one call.

**Step 3 — apply the signature checks:**

- **Signature 1:** scan the `failing_tests` array (and the title/description)
  for `Usage:`/`Options:`/`ERROR — unknown argument` markers, or any entry
  that doesn't look like a test name/path.
- **Signature 2:** run `git show --name-only <done_provenance.commit>` and
  diff the result against `metadata.files`; also eyeball `metadata.files`
  against the task's own title/description for a topical mismatch.

**Automation home, if ever warranted:** `crates/reify-audit` already models
`metadata.files` and `metadata.done_provenance` as first-class fields —
`TaskMetadata{ files: Vec<String>, done_provenance: Option<DoneProvenance>,
.. }` and `DoneProvenance{ kind, commit, note }`
(`crates/reify-audit/src/lib.rs:217-256`), populated via the fused-memory MCP
loader in `crates/reify-audit/src/fused_memory_client.rs`. A permanent
Signature-2 detector could be built as a new reify-audit pattern with no new
data-modeling work. Signature 1 is **not** currently modeled — `TaskMetadata`
has no `failing_tests` or `spawn_context` field today — so a permanent
Signature-1 detector would need those fields added to `TaskMetadata` first.

## Audit Findings (part a)

**Sweep date:** 2026-07-23 (this task, re-running the Step 1 enumeration
query above against the live `tasks.db` immediately before recording these
findings). Result:

| id | status |
|----|--------|
| 5264 | done |
| 5295 | done |

No other task has ever carried `spawn_context=offline_lane_red`. **Both are
already remediated — no un-corrected victims remain.**

### Per-task findings

| Task | Signature 1 — help-text-as-failing-tests | Signature 2 — misattributed files/commit |
|---|---|---|
| **#5264** — "offline-lane red: verify.sh: ERROR — unknown argument '--test-threads=1'…" | **Present, corrected.** Title/description/`metadata.failing_tests` originally listed ~48 fabricated entries (verify.sh `--help` usage lines mis-ingested by the auto-filer parser bug). Corrected 2026-07-21 to the single real triggering line, `"verify.sh: ERROR — unknown argument '--test-threads=1'"` — the task's current description states this explicitly ("CORRECTED 2026-07-21 (esc-5315-1): the ONLY real failure was…") and `metadata.record_correction` records the same. `done_provenance.commit=56380a8f8adcc74886bd46459e0d6115a1d388bc`, verified via `git show --stat` to be the "Merge task/5264 into main" merge touching exactly `scripts/verify.sh`, `tests/infra/run-all-classification.manifest`, `tests/infra/test_verify_test_threads.sh`. Human-gate: **esc-5315-1**, tracked by task **#5315** ("Human gate: confirm real failure for task 5264…"), now `cancelled` — closed once the correction was verified landed, mirroring #5309's closure. Root-cause parser fix: LIVE task **#5308**. | Not present. `metadata.files=["scripts/verify.sh","tests/infra/test_verify_test_threads.sh"]` is consistent with both the title and the `done_provenance.commit`'s actual diff — no misattribution. |
| **#5295** — "offline-lane: fix 1 failing test(s) (test_cpu_load_governance_deflake.sh)" | Not present. Task #5308's own investigation names 5295 as the clean comparison sample showing the parser working correctly on a normal per-test-failure report: `metadata.failing_tests=["test_cpu_load_governance_deflake.sh"]` is a real test name, not a `--help` dump. | **Present, corrected.** `metadata.files` originally read `["tests/infra/test_run_all_content_skip.sh"]` with `done_provenance.commit=264ee8cd202393a1bb6bad5b68d00016c7b7ddad` — both misattributed from task #5273's unrelated commit ("amend(5273): document drift-guard is best-effort + broaden its anchor set", which indeed touches `tests/infra/run-all-skip-closures.manifest` and `tests/infra/test_run_all_content_skip.sh`, confirmed via `git show --stat`). Corrected 2026-07-20 to `metadata.files=["tests/infra/test_cpu_load_governance_deflake.sh"]`, `done_provenance.commit=b470abbbad7965a4b3ebb736d744f130100a4527` — verified an ancestor of `main` and the last commit to touch that test file (`git show --stat` confirms the message "fix(5295): GREEN — restore SUT hermeticity via REIFY_CPU_GOVERN_DISABLE=1"; this is the rebased/landed form of pre-rebase branch commit `b29f086`, which is itself not on `main`). Human-gate: **esc-5309-1**, tracked by task **#5309**, now `cancelled`/terminal — the precedent this note's Remediation Recipe generalizes. |

**Conclusion:** the systematic sweep confirms both known offline-lane
auto-filer corruption classes are corrected on `main` as of 2026-07-23, and no
additional `offline_lane_red` victims exist. See the Remediation Recipe's
re-run trigger for when to repeat this sweep.

## Remediation Recipe (part b)

The reusable end-to-end correction workflow for a confirmed corruption on a
`done`/`cancelled` task, generalized from task #5309's precedent (the
`metadata.files`/`done_provenance.commit` correction on task #5295).

### 1. Confirm and classify

Run the Detection Procedure above against the candidate task. Determine which
signature (or both, or neither) applies before touching anything — do not
"fix" a task whose `metadata.files` merely looks unusual without confirming
it against `done_provenance.commit`'s actual diff and the task's own
title/description.

### 2. The terminal-write constraint

A plain `update_task` call issued **from the automated recon-stage
reconciliation pass** against a `done`/`cancelled` task is rejected outright.
Task #5309 hit this directly: "Stage 2 re-verified the divergence still
present and attempted the suggested correction via `update_task`, which was
REJECTED: `error_type=ReconTerminalWriteRejected` — 'Terminal tasks (done,
cancelled) are frozen against recon-stage `update_task` writes.'" So a
terminal victim's corrupted fields **cannot** be edited via the ordinary
automated path — this is a deliberate guard against the automated
reconciliation loop silently rewriting historical terminal-task records, not
a blanket "terminal tasks are immutable" rule (see path (b) below).

### 3. Two correction paths, keyed by field

**(a) `done_provenance.commit`.** `update_task` itself refuses to write
`metadata.done_provenance` at all (backend guard `done_provenance_via_update_task`),
regardless of caller. The sanctioned repair path is
`set_task_status(id=<id>, status="done", done_provenance={...})` called
against the already-`done` task. It returns
`{"success": true, "no_op": true, "done_provenance_repaired": true}`: the
status is left untouched (no re-transition, no reconciliation storm), and the
new provenance is schema-validated and ancestor-checked against `main` before
being stamped. This is the path task #5309 identified and the one used to
land task #5295's corrected `done_provenance.commit=b470abbbad7965a4b3ebb736d744f130100a4527`.

**(b) `metadata.files` / `metadata.failing_tests` / title / description.**
These are **not** covered by the `set_task_status` corrective path (it only
repairs `done_provenance`) and — per §2 — cannot be corrected by the
automated recon-stage calling `update_task` directly. The mechanism actually
used to land both #5295's `metadata.files` fix and #5264's
title/description/`metadata.failing_tests` fix was: file a dedicated
**human-gate task** (see §5) and have the correction applied via a
non-recon-stage `update_task` call once a human (or the escalation-resolution
handler acting on their behalf) has reviewed and authorized it — the guard in
§2 is scoped to the automated recon-stage caller, not to `update_task` as
such. When performing that write: read the task first and preserve every
unrelated metadata key — `update_task`'s `append` semantics are asymmetric:
`append=true` only *adds new keys* (existing keys untouched), while
`append=false` *replaces the entire metadata object*, silently dropping any
key you didn't resend.

### 4. Human git-history verification (mandatory before any commit-attribution rewrite)

Before rewriting a misattributed `done_provenance.commit`, run `git show
--stat <recorded_sha>` and `git show --stat <candidate_sha>` and read both
commit messages to confirm which one actually shipped the task's own fix —
do not assume the candidate is correct just because it touches a
plausible-looking file. This is exactly the two-SHA adjudication task #5309
performed and this audit re-verified (see Audit Findings): `264ee8cd` is
task #5273's "amend(5273): document drift-guard is best-effort…" commit
(touches `run-all-skip-closures.manifest` + `test_run_all_content_skip.sh`),
while `b470abbbad` is task #5295's own "fix(5295): GREEN — restore SUT
hermeticity via `REIFY_CPU_GOVERN_DISABLE=1`" fix, confirmed an ancestor of
`main` and the last commit to touch `test_cpu_load_governance_deflake.sh`.

### 5. Human-gate / escalation entry point

Because §2 blocks the direct automated path, file the correction as a
dedicated **human-gate task**: `task_kind=deterministic`,
`always_escalates=true`, describing the confirmed corruption (from §1), the
exact current (wrong) field values, and the proposed corrected values — this
is the shape of both precedent tasks #5309 and #5315. That task immediately
raises a gating escalation (the `esc-<task_id>-<n>` id, e.g. `esc-5309-1`,
`esc-5315-1`); a human or the escalation-resolution handler then performs the
authorized write per §3. **Close the loop:** once the correction is verified
landed (re-`get_task` the victim and confirm the field values), transition
the human-gate task itself to `cancelled` — do not leave it `blocked`. Task
#5315 initially missed this closure step (left `blocked` after its gated
correction had already landed) until task #5321 flagged the gap and it was
cancelled to match; #5321 is now building a standing sweep for exactly this
closure-staleness pattern (see Cross-references).

## Cross-references

| Task | Role |
|---|---|
| **#5308** | Root-cause parser fix for Signature 1 (auto-filer `--help`-usage-text ingestion). LIVE as of this audit. Once landed, Signature 1 cannot recur. |
| **#5264** | Signature-1 victim; corrected. Human-gate: **#5315** (cancelled). |
| **#5295** | Signature-2 victim; corrected. Human-gate: **#5309** (cancelled) — the precedent this recipe generalizes. |
| **#5321** | Standing recon capability generalizing the human-gate closure-staleness check (§5's "close the loop" step) beyond `offline_lane_red`; depends on this note. |

## Re-run trigger

Re-run the Detection Procedure's Step 1 enumeration whenever a new task
appears with `metadata.spawn_context=offline_lane_red`, and in particular:

- **Before task #5308's parser fix lands** — Signature 1 can still occur.
- **Whenever a `done`/`cancelled` offline-lane task's `metadata.files` looks
  inconsistent with its own title/description** — Signature 2 is a
  filer/attribution bug independent of #5308's fix and can recur from a
  different root cause even after #5308 lands.

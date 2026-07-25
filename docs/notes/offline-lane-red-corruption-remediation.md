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
   future recurrence — which is expected, since Signature 1's root cause is
   **unfixed** (see below) — doesn't need a bespoke investigation each time.

**Signature 1's root cause is not being fixed.** Task #5308, which tracked the
auto-filer parser fix, was **cancelled** (terminal) on 2026-07-24 without the
fix landing. The nearest live work, task #5368
(`docs/prds/verify-confirm-failed-self-discovery.md`, now `done`), is
explicitly scoped as the *complementary* deliverable, not a successor: its own
description says "Keep SEPARATE from task 5308 (the auto-filer parser-bug fix
that CAUSED the mis-scrape). 5308 stops the false premise recurring; THIS task
delivers the actually-missing capability." So `--confirm-failed` now exists,
removing *one* trigger of the argument-error path, but the auto-filer will
still ingest usage text as `failing_tests` for **any** future unsupported
flag. Signature 1 can recur indefinitely.

This note remains a documentation runbook rather than a new automated
detector, but on narrower grounds than "the root cause is going away": the
audit surface is three tasks, the remediation for each is a human-judgment
git-history adjudication (§4) that a detector could flag but not perform, and
a Signature-1 detector would need new `TaskMetadata` fields that do not exist
today. See "Automation home" at the end of the Detection Procedure for what to
build if a permanent detector becomes warranted — with Signature 1's root
cause unfixed, that bar is now materially closer than it was.

## Corruption signature catalog

### Signature 1 — `--help` usage-text ingested as failing tests

`scripts/verify.sh`'s argument-error path prints the full `--help` usage text
to stdout instead of a clean one-line error (triggered e.g. by an
as-yet-unsupported flag such as `--test-threads=1`). The offline-lane
auto-filer (`orchestrator/offline_lane.py` in dark-factory; root-cause fix was
tracked by task **#5308**, Stage 1 finding
`1ff0eacb-496a-4298-bad6-84976a8edf9b` — **#5308 is `cancelled`/terminal as of
2026-07-24 and the fix never landed**, so this signature is live) mis-parsed
that dump, ingesting each
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
- **`done_provenance.commit` is not an ancestor of `main`.** This is the
  single highest-yield check and it must be run *first* — see the warning
  below.
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

> **Warning: `git show --stat` alone cannot detect the discarded-duplicate-merge
> variant.** A recorded SHA can be a *real, plausible-looking* merge commit for
> the right task — correct subject line, correct parents, a diff touching
> exactly the expected files — and still never have landed on `main`. This is
> what happened to task #5264 (see Audit Findings): the merge queue produced
> two "Merge task/5264 into main" commits with identical parents two hours
> apart; the earlier one was discarded and survives only on an unrelated
> branch, while the later one landed. Inspecting the recorded SHA's diff is
> reassuring and *wrong*. The only check that catches this is reachability:
>
> ```bash
> git -C /home/leo/src/reify merge-base --is-ancestor <recorded_sha> main \
>   && echo "on-main" || echo "NOT ON MAIN — Signature-2 candidate"
> git -C /home/leo/src/reify branch -a --contains <recorded_sha>   # where does it live?
> ```
>
> Run this before, not after, reading the diff. The first pass of this audit
> checked #5264 with `git show --stat` only and signed a live corruption off
> as clean.

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
- **Signature 2 (in this order):**
  1. **Reachability first** — `git merge-base --is-ancestor
     <done_provenance.commit> main`. A non-zero exit is a confirmed
     Signature-2 hit on its own, regardless of how plausible the diff looks;
     follow up with `git branch -a --contains <sha>` to see what the SHA
     actually is. See the discarded-duplicate-merge warning above.
  2. Then run `git show --name-only <done_provenance.commit>` and diff the
     result against `metadata.files`; also eyeball `metadata.files` against
     the task's own title/description for a topical mismatch.

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

**Sweep date:** 2026-07-26 (re-run of the Step 1 enumeration query against the
live `tasks.db`, with the reachability check of Step 3 applied to every
`done_provenance.commit`). Result:

| id | status | `done_provenance.commit` | ancestor of `main`? |
|----|--------|--------------------------|---------------------|
| 5264 | done | `f84836f062…` (corrected 2026-07-26; was `56380a8f8a…`) | yes (after correction) |
| 5295 | done | `b470abbbad…` (corrected 2026-07-20) | yes |
| 5368 | done | `ef6863e770…` | yes |

No other task has ever carried `spawn_context=offline_lane_red`.

> **This supersedes an earlier 2026-07-23 sweep recorded in this note, which
> was wrong on two counts** and is retained here as a worked example of how
> the audit can fail:
> - It listed only #5264 and #5295, **missing #5368** — a third victim that
>   was auto-filed and Signature-1-corrupted on 2026-07-23, the same day.
> - It cleared #5264 of Signature 2 on the strength of `git show --stat`
>   alone, **missing a live misattribution** (details in the per-task table).
>
> Both misses are now covered by procedure changes: the reachability-first
> ordering in Step 3, and the re-run trigger's same-day-recurrence caveat.

### Per-task findings

| Task | Signature 1 — help-text-as-failing-tests | Signature 2 — misattributed files/commit |
|---|---|---|
| **#5264** — "offline-lane red: verify.sh: ERROR — unknown argument '--test-threads=1'…" | **Present, corrected.** Title/description/`metadata.failing_tests` originally listed ~48 fabricated entries (verify.sh `--help` usage lines mis-ingested by the auto-filer parser bug). Corrected 2026-07-21 to the single real triggering line, `"verify.sh: ERROR — unknown argument '--test-threads=1'"` — the task's current description states this explicitly ("CORRECTED 2026-07-21 (esc-5315-1): the ONLY real failure was…") and `metadata.record_correction` records the same. Human-gate: **esc-5315-1**, tracked by task **#5315** ("Human gate: confirm real failure for task 5264…"), now `cancelled` — closed once the correction was verified landed, mirroring #5309's closure. Root-cause parser fix: task **#5308**, now `cancelled` without landing — this signature remains live. | **Present, corrected 2026-07-26 (this audit).** Recorded `done_provenance.commit` was `56380a8f8adcc74886bd46459e0d6115a1d388bc`, which is **not an ancestor of `main`** — `git branch -a --contains` places it only on `task/5259`. It is a *discarded duplicate* merge: correct subject ("Merge task/5264 into main"), correct parents (`0362506f`, `61134e64`), and a diff touching exactly the three expected files, which is why the first pass's `git show --stat` check cleared it. The merge that actually landed is **`f84836f06268a0e492526d7bea10c6ee6e4412c5`** — identical parents, committed 2h later (2026-07-20 12:51:35 +0100), additionally touching `docs/prds/v0_6/fea-load-support-selector-migration.md`, and confirmed via `git merge-base --is-ancestor f84836f062 main`. Corrected via the §3(a) path (`set_task_status(id=5264, status="done", done_provenance={…})` → `{"success": true, "no_op": true, "done_provenance_repaired": true}`) under **esc-5316-17**. `metadata.files=["scripts/verify.sh","tests/infra/test_verify_test_threads.sh"]` was already correct and is unchanged. |
| **#5295** — "offline-lane: fix 1 failing test(s) (test_cpu_load_governance_deflake.sh)" | Not present. Task #5308's own investigation names 5295 as the clean comparison sample showing the parser working correctly on a normal per-test-failure report: `metadata.failing_tests=["test_cpu_load_governance_deflake.sh"]` is a real test name, not a `--help` dump. | **Present, corrected.** `metadata.files` originally read `["tests/infra/test_run_all_content_skip.sh"]` with `done_provenance.commit=264ee8cd202393a1bb6bad5b68d00016c7b7ddad` — both misattributed from task #5273's unrelated commit ("amend(5273): document drift-guard is best-effort + broaden its anchor set", which indeed touches `tests/infra/run-all-skip-closures.manifest` and `tests/infra/test_run_all_content_skip.sh`, confirmed via `git show --stat`). Corrected 2026-07-20 to `metadata.files=["tests/infra/test_cpu_load_governance_deflake.sh"]`, `done_provenance.commit=b470abbbad7965a4b3ebb736d744f130100a4527` — verified an ancestor of `main` and the last commit to touch that test file (`git show --stat` confirms the message "fix(5295): GREEN — restore SUT hermeticity via REIFY_CPU_GOVERN_DISABLE=1"; this is the rebased/landed form of pre-rebase branch commit `b29f086`, which is itself not on `main`). Human-gate: **esc-5309-1**, tracked by task **#5309**, now `cancelled`/terminal — the precedent this note's Remediation Recipe generalizes. |
| **#5368** — "DESIGN: reify-side verify.sh `--confirm-failed` self-discovery protocol" (re-scoped; originally auto-filed as "56 failing tests") | **Present, corrected — and it is a post-#5264 RECURRENCE.** Auto-filed 2026-07-23 from `bash scripts/verify.sh test --confirm-failed` → exit 64 + usage dump; the auto-filer ingested the 59-line `usage()` output as 56 `failing_tests` entries and folded them into the title/description, exactly as for #5264. Cleared 2026-07-24 by the L2 escalation-watcher (`metadata.corrupted_autofile_metadata_cleared`, `metadata.failing_tests=[]`), and the task re-scoped to the real underlying need per Leo's decision on **esc-5368-2** (option B); it has since landed `done`. **This is the load-bearing audit finding:** Signature 1 recurred on a *different* unsupported flag three days after #5264, confirming the corruption is flag-agnostic and that #5308's cancellation leaves it live. | Not present. `done_provenance.commit=ef6863e77009a2ddf0a2523f3a6241230f6b688b` ("Merge task/5368 into main") is an ancestor of `main`, and `metadata.files=["docs/prds/verify-confirm-failed-self-discovery.md"]` matches both the re-scoped title and the landed diff. |

**Conclusion:** the sweep finds **three** `offline_lane_red` tasks, not two.
All three are corrected as of 2026-07-26 — but two of those corrections
(#5264's Signature-2 misattribution, #5368's Signature-1 ingestion) were
*missed* by this note's own first-pass audit, and #5264's was still live in
the DB until this audit repaired it. The correct standing conclusion is
therefore **not** "no victims remain" but: *no known-uncorrected victim
remains, and new victims should be expected* — Signature 1's root cause is
unfixed (#5308 cancelled), and #5368 demonstrates it recurring on a new flag
within three days. See the re-run trigger below.

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
such.

**Three fields, three different knobs.** This section's heading spans
`metadata.files`, `metadata.failing_tests`, title and description, but they
are written through three separate mechanisms. Every quotation below is from
the live `update_task` MCP tool schema, which is the **authority** here —
re-read it with `ToolSearch(query="select:mcp__fused-memory__update_task")`
before performing a correction rather than re-deriving these semantics from
memory or from this note.

- **`metadata.files` / `metadata.failing_tests`** — inside the `metadata`
  blob; governed by `metadata_mode`.
- **`title` / `description`** — top-level structured columns, *unaffected by
  `metadata_mode`*: "Each non-None field overwrites the corresponding
  column." Never route a title/description correction through `metadata`.
- **`details` / `prompt`** — governed by `append`, which the schema calls
  "the only knob that governs `details`/`prompt` append — `metadata_mode`
  does NOT affect the details path".

**For a scoped metadata correction, use the default `merge` mode.** Omitting
`metadata_mode` selects `'merge'`: "shallow last-write-wins. Omitted keys
preserved; supplied keys overwrite wholesale" (`{**existing, **incoming}`).
That is exactly what a Signature-1 or Signature-2 field repair wants —
supply *only* the key being fixed, its new value overwrites the old one
wholesale, and every unrelated metadata key is preserved automatically.

> **Warning: `metadata_mode='additive'` (and its deprecated `append=true`
> shim) CANNOT correct an existing wrong value.** `'additive'` is "recursive
> list union+dedup, scalar/type-collision OLD-wins". Worked through this
> runbook's own headline case: correcting #5295's `metadata.files` from
> `["tests/infra/test_run_all_content_skip.sh"]` to
> `["tests/infra/test_cpu_load_governance_deflake.sh"]` under `'additive'`
> yields the *union* of the two lists — the misattributed Signature-2 path
> survives while the task now reads as "corrected", which is worse than the
> original corruption because it looks remediated. For a scalar metadata
> value, OLD-wins makes the write a silent no-op. Do not use
> `additive`/`append=true` on this path.

`metadata_mode='replace'` — "whole-blob overwrite. Bypasses the corrupt-blob
guard; the sanctioned repair path" — is for the case where the metadata
**blob itself** is corrupt (unparseable or structurally wrong), not for a
scoped single-key fix; using it for a scoped fix needlessly makes every
sibling key depend on your resending it verbatim. A bare `append=False` with
no `metadata_mode` on a metadata write is **rejected** by the backend (it
used to silently whole-blob replace and wiped a live in-progress task — the
task-2180 metadata-wipe incident), so it is *not* a silent-key-drop hazard to
defend against: the backend refuses it outright.

Reading the victim task with `get_task` before the write is still worth
doing — but to capture the pre-correction values for the §5 human-gate audit
trail, not to avoid losing unrelated keys, which `merge` already handles.

### 4. Human git-history verification (mandatory before any commit-attribution rewrite)

Before rewriting a misattributed `done_provenance.commit`, run `git
merge-base --is-ancestor <sha> main` on **both** the recorded and the
candidate SHA, then `git show --stat <recorded_sha>` and `git show --stat
<candidate_sha>`, and read both commit messages to confirm which one actually
shipped the task's own fix. Do not assume the candidate is correct just
because it touches a plausible-looking file, and do not clear a recorded SHA
just because its diff looks right — a discarded duplicate merge passes the
diff test and fails the ancestor test (this is precisely how #5264 was
mis-cleared on the first pass). This is exactly the two-SHA adjudication task #5309
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
| **#5308** | Was to be the root-cause parser fix for Signature 1 (auto-filer `--help`-usage-text ingestion). **`cancelled` 2026-07-24 without landing** — Signature 1 therefore remains live and unowned. No successor task currently owns this fix. |
| **#5368** | Signature-1 victim (2026-07-23 recurrence); corrected 2026-07-24 via **esc-5368-2**. Separately, its *deliverable* — the `--confirm-failed` primitive, `docs/prds/verify-confirm-failed-self-discovery.md` — removes one trigger of the argument-error path, but is explicitly **not** #5308's parser fix and does not prevent the next unsupported flag from re-triggering Signature 1. |
| **#5264** | Signature-1 victim; corrected. Human-gate: **#5315** (cancelled). Also a Signature-2 victim — corrected 2026-07-26 under **esc-5316-17** (see Audit Findings). |
| **#5295** | Signature-2 victim; corrected. Human-gate: **#5309** (cancelled) — the precedent this recipe generalizes. |
| **#5321** | Standing recon capability generalizing the human-gate closure-staleness check (§5's "close the loop" step) beyond `offline_lane_red`; depends on this note. |

## Re-run trigger

Re-run the Detection Procedure's Step 1 enumeration whenever a new task
appears with `metadata.spawn_context=offline_lane_red`, and in particular:

- **On every new `offline_lane_red` task, indefinitely.** There is no longer a
  "once #5308 lands" end date: #5308 was cancelled without landing and nothing
  else owns the parser fix, so Signature 1 is a permanent standing risk.
- **Promptly — a victim can be auto-filed and swept on the same day.** Task
  #5368 was auto-filed and corrupted on 2026-07-23 and was still missed by
  that day's sweep. Do not assume a recent sweep covers a recently-filed task;
  re-run the Step 1 enumeration rather than trusting the last recorded table.
- **On any unsupported-flag invocation of `verify.sh`.** Signature 1 is
  flag-agnostic — `--test-threads=1` triggered it for #5264 and
  `--confirm-failed` for #5368. Any future flag the offline lane passes that
  `verify.sh`'s arg parser does not know will reproduce it.
- **Whenever a `done`/`cancelled` offline-lane task's `metadata.files` looks
  inconsistent with its own title/description** — Signature 2 is a
  filer/attribution bug independent of the parser fix and can recur from a
  different root cause.
- **Whenever the merge queue is known to have retried or re-enqueued a task**
  — that is how #5264 ended up with two candidate merge commits and a recorded
  SHA pointing at the discarded one. Apply the reachability check of Step 3 to
  every `done_provenance.commit`, not only to ones whose diff looks wrong.

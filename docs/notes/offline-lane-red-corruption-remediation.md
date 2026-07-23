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

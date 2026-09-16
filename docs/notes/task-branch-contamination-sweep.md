# Task-branch contamination sweep runbook — `scripts/task-branch-contamination-sweep.sh`

**Task #7244 | 2026-09-10** · originating incident: esc-6205-4

Operational digest for the read-only audit that answers one question per in-flight task branch:
**does this branch carry work that is not its own?**

The **normative source is the script itself** — its `-h` usage block and its header `# Invariants:`
list (R1–R5). This note is a digest that points at them; where the two disagree, the script wins.

---

## Purpose

`git worktree add -b task/N <lane> <base>` cuts a task branch at `<base>`. When `<base>` is a
commit on `main` that is the intended shape. Occasionally it is **another task's branch tip**:
`task/N` then silently contains every one of that peer's commits, and merging `task/N` lands the
peer's unreviewed work under this task's name. esc-6205-4 is the catalogued instance: `task/6205`
was reported cut at a base that was not on `main` but on `task/5686`. That reading is the
escalation's — `refs/heads/task/6205` no longer exists, so it cannot be re-measured from the pool
today.

The sweep reports two independent dimensions of that fault:

* **`signature`** — the *commit* census. Does this branch's own commit range carry commits that
  cite another live task? This is the sharp signal, and it is the one that reproduces the
  incident.
* **`scope`** — the *file* cross-check. Do the branch's changed files fall outside what the task
  declared in `metadata.files`, and if so, does a live peer declare them?

Both are **advisory**. The sweep never gates anything, never writes, and exits 0 on every valid
invocation (R3) — the exit status carries no classification, so no caller can gate on it by
accident.

## Where it runs

One primitive, two seams, following the repo's cross-repo rule (*reify ships the primitive,
dark-factory wires the invocation*) and the same two-mode shape as
`scripts/warm-lane-degenerate-ref-check.sh`:

| Mode | Invocation | Seam |
|---|---|---|
| Single branch | `--task <id>` | a pre-merge advisory consult on one branch |
| Fleet sweep | `--audit` | a timer-driven or on-demand pool sweep |

Exactly one of the two is required; supplying both, or neither, is a usage error (exit 2).

## CLI

Run `scripts/task-branch-contamination-sweep.sh -h` for the full flag table — it is not restated
here. Every flag that has an env counterpart:

| Flag | Env knob | Default |
|---|---|---|
| `--db PATH` | `REIFY_LANE_TASK_DB` | `/home/leo/src/reify/.taskmaster/tasks/tasks.db` |
| `--tag TAG` | `REIFY_LANE_TASK_TAG` | `master` |
| `--repo DIR`, `-C DIR` | — | CWD |
| `--main-ref REF` | — | `main` |
| `--branch-prefix PFX` | — | `task/` |
| `--format table\|json` | — | `table` |

`REIFY_LANE_TASK_DB` / `REIFY_LANE_TASK_TAG` are **reused verbatim** from
`scripts/lane-task-status.sh`'s contract, so the repo has one task-DB plumbing contract rather
than two that can drift. Task ids are unique only within a tag (`tasks` is `PRIMARY KEY (tag,
id)`), so every query is tag-scoped.

One knob is **env-only, with no flag**: `REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN` overrides the
sqlite3-CLI probe, and an explicitly *empty* value forces the python3 engine. The probe resolves
`/usr/bin/sqlite3` by absolute path, so stripping `PATH` cannot reach it — which makes this both
the test seam for the fallback engine and the break-glass for an incompatible CLI.

There is deliberately **no staleness-threshold knob**. See *Why `behind` is not a trigger*.

## Cost

**One tag-scoped query per invocation**, not one per branch: `id · status · metadata.files` for
every non-terminal task arrives in a single read, so a branch costs zero further store opens.
Measured on the live store: **1345 rows in 0.38s**, against **1m43s** for the per-task-oracle
shape (`scripts/lane-task-status.sh` at ~92ms × 1095 refs).

Per branch the git work is capped at four read-only invocations — one `merge-base`, two
`rev-list --count`, one `diff -z --name-only` — plus one `git log` for the census. Warm, a full
fleet sweep over today's ~351 live branches is **1–2 minutes**; cold it was ~7 minutes.

## Columns

One row per audited branch, `key=value` pairs (table) or one object under `branches` (json):

| Column | Meaning |
|---|---|
| `task` | the task id — the branch's numeric suffix |
| `status` | its Taskmaster status (non-terminal by construction) |
| `merge_base` | abbreviated `git merge-base <branch> <main-ref>` |
| `behind` | commits on `<main-ref>` since `merge_base` — **context only** |
| `commits` | commits in `<main-ref>..<branch>` |
| `peer_commits` | how many of those cite a live task that is **not** this one |
| `changed` | files in `git diff <merge_base> <branch>` |
| `foreign` | changed files absent from this task's `metadata.files` |
| `peer_files` | foreign files declared by a live task that is not this one |
| `peers` | the union of implicated task ids, sorted-unique, or `-` |
| `scope` | the file cross-check verdict (below) |
| `signature` | `SUSPECT` or `-` (below) |

A branch that could not be measured reports `-` in every count. Under `--format json` a count is a
JSON **number** and the unmeasured placeholder is the **string** `"-"`, so a consumer never parses
`-` out of an integer field.

### `scope` — the declared-scope cross-check

Resolved in this order; the order *is* the invariant, and it is total:

| Verdict | Meaning |
|---|---|
| `UNKNOWN` | the branch was not measured: the store was not read, the id is not in this tag's non-terminal set, or the git measurement failed (no such branch, unresolvable `--main-ref`, non-git `--repo`, failed diff) |
| `UNDECLARED` | the task declares no files. **Never** downgraded to `OUT-OF-SCOPE` |
| `PEER-FILES` | some foreign path is declared by a live task other than this one |
| `OUT-OF-SCOPE` | the foreign set is non-empty, but no live task declares any of it |
| `CLEAN` | the foreign set is empty |

Declared-vs-changed matching is **exact repo-relative string equality** — no prefix or glob
machinery. That is a measurement, not a shortcut: across all 1345 non-terminal tasks exactly one
declared path is not a file-with-extension (`hooks/reference-transaction`, an extensionless *file*
in `scripts/lock-charter-guard.sh`'s allowlist). Directory declarations do not exist to be
handled, so prefix matching would be an unused dimension of variability — and a wrong one, since
it would let a declared `x/a/b.rs` cover a changed `a/b.rs`.

### `signature` — the commit-citation census

`SUSPECT` **iff `peer_commits > 0`**, else `-`. A commit in `<main-ref>..<branch>` counts as a peer
commit iff its message cites at least one id that is non-terminal in the store and is not this
task's own — *and* the message does not also cite this task's own id. That last condition skips the
whole commit: an amend that says "re-lands #N, coordinated with #M" is this task's own work
referencing a sibling, not somebody else's commit riding along.

The citation grammar lives in exactly one place, `scripts/lib_task_citation.sh`, shared with
`scripts/warm-lane-degenerate-ref-check.sh`; it mirrors dark-factory `orchestrator/git_ops.py`
byte-for-byte. Never re-inline it in a consumer —
`tests/infra/test_lib_task_citation.sh` fails if a second copy appears.

`scope` and `signature` are **orthogonal**, not one collapsed verdict: a branch can be `CLEAN`
and `SUSPECT` (a foreign commit that touched only files this task also declares), or
`OUT-OF-SCOPE` and `-` (plain under-declaration).

## The summary line

Fleet mode ends with a `SWEEP:` line (table) / `summary` object (json). Its first six counters
**partition** the row count:

```
branches = suspect + peer_files + out_of_scope + undeclared + clean + unknown
```

A `SUSPECT` row is counted under `suspect` and nowhere else, which is what keeps the partition
exclusive. Three further counters cross-cut it, one per REASON a ref yielded no row — each of them
something the store positively said, never a guess standing in for an answer it never gave:

* `skipped_terminal` — the store shows the backing task `done` or `cancelled`.
* `skipped_no_task` — the store was read and does not carry that id under `--tag` **at all**. A
  four-digit count here is the signature of a mistyped `--tag`, not of a retired pool; it is also
  called out on stderr, and a tag with *no* live tasks at all gets its own louder warning, since
  that is never a legitimate steady state.
* `skipped_nonnumeric` — a ref whose suffix is not a number (`task/1741-recovered`,
  `task/208-merge`, `task/2962-20260530T173412Z`). **48 of the live pool's 1095 `task/*` refs** are
  in this class; they are skipped with a note on stderr, never silently dropped and never an error.

A task with no branch ref produces no row at all in fleet mode. In `--task` mode it *does* get a
row — a degraded `UNKNOWN` one — because the caller asked about that branch by name and is owed an
answer.

**A skip is never a degradation channel.** The store failing to answer is not a property of any
one ref, so it is not reported as one: when the store could not be consulted at all (missing,
0-byte, unreadable, no SQL engine), fleet mode measures nothing but still emits a row for **every**
`task/*` ref, each `scope=UNKNOWN` with `-` in every column, plus one warning on stderr. That is
deliberate — a short report and an all-clean report are the same bytes to a caller reading stdout,
so a timer wired with a typo'd `--db` must not be able to report a quiet fleet forever.

`--task` degrades on the store side for the same reason, and owes a row either way: a typo'd
`--db`, a wrong `--tag`, a store it cannot read, or an id whose task has gone terminal all produce
`scope=UNKNOWN` with `-` in every measured column and a warning on stderr. Read `UNKNOWN` as
*"this consult answered nothing"*, never as *"this branch is fine"* — the point of the degraded
shape is that the two can never be confused on stdout.

## How to read a report

**1. Start at `signature`.** `SUSPECT` means this branch's own commit range carries a commit
citing another live task. That is the incident's mechanism, measured directly.

**2. `SUSPECT` is investigate-then-adjudicate — never an auto-repair trigger.** The sweep reports;
a human or a merge worker decides. It is a *signal that foreign commits are present*, not a proof
that they arrived by the mis-cut-base route: a deliberate cherry-pick, a coordinated pair of tasks,
or a legitimate merge of a landed sibling all produce the same reading. Confirm by looking at
`git log <main-ref>..task/N` before acting, and never rewrite a branch on the strength of this
column alone.

**3. `OUT-OF-SCOPE` and `PEER-FILES` usually mean UNDER-DECLARATION, not contamination.** This is
the single most important thing to know about the file half of the report. Measured over the 351
live branches: 19 carry at least one foreign file, 13 of those have a foreign file a live peer also
declares — but spot-checking the strongest hits shows ordinary under-declaration. `task/4194`'s six
foreign paths are all *test siblings of its own declared sources* (`geometry_ops/tests.rs` beside a
declared `geometry_ops.rs`); `task/7245`'s are `Cargo.lock` plus one adjacent test file. Treat
these two verdicts as "the declaration and the diff disagree", and check which one is wrong.

**4. `peer_files` is weak evidence on a hot file.** Peer ownership is common by construction:
among non-terminal tasks, `crates/reify-eval/src/engine_build.rs` is declared by **107** of them,
`crates/reify-core/src/diagnostics.rs` by **98**, `engine_eval.rs` by **90**. Any foreign file in a
busy crate will be "peer-owned". The `peers` column is a lead to follow, not a finding.

**5. `UNDECLARED` is a normal state, not a defect.** **186 of the store's 1345 non-terminal tasks**
declare `files: []` — the documented defer-to-architect value (`scripts/lock-charter-guard.sh`
header). Reporting those as "every changed file is foreign" would manufacture 186 false positives,
which is why the verdict exists and is never downgraded to `OUT-OF-SCOPE`.

**6. Ignore `behind` as a verdict input.** See below.

## Why `behind` is not a trigger

The obvious signature for this defect — *merge-base far behind main* — **does not discriminate**,
and the numbers are not close. Measured over all 351 live task branches:

| statistic | commits behind `main` |
|---|---|
| min | 0 |
| p25 | 878 |
| **median** | **2201** |
| p75 | 3781 |
| p90 | 5324 |
| max | 17724 |

345 of 351 (**98%**) are ≥ 50 behind; 329 are ≥ 200; 304 are ≥ 500. The incident's own branch,
`task/6205` at **1969** behind, sits *below* the pool median. Any defensible threshold therefore
fires on 87–98% of the pool and separates nothing, so `behind` is reported as **context only** and
no threshold knob exists to be mis-tuned (R5).

## Why `metadata.branch_base_sha` is not the oracle

The task record stores the base the branch was cut from, which looks like the natural check. It is
not. Measured at #7244 planning time: **task 6205's recorded `branch_base_sha` was a commit on
`main`** — and was *also* an ancestor of the bad merge-base. A recorded base being on `main` is therefore consistent with the branch
having been cut somewhere else entirely, so the field cannot distinguish the fault. The sweep reads
the refs as they actually are instead.

## Invariants

Pinned in the script header as R1–R5 and asserted by
`tests/infra/test_task_branch_contamination_sweep.sh` block 7, each paired with a
mutation-injection check proving the assertion can fail:

* **R1** — read-only on the task store: opened strictly `-readonly` / `mode=ro`, and never
  created. Pointing `--db` at a nonexistent path leaves it absent, with no `-wal` / `-shm` /
  `-journal` sibling.
* **R2** — read-only on the repo: every git call goes through one wrapper setting
  `GIT_OPTIONAL_LOCKS=0`, so no read can take a lock or refresh the index. The only files written
  anywhere are two `mktemp` temporaries under `$TMPDIR`.
* **R3** — non-gating: exit 0 on every valid invocation in both modes, whatever it finds. The only
  non-zero exit is 2, for a usage error or for `--format json` on a host with no python3 (refused
  up front, before any measurement). Stdout is the only result channel.
* **R4** — fail-safe degradation: an unreadable store, an id absent from the tag's non-terminal
  set, an unresolvable ref, a failed diff or a failed SQL engine degrades the affected row (or the
  whole report) to `UNKNOWN` with a warning on stderr, never an abort. The verdict is decided
  *before* measurement, so a degraded row carries `-` in every column and can never be read as a
  benign one. Fleet mode's "emit no row at all" is **not** a degradation channel: a ref is dropped
  only on the store's positive evidence about it, each such reason carrying its own counter.
* **R5** — `behind` is context, never a trigger.

## Cross-references

| Topic | Source |
|---|---|
| The citation grammar (single copy, reify/dark-factory seam) | `scripts/lib_task_citation.sh` |
| Sibling two-mode read-only classifier | `scripts/warm-lane-degenerate-ref-check.sh` |
| Sibling batched-store-read sweep | `docs/notes/deterministic-gate-closure-staleness-sweep.md` |
| Lane occupancy / disk reporting (a different question) | `docs/notes/warm-lane-audit-runbook.md` |
| Declared-scope contract (`metadata.files`, the empty-list value) | `scripts/lock-charter-guard.sh` header |

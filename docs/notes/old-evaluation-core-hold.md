# The hold on the old evaluation core

**Task #10510 | 2026-11-25**

Reify's evaluation engine is being replaced. A new core is built beside the
old orchestration, the new engine is brought to full coverage, and then every
surface (CLI, language server, GUI) switches in one release that also deletes
the old orchestration. Until that release, work on the old orchestration is
held: an agent spent on it is working on code that is due for deletion.

This note, with the list beside it, is the one place that says what "the old
orchestration" is and how a task is held or released. Task #10510 is the
holder and points here.

## The files

The held files and the holder's id are data in
[`old-evaluation-core-hold.toml`](old-evaluation-core-hold.toml) beside this
note, their one home: one repo-relative path per entry, a trailing slash
naming a directory. They are matched one by one, never by a prefix of the
crate's name: the new core lives in a crate of its own, and a prefix match on
`crates/reify-eval` would sweep its tasks into the hold. `registration_only`
marks `lib.rs`.

Everything else is kept code and is not held: the expression evaluator, the
compiler, the geometry kernels, the solvers, FEA and the other analyses,
tolerance mathematics, selectors, the stdlib, the CLI and GUI shells, the
language server. `lib.rs` is on the list because the old engine's public
surface lives there; a task that touches it only to register a module is not
old-orchestration work.

## The rule

A task is old-orchestration work if doing it means refactoring, patching,
extending or documenting the held files. Judge by what the task changes, not
by its file list alone: a task can list `engine_eval.rs` and be a solver
change with one call site, or list only kept files and be a patch to an old
mechanism.

- **Do not plan or land a change to these files** unless the task has been
  released from the hold by name (below).
- **A new task that is old-orchestration work is filed held.** This covers
  follow-ups and reviewer suggestions as well as planned work.
- A defect in the old engine that a user can see is still worth a task: its
  record is what the new engine's tests are written from. File it held.

## Holding and releasing one task

A task is held by an ordinary dependency edge on #10510, which never
dispatches.

- **To hold:** `add_dependency(task, 10510)`.
- **To release:** `remove_dependency(task, 10510)`, and record on the task why
  it is wanted before the switch, in `metadata.old_core_release`
  (`reason`, `approved_by`, `approved_at`). A release is the project owner's
  decision. If only part of a task is kept-code work, narrow its text to that
  part when it is released, or split it.
- **Never mark #10510 `done` or `cancelled`.** Either releases every held
  task at once. It is released, or its remaining tasks cancelled, in the
  release that deletes the old orchestration.
- **Never replace a held task's dependency list** in a way that drops #10510.
  Add and remove single edges.

## Cancelling a held task

A cancelled dependency counts as satisfied, so cancelling a held task
releases whatever depends on it. The exception is a cancelled task that
declares `metadata.delivered_checks`: its dependents then wait for a check
that can no longer pass. Settle a task's dependents before cancelling it.

## Detection

`reify-audit --pattern POLDCORE` (the `/audit` skill) reports each open task
that meets the mechanical rule (half or more of its `metadata.files`, tests
excluded, are held) yet has no edge on the holder and no
`metadata.old_core_release`; a task whose only held file is `lib.rs` is
reported at Low. It also reports the holder's own state: closed, absent, or
no longer kept from dispatching.

It cannot see a task that lost its edge unless the task meets the rule: the
store records a hold only as the edge itself, so such a task looks like one
never held. No list of held tasks is kept in the repo.

# Diagnostic Severity Policy

**Status:** Normative — cited verbatim in the `reify-audit --pattern PDIAG` failure message
**Date:** 2026-07-28
**Source:** Task 5405 (PRD `docs/prds/v0_6/eradicate-silent-undef.md` task η, §3 Leg C item 2, §7 "PDIAG baseline")
**Invariants realized:** `INV-SF-2 error-severity-exits-nonzero` (`docs/legibility/design-invariants.md` §INV-SF-2), `INV-SF-6 diagnostics-carry-codes` (§INV-SF-6)

This note answers two questions for anyone adding a diagnostic:

1. **Which severity?** — §1.
2. **Which code, and why is one mandatory?** — §2.

§3 is the remediation recipe for a PDIAG-RED diff.

This document is descriptive of what is *landed*, not a place to decide new
policy. Where it restates another task's severity choices it says so and cites
the source; disagreements are resolved in favour of the code, and this note is
corrected.

---

## 1. The three severities

`reify_core::Severity` (`crates/reify-core/src/diagnostics.rs:98-105`) has
exactly three variants. There is no `Debug` variant and none is planned.

| Severity | Rule | Exit-code effect |
|---|---|---|
| `Error` | **Never expected on a healthy path.** | Makes the CLI command exit nonzero (INV-SF-2). |
| `Warning` | Actionable degradation the user can act on. | Exit-neutral. |
| `Info` | The debug tier — narration, advisories, once-per-session standing notes. | Exit-neutral. |

### `Error` — never expected on a healthy path

INV-SF-2's rule is that *any* Error-severity diagnostic on *any* channel
(compile, eval, constraint, kernel, build) makes the command exit nonzero, with
no per-code bolt-on escalation lists.

The corollary (`design-invariants.md` §INV-SF-2) is the part that governs severity
choice, and it is the one most often got wrong:

> a diagnostic *expected* on a healthy path is by definition not
> Error-severity — demote or recode it; never exempt it from the gate.

So the test for `Error` is not "is this bad?" but "does a correct design,
correctly evaluated in a correctly provisioned environment, ever produce this?"
If yes, it is a `Warning` (or `Info`) — **not** an `Error` with an exemption.
"The kernel isn't available so we skipped the check" is the archetype: skipping
is a healthy path, so that emission is a Warning, not an exempted Error.

### `Warning` — actionable degradation

The user can do something about it, and the thing they'd do changes the result.
A Warning that no one can act on is an `Info`; a Warning that means the answer
is wrong is an `Error`.

Reify-internal provenance bugs are also Warnings, not Errors — see
`W_UNDEF_UNEXPLAINED` (PRD §6 decision 4): it reports a reify bug rather than a
user error, and Error severity would make every future coverage gap fail user
builds, defeating a backstop whose whole point is graceful loudness.

### `Info` — the debug tier

`Severity::Info` **is** the "debug" tier. This is not a choice made here; it is
the layering landed for #5196 and stated at
`crates/reify-core/src/diagnostics.rs` (`DiagnosticCode::HexWedgeForceTet`'s doc):

> The PRD "debug" tier is realized as `Severity::Info` because
> `reify_core::Severity` has no `Debug` variant; adding one would require
> cross-cutting changes well outside this task's scope.

Two landed consequences worth knowing before you pick `Info`:

- **Standing advisories are `Info`, deduped.** `FlexureFatigueCheckMissing`
  (`DiagnosticCode::FlexureFatigueCheckMissing`) is emitted once per eval session because it
  describes a surface-level gap, not a per-instance defect.
- **`Info` can be upgrade-exempt.** `HexWedgeForceTet` and
  `HexWedgePromoted` are documented as always `Info` and never
  upgraded, while their sibling `HexWedgeInvalidSweepGeometry` is
  `Info` by default and upgraded to `Error` under `require_hex_wedge=true` —
  **with the code preserved across the upgrade**. Severity is a property of the
  emission; the code is a property of the *condition* and does not change when
  severity does.

Per-code severity choices belong in the `DiagnosticCode` variant's own
doc-comment (every variant carries an `Origin:` block and a `Severity:` line
where it is non-obvious), not in this note. #5196 owns the persistent-naming
de-noise pass; this note owns the written policy.

---

## 2. Codes are mandatory (INV-SF-6)

**Rule** (`design-invariants.md` §INV-SF-6):

> Every emitted Warning/Error carries a `DiagnosticCode`.

The rationale is mechanical, not stylistic: code-less diagnostics cannot be
gated, filtered, counted, or de-noised systematically, and they force
message-substring hacks downstream. The `E_DFM_` message-prefix escalation in
the CLI exists *only* because the co-resident Error diagnostics are code-less —
that is the cost this invariant exists to stop compounding.

`Info` is out of INV-SF-6's scope, and PDIAG does not scan `Diagnostic::info`
sites. Coding an `Info` is welcome, not required.

### How to attach one

1. Add a PascalCase variant to `enum DiagnosticCode`
   (`crates/reify-core/src/diagnostics.rs:156`), with the doc-comment shape its
   neighbours use: an `Origin:` line naming the emitting function, the task /
   PRD cite, when it is emitted, and — where non-obvious — a `Severity:` line.
   The enum is `#[non_exhaustive]`, so adding a variant is additive.
2. Attach it at the construction site with `Diagnostic::with_code`, which
   chains off `Diagnostic::error` or `Diagnostic::warning` (all three are
   inherent methods on `Diagnostic` in the same file).

`Diagnostic` is itself `#[non_exhaustive]`, so struct-literal
construction is impossible outside `reify-core`: those three constructors are
the whole shape space, which is what makes the PDIAG scan tractable.

### Assert on codes, not on message text

Tests match `DiagnosticCode` identity, not message substrings (tasks 2255 and
3416 flipped the existing substring assertions). A new consumer that matches on
message text where a code should exist is an INV-SF-6 violation even if the
emission itself is coded.

---

## 3. "PDIAG says my diff is RED"

`reify-audit --pattern PDIAG` maintains a per-file ratchet over code-less
`Diagnostic::error` / `Diagnostic::warning` construction sites. The baseline is
`crates/reify-audit/pdiag-baseline.txt`; counts may only **decrease**. A file
whose live count exceeds its baseline row — or that has sites and no row at all
— is a High finding and fails the gate.

Migration of the pre-existing backlog is **opportunistic**, per PRD §6 decision
3: enforcement is for *new* sites. You are not asked to fix a file you merely
touched.

**The finding names the lines.** A High summary lists the 1-based line of every
code-less site the scan found in that file (a dozen at most, with the rest
elided as `(+N more)`) — for example `… 3 code-less
Diagnostic::error/warning site(s) at lines 118, 204, 511, baseline allows 2`.
The detector compares *counts*, so it cannot know which constructor you added:
intersect that list with your own diff. The Medium advisories carry no lines,
because their remedy is regeneration rather than an edit at a site.

Three legitimate remedies for a site you are responsible for, in preference
order — plus (d) and (e), for a file that merely moved and for a manifest that
moved under the detector itself:

### (a) Attach a code — the default

Follow §2. This is the right answer for essentially every new Warning/Error.

**The detector's reach is bounded — keep the attachment close.** PDIAG is a
line scanner, not an AST pass: it accepts a `.with_code(` on the constructor
line itself, or on any of the next **15 non-comment lines** below it. That
window covers 100% of the sites in the tree today, but with no headroom — the
widest landed constructor-to-`.with_code(` gap is exactly 15. So a genuinely
coded diagnostic whose attachment lands 16+ non-comment lines below its
constructor is counted code-less and turns your diff RED even though you did
remedy (a). It is the main direction in which the detector is not permissive;
`crates/reify-audit/src/pdiag.rs`'s residual-imprecision list names the two
others — the probe's second bound, immediately below, and a rarer
comment-masking case that takes the same escape.
If it happens: move the `.with_code(` up the chain (nearly always possible —
it is a builder method, and ordering among `.with_*` calls is free), or take
remedy (b) with that as the stated reason. The window is not widened to buy
headroom because widening is not free: past 15, the extra reach stops finding
own-chain attachments and starts letting an *unrelated* neighbouring
constructor's `.with_code(` mark this site coded, which silently retires real
code-less sites from the gate. `PDIAG_CODE_WINDOW` in
`crates/reify-audit/src/pdiag.rs` is the canonical value; appendix A below
carries the measurements behind it.

**The probe also stops at another constructor — but only across a statement
end.** Distance is not the only bound: the scan terminates at the first
`Diagnostic::error` / `Diagnostic::warning` that is separated from your site by
a `;`. Without that bound, adding a code-less diagnostic directly above — or to
the left of — an already-coded one was silently absorbed by the neighbour's
code and never reached the gate at all. A `;` alone does not stop the scan and
another constructor alone does not stop it; only the pair does, precisely so
the common `let d = if bad { …error(m) } else { …warning(m) };` /
`d.with_code(code)` shape, whose code lands in a LATER statement, keeps
reading as coded.

The cost is one narrow false RED: an **interleaved binding**, where a second
constructor is both bound and coded in between your site and your site's own
code —

```rust
let a = Diagnostic::error(m);                 // reads code-less: `b`'s anchor
let b = Diagnostic::warning(m2).with_code(c); // sits past the `;` above
a.with_code(c2);
```

No such shape exists in the swept corpus. If you write one, the fixes are, in
order: attach the code inside the constructor's own statement (`let a =
Diagnostic::error(m).with_code(c2);`), which is better code anyway; reorder so
nothing intervenes; or take remedy (b) with the interleaving as the stated
reason. Appendix A carries the measurement that fixed this bound.

### (b) Escape the site — only when code-less is deliberate

Add a trailing `// pdiag:allow — reason` on the construction site, or on a line
below it within the site's chain. Only the substring `pdiag:allow` is
load-bearing; the reason prose is for humans, and it is not optional in review
even though the detector does not parse it. This mirrors `ptodo:allow` exactly.

**One escape covers exactly one site.** The escape is forward-scoped and stops
at the next construction site, so it can never reach backwards over the site
above it — that bound is what stops a new code-less diagnostic from silently
inheriting somebody else's reviewed opt-out. A run of code-less constructors
therefore needs an escape on each; if that reads as noise, it is the honest
signal that (a) is the better remedy.

The archetype of a legitimate escape is
`crates/reify-stdlib/src/dfm.rs:174-200`: the DFM rules deliberately encode the
PRD's diagnostic-code naming in an `{I,W,E}_DFM_*` **message prefix** across a
severity-parameterized constructor, where the severity — and therefore the
prefix — is chosen at runtime from the rule's declared tag. Those sites are
documented as code-less by design and are baseline entries, not defects.

"I didn't feel like minting a code" is not a reason. If the condition is worth
diagnosing, it is worth naming.

### (c) Shrink the baseline — when you fixed something

If you coded up sites that were previously code-less, the file's live count
drops below its baseline row and PDIAG reports a Medium `baseline-stale`
advisory (exit-neutral — an opportunistic fix must never turn a diff RED).
Regenerate the manifest **in the same commit** as the fix:

```
cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \
  > crates/reify-audit/pdiag-baseline.txt
```

The generator runs the detector's own scan, so it is the single source of truth
for site counts. Never hand-edit a count, and never re-derive counts in shell.

### (d) You moved or renamed a file — regenerate

A pure move, rename or module split is the one High finding that (a)/(b)/(c)
do not answer. The ratchet keys on PATH, so relocating a baselined file
produces two findings at once:

- a **High `pdiag-ratchet`** at the NEW path — sites, no row, so it reads as
  "new to the baseline" — carrying every pre-existing site the file has always
  had. Moving `crates/reify-eval/src/engine_build.rs` reports 48 of them;
  `geometry_ops.rs`, 138.
- a **Medium `pdiag-baseline-stale`** orphan-row advisory at the OLD path.

Attaching codes to sites you did not write is not the fix, and neither is
opting all of them out one by one. Regenerate, **in the same commit as the
move**:

```
cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \
  > crates/reify-audit/pdiag-baseline.txt
```

This is the first of the two exceptions to "regenerating is NOT a
remediation" — (e) is the other — and it is a narrow one: nothing is
re-blessed, because the same sites were already blessed under the old path. **The review check is that the two counts match** — the new
row's count equals the count the orphaned row allowed. If the new count is
higher, the diff added code-less sites on top of the move and those go back to
(a)/(b). When that is hard to see in review, split the move and the edits into
separate commits.

### (e) The detector itself got stricter — regenerate

A manifest row can also rise because the **scanner** changed, with the scanned
file untouched. When a bound is tightened, sites that were previously hidden by
an imprecision become visible, the live count exceeds the baseline row, and the
file goes High — for a file nobody in the diff edited.

This is the second and last shape where regeneration is the whole fix, and,
like (d), it is **not** the ratchet being re-blessed away. Nothing was allowed that was not already there:
the sites always existed, and what changed is that the detector can now see
them. Attaching codes to them is a different task, usually in a different
crate, and blocking the detector fix on it is backwards.

The review checks are the ones that make it falsifiable:

- the diff to `pdiag-baseline.txt` is **exactly** the rows the detector change
  explains, and no others;
- each newly surfaced site is inspected and named in the commit message or in
  appendix A;
- the manifest is regenerated **in the same commit as the detector change**, or
  the gate is RED at every commit in between.

Worked example: bounding the code probe (2026-09, task #5887) raised
`crates/reify-eval/src/geometry_ops.rs` from 137 to 138 and moved no other ROW
— which is exactly why the first check above is necessary but not sufficient.
It surfaced FOUR sites, not one: `modal_ops.rs` and `reify-stdlib/src/geometry.rs`
each carried a pre-existing stale row with one of headroom, which absorbed
their new site without moving a row, so a reviewer reading only the manifest
diff sees two of the four movers not at all. The census diff does show them,
which is why the second check names sites rather than rows. All four are
inspected one by one in appendix A — two carry an in-source note declaring them
code-less *by class*, so coding them would have been wrong, not merely out of
scope.

### When the finding is not about your diff

Two High findings mean the ratchet could not run at all, rather than that you
added a site. Neither is remedied by (a)/(b)/(c)/(d):

- **`pdiag-baseline-unreadable`** — the manifest does not parse. The summary
  names the offending line. Regenerate as in (c); never hand-repair a row.
- **`pdiag-census-empty`** — `git ls-files` returned no swept files while the
  manifest still holds rows. Almost always the run was not inside the git
  worktree, or `git` itself failed there. Fix the invocation; regenerating
  against a broken census would wipe the manifest. The generator refuses that
  wipe rather than relying on this warning: `pdiag-baseline-gen` exits **3**
  and writes nothing to stdout when its own census reaches zero swept files,
  so the redirect in (c) cannot truncate the baseline to its header. (The
  shell truncates the target *before* the process starts, so an exit-0
  header-only render would already have been the damage.)

Both are deliberately High rather than advisory. The ratchet has two inputs —
the manifest and the census — and when either goes missing *wholesale* the
comparison is vacuous: an empty census makes every row look like a deleted
file, which would otherwise read as an exit-0 all-clear from a run that
scanned nothing. A green PDIAG has to mean the detector looked.

### Regeneration is tree-bound

A regenerated manifest is valid for **exactly the tree it was run against**.
Any subsequent rebase, merge, amend or cherry-pick — including one the merge
queue performs on your behalf — produces a different tree and silently
invalidates it. This is not an ordering nicety: it is how this detector's own
"final bootstrap reconciliation" shipped a stale row. That commit regenerated
the census and committed it, and was then replayed onto a newer base; the tree
that was measured and the tree that was committed were no longer the same one.

So "regenerate last" is not a sufficient rule — an agent that rebases after
regenerating still believes it regenerated last. The checkable form is:
**re-run the census against the committed tree**, after the commit and after
any replay, and require an empty diff.

```
git status --porcelain    # must be empty
cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . \
  | diff -u crates/reify-audit/pdiag-baseline.txt -
```

The first command establishes that the tree you are measuring is the tree you
are shipping; the second is the proof. A green captured *before* the commit, or
before a rebase, says nothing about what lands.

The hazard is not confined to files your branch touches, because the manifest
stores **absolute** per-file counts rather than a delta against main. Every row
that drifted under this detector's own branch named a file outside its diff:
`modal_ops.rs` 18 → 19 → 21 → 25, `geometry_ops.rs` 134 → 136 → 138,
`auto_type_param_phase.rs` 1 → 2, `elastic_static.rs` 7 → 12.

This is a **bootstrap** hazard, and it is self-limiting. It exists only while
PDIAG is not yet enforced on `main`: once it is, `main` cannot add a code-less
site to a baselined file without going red itself, which closes the drift
source at its origin.

### Scope — what PDIAG does not scan

The detector sweeps `crates/*/src/**.rs` and `gui/src-tauri/src/**.rs` only,
minus `crates/reify-audit/` (self-match: the detector's own doc-comments carry
literal constructor tokens) and `crates/reify-test-support/`, minus any path
with a `tests/` segment or a `tests.rs` / `*_tests.rs` file name, and minus
`#[cfg(test)]` module bodies. INV-SF-6 governs *emitted* diagnostics; test
scaffolding that fabricates a `Diagnostic` to assert on is out of scope by
construction, not by exemption.

---

## Appendix A — corpus measurements behind the detector (snapshot)

**Snapshot, not a claim about the current tree.** Each subsection below was
measured once, on the tree as it stood at its stated date — PDIAG's own build
(task #5405, 2026-08/09) unless the heading says otherwise. These figures
justify design choices that are now fixed in code; nothing re-derives them, and
nothing should be taken as describing today's corpus. The committed
`crates/reify-audit/pdiag-baseline.txt` is the only authority on current counts.
Re-measure before citing any of it in a new argument. Figures from different
subsections are **not comparable** — the corpus moved between them.

### Why a bounded line window rather than paren matching

Only ~16% of construction sites fit on one line — multi-line `format!` wrapping
is the norm — so a chain-matching mechanic was a real candidate. Both were run
over the whole corpus and diffed: a strict paren-depth chain scan and the
15-line window disagreed on **7 of 730** code-less sites. Two of the seven were
the strict scan being *wrong* (the `if {…} else {…}.with_code(code)`
severity-dispatch shape closes its constructor paren before the chain resumes,
so paren matching declares a coded site code-less — a false RED); the other
five sat in `#[cfg(test)]` bodies the detector excludes anyway.

### Why `PDIAG_CODE_WINDOW` is 15

Measured by regenerating the baseline at each window and reading the census the
generator prints (`<files> / <code-less sites>`):

| window | 13  | 14  | **15** | 25  | 30  |
|--------|-----|-----|--------|-----|-----|
| files  | 66  | 66  | **66** | 65  | 65  |
| sites  | 640 | 640 | **639**| 627 | 622 |

Two readings, the second load-bearing:

1. The 14 → 15 step moves exactly one site — `crates/reify-compiler/src/expr.rs`'s
   `let base_diag = Diagnostic::error(…)` / `base_diag.with_code(…)` pair — so
   15 was the widest OWN-chain offset in the tree, not an estimate. That bound
   moves with the corpus: it was 13 when the detector was first written.
2. The 15 → 25 step drops **12 further sites and a whole file**, and every one
   inspected was the unrelated-`.with_code(`-in-window imprecision, NOT a
   genuine own-chain attachment. The cleanest specimen was
   `crates/reify-compiler/src/diagnostics.rs`, whose only site —
   `lossy_real_warning`'s `Diagnostic::warning(…)`, ending its own chain at
   `.with_label(…)` — is falsely coded at window 25 by the `.with_code(`
   belonging to `dup_member_key_error`, a different function 23 lines below.
   The file loses its baseline row entirely.

So the window trades hard-gate coverage for headroom, and 15 was the largest
value that retired nothing.

### Why the code probe is bounded at a constructor beyond a statement end

*Re-measured 2026-09-21, task #5887, on the tree the bound lands on: 529 swept
files / 67 with rows / **644** code-less sites. Not comparable with the window
table above, which is a #5405-era corpus.*

The probe used to run its whole 15-line window unconditionally, so a brand-new
code-less site parked directly ABOVE an existing coded one was censused as
coded — a hole in the hard gate rather than mere imprecision, and its same-line
twin let a new site be parked to the LEFT of a coded one on one line. Two
candidate bounds were run over the full corpus and their censuses diffed.

**Rejected — terminate at a line whose trailing non-comment character is `;`.**
644 → **651** sites across 4 files. Four of the seven movers are the genuine
sites the accepted rule also finds; the other THREE are false REDs on landed,
genuinely-coded code:

| false RED | manifest row | shape |
|---|---|---|
| `crates/reify-eval/src/engine_compute.rs:166,168` | 1 → 3 | `let diagnostic = if … { Diagnostic::warning(m) } else { Diagnostic::error(m) };` then `diagnostic.with_code(…)` |
| `crates/reify-compiler/src/expr.rs:3314` | 64 → 65 | `let base_diag = Diagnostic::error(…)` / `.with_label(…);` then `base_diag.with_code(…)` in a later statement |

All three BIND the constructor to a variable and attach the code in a LATER
statement, so a bare statement terminator cuts exactly between the site and its
own code — the same failure class that killed paren-depth matching. Worse,
`expr.rs:3314` is the very site the window table above names as the 14 → 15
step, so this rule would have invalidated `PDIAG_CODE_WINDOW`'s own
justification.

**Accepted — terminate at the first CONSTRUCTOR separated from this site by a
statement terminator.** `;` alone never bounds; an anchor alone never bounds
(that is `escape_in_window`'s bound, and it reds the
`if {…} else {…}.with_code(code)` dispatch); the ordered pair `; … <anchor>`
does. It is exactly right because a `.with_code(` belonging to a DIFFERENT
constructor is necessarily preceded by that constructor's own anchor, while a
code attached to THIS site through a variable in a later statement has no
intervening anchor.

644 → **648** sites: four movers across three files, **zero** false REDs,
**zero** newly exempt.

| site | why it is genuinely code-less |
|---|---|
| `crates/reify-eval/src/geometry_ops.rs:381` | non-finite `Length` value-domain verdict |
| `crates/reify-eval/src/geometry_ops.rs:692` | non-finite `Angle`, carrying an in-source "CODE-LESS BY CLASS, not by omission" note |
| `crates/reify-eval/src/modal_ops.rs:556` | `W_ModalConvergence`, a real code-less warning: nothing on its chain codes it, and it was absorbed by `ShiftSkippedModes`'s `.with_code(` at :589 — 33 lines below, but reachable because the 18 intervening comment lines cost no window budget |
| `crates/reify-stdlib/src/geometry.rs:1987` | a real code-less warning, absorbed by the coded `affine_translate` error below it |

Both negative controls held: `fea_diagnostics.rs:48-53` stays coded, and the
two binding shapes above stay coded. The same-line half moved nothing — the
corpus carries zero such deltas — so it was landed on the unit tests and the
`scenario07_code_absorption.rs` fixture rather than on a census movement.

Because all four movers are genuine, the correct action was to re-bless them —
but only ONE manifest row moved, and the gap matters to §3(e)'s first review
check. `geometry_ops.rs` gained two sites against a row carrying one of
headroom, so it rose 137 → 138. `modal_ops.rs` (row 25, live 24) and
`reify-stdlib/src/geometry.rs` (row 2, live 1) each carried a pre-existing
STALE row with exactly one of headroom, which silently absorbed their new site:
live rose, the row did not, and the manifest diff shows nothing. So "the diff
to `pdiag-baseline.txt` is exactly the rows the detector change explains" is a
necessary check, not a sufficient one — a census diff is what makes every mover
visible. See §3(e).

The figures above are a re-measurement, and the first record of them was wrong
in a way worth naming: it recorded the starting census as 645 rather than 644
and named only three movers. Both follow from ONE omitted site — `modal_ops.rs:556`,
which is absorbed by a stale row and so leaves no manifest trace — and, counted
off that inflated start, +7 read as +6 for the rejected rule and +4 as +3 for
the accepted one. Derive the census and the mover list from the SAME pair of
generator runs; an endpoint that looks right (651 here did) does not confirm
the start.

### Comment-mask incidence

The `* ` block-comment-continuation rule also matches a wrapped arithmetic
continuation (`let x = a\n    * b\n    + c;`), masking such a line as
comment-only: **19** such lines existed in the swept corpus
(`shell_assembly.rs`, `modal/transient.rs`, …), none within 30 lines below a
constructor, so the live census was unaffected.

The two false-RED routes the module header enumerated *at this snapshot* were
both latent rather than live: no swept file ended at non-zero block-comment
depth, and no masked line carried a `.with_code(`. (The header lists three
today; the probe-bound route was added in 2026-09 — see the subsection above.)

### Re-measuring

Change the constant in `crates/reify-audit/src/pdiag.rs`, then for each
candidate window run the generator and read its stderr census line:

```
cargo run -p reify-audit --bin pdiag-baseline-gen -- --project-root . > /dev/null
```

Do not commit a manifest generated at a non-canonical window.

To measure a DETECTOR change instead of a constant, generate a manifest with
each version of `pdiag.rs` — the old one is `git show <base>:crates/reify-audit/src/pdiag.rs`
— and diff the two files. The diff gives per-file counts, and the stderr census
line gives the totals; take both from the same pair of runs.

That diff names files, not sites. To name the moved SITES, drop the affected
rows from `pdiag-baseline.txt` so every site in them is reported, and run
`reify-audit --pattern PDIAG --runs-db <tmp> --no-jcodemunch`: each High
summary lists its file's code-less lines, and the set difference between the
two runs is the mover list. Raise `PDIAG_SUMMARY_LINE_CAP` while doing so —
summaries elide past 12 lines, and a mover past the cut is invisible. Both
mutations are measurement scaffolding; revert them before committing.

---

## Related

- `docs/prds/v0_6/eradicate-silent-undef.md` — §3 Leg C (the detector + this
  doc), §6 decisions 3/7/8, §7 contract, §8 boundary row 8.
- `docs/legibility/design-invariants.md` — §INV-SF-2, §INV-SF-6.
- `docs/prds/reify-audit-ptodo-detector.md` §8 — the sibling detector whose
  escape-hatch and baseline conventions PDIAG mirrors.

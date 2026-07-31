# Diagnostic Severity Policy

**Status:** Normative — cited verbatim in the `reify-audit --pattern PDIAG` failure message
**Date:** 2026-07-28
**Source:** Task 5405 (PRD `docs/prds/v0_6/eradicate-silent-undef.md` task η, §3 Leg C item 2, §7 "PDIAG baseline")
**Invariants realized:** `INV-SF-2 error-severity-exits-nonzero` (`docs/legibility/design-invariants.md:47`), `INV-SF-6 diagnostics-carry-codes` (`:154`)

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

The corollary (`design-invariants.md:47`) is the part that governs severity
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
`crates/reify-core/src/diagnostics.rs:3355`:

> The PRD "debug" tier is realized as `Severity::Info` because
> `reify_core::Severity` has no `Debug` variant; adding one would require
> cross-cutting changes well outside this task's scope.

Two landed consequences worth knowing before you pick `Info`:

- **Standing advisories are `Info`, deduped.** `FlexureFatigueCheckMissing`
  (`diagnostics.rs:2316-2326`) is emitted once per eval session because it
  describes a surface-level gap, not a per-instance defect.
- **`Info` can be upgrade-exempt.** `HexWedgeForceTet` (`:3355`) and
  `HexWedgePromoted` (`:3293`) are documented as always `Info` and never
  upgraded, while their sibling `HexWedgeInvalidSweepGeometry` (`:3331`) is
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

**Rule** (`design-invariants.md:154`):

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
   The enum is `#[non_exhaustive]` (`:155`), so adding a variant is additive.
2. Attach it at the construction site with `.with_code(...)`
   (`:3915`), which chains off `Diagnostic::error` (`:3875`) or
   `Diagnostic::warning` (`:3885`).

`Diagnostic` is itself `#[non_exhaustive]` (`:3819`), so struct-literal
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

Three legitimate remedies, in preference order:

### (a) Attach a code — the default

Follow §2. This is the right answer for essentially every new Warning/Error.

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

### When the finding is not about your diff

Two High findings mean the ratchet could not run at all, rather than that you
added a site. Neither is remedied by (a)/(b)/(c):

- **`pdiag-baseline-unreadable`** — the manifest does not parse. The summary
  names the offending line. Regenerate as in (c); never hand-repair a row.
- **`pdiag-census-empty`** — `git ls-files` returned no swept files while the
  manifest still holds rows. Almost always the run was not inside the git
  worktree, or `git` itself failed there. Fix the invocation; regenerating
  against a broken census would wipe the manifest.

Both are deliberately High rather than advisory. The ratchet has two inputs —
the manifest and the census — and when either goes missing *wholesale* the
comparison is vacuous: an empty census makes every row look like a deleted
file, which would otherwise read as an exit-0 all-clear from a run that
scanned nothing. A green PDIAG has to mean the detector looked.

### Scope — what PDIAG does not scan

The detector sweeps `crates/*/src/**.rs` and `gui/src-tauri/src/**.rs` only,
minus `crates/reify-audit/` (self-match: the detector's own doc-comments carry
literal constructor tokens) and `crates/reify-test-support/`, minus any path
with a `tests/` segment or a `tests.rs` / `*_tests.rs` file name, and minus
`#[cfg(test)]` module bodies. INV-SF-6 governs *emitted* diagnostics; test
scaffolding that fabricates a `Diagnostic` to assert on is out of scope by
construction, not by exemption.

---

## Related

- `docs/prds/v0_6/eradicate-silent-undef.md` — §3 Leg C (the detector + this
  doc), §6 decisions 3/7/8, §7 contract, §8 boundary row 8.
- `docs/legibility/design-invariants.md` — INV-SF-2 (`:47`), INV-SF-6 (`:154`).
- `docs/prds/reify-audit-ptodo-detector.md` §8 — the sibling detector whose
  escape-hatch and baseline conventions PDIAG mirrors.

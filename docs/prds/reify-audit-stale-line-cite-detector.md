# PRD: reify-audit PLINECITE — stale hard line-cite detector (scope decision record)

**Status:** active — scope decided and built by #6935, which coalesced #5872 and #5874; spelled lead-ins added by #9188.

Measurements and code anchors taken at main 12d47d8429 (2026-10-11); counts are
dated, not live. The `<file>:<line>` examples quoted below are part of that dated
record.

## 1. Problem

Comments that cite source by `<file>:<line>` rot silently. Any edit above the
target moves it, nothing validates the number, and the comment then points a
reader at unrelated code. The repo has rediscovered this repeatedly: #3656,
#5837, #5854, #5856, #5861, #6064, #6990, #7056, #7403, #7786, #8073, #8305,
#8316. Each rediscovery ended in a manual sweep and a restated convention:
cite the enclosing symbol, never the line (canonical memory topic
`cite-code-by-symbol-not-file-line`).

A convention alone has measurably not held:

- **stdlib regressed.** #5856 swept `crates/reify-compiler/stdlib/*.ri` to
  about zero comment cites. At 12d47d8429 there are 7, in 3 files
  (`materials_fea.ri` 5, `materials_mechanical.ri` 1, `trajectory.ri` 1).
- **crates/*/src grew.** Using #5874's exact command, comment-leading lines
  carrying a `.rs:`/`.ri:` cite under `crates/*/src` went from 716 lines in 163
  files at 587109618a (2026-07-30) to 918 lines in 190 files at 12d47d8429:
  +202 in about ten weeks, with no detector.

## 2. Decisions

| Question | Decision | Section |
|---|---|---|
| Build a detector? | Yes. The convention has not held (§1). | — |
| Design | Syntactic: report every hard cite, resolve none. | §3 |
| Grammar | Comment-leading lines only; `.rs`/`.ri` targets; colon and measured spelled lead-ins; decline-only. | §4 |
| Scope | `crates/reify-compiler/stdlib/**.ri` plus `crates/<crate>/src/**.rs`, minus `crates/reify-audit/`. | §5 |
| `docs/` | Excluded permanently. | §5 |
| Residual | A ratchet ledger, `crates/reify-audit/plinecite-baseline.txt`, settled by `check()`. | §6 |
| Severity | Medium, report-only. Never moves the exit code. | §7 |
| Consumer | Opt-in `/audit --pattern PLINECITE`. Not in the default sweep, not in the docs-truth sweep. | §8 |
| Escape hatch | Same-line `plinecite:allow — <reason>`; a reasonless marker is itself reported. | §4 |
| Merge-time gate | Deferred to #8822 (S24 ο), which subsumed the agent-followup #8509. | §7 |

## 3. Syntactic, not semantic

A semantic checker would resolve each cite's path and line and report only the
ones that no longer hold. It was rejected:

- **Path/range resolution passes the motivating defect.** `dimension.rs:1259`
  existed and was in range; it pointed into an unrelated assert. On docs, the
  same check scored `kernel-seam-contracts.md` 6/6 clean while 37 of its 47
  anchors had drifted. "The line exists" is not "the line is the one meant".
- **Most cites cannot be resolved at all.** They name a bare filename
  (`cache.rs`, `lib.rs`, `kernel.rs`), and several crates ship each of those.
- **Anchor-token resolution needs an ad-hoc parser** of which symbol the
  surrounding prose means. That is matcher hardening, the path 593aaeb4f0
  rejected when it deleted #5856's guard: each new spelling evades the next
  matcher.
- **Syntactic matches the remedy.** The repo keeps converging on de-numbering
  (#2450, #5837, 7018e270a5, the canonical memory topic above). A hard cite is
  the defect whether or not it is stale today, because nothing will tell
  anyone when it goes stale. Its only "false positives" are deliberate dated
  cites, and those take the escape hatch.

## 4. Grammar (normative)

- A **comment line** is a line whose trimmed start begins `//`. That covers
  `//`, `///` and `//!`.
- A **cite** is a stem of bytes `[A-Za-z0-9_./-]` ending in a word byte, then
  `.rs` or `.ri`, then an optional ` esc-<digits and hyphens>` anchor, then one
  lead-in from the closed set `:`, `:~`, ` ~`, ` (~`, ` line `, ` lines `
  (`plinecite::LINE_NUMBER_LEADS`), then a digit. Each lead-in is matched
  literally, so the digit must follow it directly.
- The **reported token** is verbatim, from the stem's start through the first
  digit run: `cache.rs:617-621` is reported as `cache.rs:617`, and
  `entity.rs (~2726-2790)` as `entity.rs (~2726`. Re-spelling a cite is
  therefore new debt, exactly as re-numbering is (§6).
- **The lead-in set is closed.** A lead-in joins it only on a measured live
  instance in scope. #9188 admitted the five spelled ones on the instances
  §5 records.
- A bare `esc-<id>` anchor with no line number is legal and is not a cite.
- A same-line `plinecite:allow — <reason>` exempts that line. The marker
  grammar is the crate's shared one (`scan_util::allow_marker_body`). A marker
  with no reason exempts nothing and is reported as `allow-missing-reason`.

**Out of grammar, by design:**

- the prefix prose form, `line N of foo.rs`;
- spellings with no measured live instance: `foo.rs, line N`, `foo.rs (line N`,
  `foo.rs line ~N`, `foo.rs L12`, `foo.rs ~ N`;
- a bare parenthesised number, `foo.rs (N)`, which the corpus uses for counts
  (`beam.rs (2)`), not lines;
- a cite whose digits wrap onto the next line;
- string literals, including the 15 assertion-message cites measured in scope;
- block comments and trailing comments, of which 0 were measured in scope;
- targets other than `.rs`/`.ri`; the measured `.js`, `.h` and `.cpp` cites
  point at external code.

PLINECITE **reports**; it never **certifies** a file free of line cites. A
clean run means no in-grammar cite was found, nothing more. Hardening the
matcher to close these gaps is rejected for the reason 593aaeb4f0 gives.

The spelled lead-ins are not that hardening. 593aaeb4f0 rejected an
open-ended matcher behind a gate, where each new spelling evades the next
matcher. The lead-in set is closed, each member is admitted on a measured live
instance, and the lane stays report-only. #9188 admitted the set because,
without it, S23's ledger burn-down (`docs/prds/v0_6/prose-rehoming-and-symbol-citations.md`
D1, whose done-state is "no ledger row under the leaf's paths") could reach
done with these cites still in place.

## 5. Scope

`plinecite::in_scope` is the single boundary.

| Tree | In? | Why |
|---|---|---|
| `crates/reify-compiler/stdlib/**.ri` | in | The v1 pilot. Shipped stdlib that users read. |
| `crates/<crate>/src/**.rs` | in | #5874's widening: where the growth in §1 is. |
| `crates/reify-audit/` | out | The detector's own docs and fixtures would self-match, and its 48 hits are fixture-provenance records of the form "real shape: X:N". This mirrors `ptodo::ALLOWLIST_PREFIXES`. |
| `docs/` | out, permanently | Forensic point-in-time records. De-staling one falsifies it. |
| `crates/*/tests/`, benches, `build.rs` | out | Tests carry deliberate dated-evidence cites, such as the Citation-convention header in `objective_seed_parking_triage.rs`. |
| `crates/*/src/**.ri` | out | The src arm is `.rs` only. |
| `examples/`, prd-gate fixtures, other `.ri` | out | Non-stdlib `.ri` de-numbering is owned by #8305. |
| `scripts/`, `gui/` | out | #8822 widens the scope to `scripts/`, `hooks/` and `gui/` (it subsumed #7302). |

**Widening re-derived.** The widening was specified against a design that did
not yet exist. Re-checked with the detector in hand, the design is the one
#5874 assumed: a syntactic scan that is cheap over the whole tree and needs no
per-crate knowledge. So the widening is still warranted. It also catches cites
into trees that no longer exist, such as the deleted `crates/reify-types`.

**Measured surface at 12d47d8429:**

| Measure | Value |
|---|---|
| stdlib comment cites | 7 in 3 files |
| `crates/*/src` comment-leading cite lines, all crates | 918 in 190 files |
| … excluding `crates/reify-audit/` | 870 lines, 885 cites, 737 distinct (path, cite) pairs, 185 files |
| Scanned corpus | 583 files, 35 MB |
| Cited extensions | rs 828, ri 57, js 30, h 16, cpp 11, md 4 |
| Trailing-comment and block-comment cites | 0 |
| Assertion-message string cites | 15 |
| `.ri esc-<id>:<n>` cites anywhere | 0; #5861 removed the last one, and the form stays in the grammar |

**Measured at landing** (the #6935 branch over main 4a0d1687df, 2026-10-14):

| Measure | Value |
|---|---|
| Seeded ledger rows | 742: 735 `crates/*/src` (path, cite) pairs, 7 stdlib pairs |
| Citing files | 186 (183 `crates/*/src`, 3 stdlib), in 22 crates |
| Re-seed after #8528 de-numbered 2 cites | shrink-only: 2 rows dropped, 0 admitted |
| `reify-audit --pattern PLINECITE`, real tree | 0 findings, exit 0 |
| Shrink-only regeneration over the seeded ledger | byte-identical |

**Measured at #9188** (main 8113685867, 2026-10-24), the lead-in widening:

| Measure | Value |
|---|---|
| Rows admitted by the spelled lead-ins | 28: ` ~` 16, `:~` 7, ` (~` 2, ` lines ` 2, ` line ` 1 (19 in reify-compiler) |
| … sample tokens | `entity.rs (~2145`, `entity.rs:~631`, `graph.rs ~429`, `lib.rs line 444`, `kernel.rs lines 178` |
| False positives | 1, `parts.ri line 0` in reify-lsp `server.rs` (an LSP position a test asserts), settled by `plinecite:allow` |
| Carried-over colon rows, un-ledgered on the base after a file move | 2: kernel-occt `lib.rs` → `op_args.rs`, reify-lsp `references.rs` → `references/cross_file/tests.rs` |
| Stale rows dropped, the same set any regeneration on the base drops | 32 |
| Ledger rows after `--admit-new` | 737 (the same run with the base's grammar writes 709) |
| Shrink-only regeneration over the written ledger | byte-identical; 0 stale, 0 new debt |

## 6. Residual ledger

About 740 pre-existing cites (exact, dated counts: §5's landing table) span
about 185 files in 22 crates. Sweeping them is per-site symbol research across many
modules, out of scope here and lock-hostile. Reporting them raw would bury
every new cite under hundreds of old ones. So the residual is a ratchet
ledger, `crates/reify-audit/plinecite-baseline.txt`:

- **Row grammar:** `<path> :: <cite>`, one per line, for example
  `crates/reify-eval/src/cache.rs :: engine_eval.rs:3884`. The separator is
  the first ` :: `, because a path has no spaces and a cite can (an esc anchor,
  ` ~`, ` line `).
- **Why (path, cite), not a per-file count.** The cite's target number is part
  of the row, so re-pointing a stale number is reported as new debt.
  Re-numbering is not the fix; naming the symbol is. Under a per-file count, a
  swap of one cite for another would be invisible.
- **Why the citing line's own number is not in the row.** The row survives
  code motion: adding a line above the comment does not churn the ledger.
- **Shrink-only generator.** `plinecite-baseline-gen` only drops stale rows by
  default; `--admit-new` is the only way to grow the ledger, and every row it
  adds must be justified in review.
- **Settlement.** `check()` reports new debt, stale rows
  (`stale-baseline-entry`), reasonless allow markers, and an empty scope. An
  untracked ledger is inert. A degenerate tree, where no tracked file is in
  scope, is refused, so a failed `git ls-files` can never render an empty
  ledger or a vacuous clean run.
- **The set algebra is shared.** Kept, new and stale are
  `crate::ratchet_ledger::Ledger`, the same type PDOCCOVER settles with.
- **No real-tree count or freshness is pinned in tests.** The corpus is debt
  meant to shrink to zero, so a floor is a time bomb. A freshness assertion
  would make any comment edit in any crate red this crate's tests at merge,
  with misattributed blame: the de-facto hard gate §7 rejects, and the class
  `baseline_report_freshness.rs` already records. The committed ledger is
  pinned only to be tracked and in generated form.

## 7. Severity and enforcement

Every PLINECITE finding is Medium. The CLI's exit code counts High findings
only, so PLINECITE never moves it, even when a gate selects the pattern.

**Why no gate now.** 593aaeb4f0 removed a *gating* substring line-cite test.
The grammar is decline-only and blind to wrapped and prose forms, so a gate
pressures authors into out-of-grammar spellings such as `line 12 of foo.rs`,
which are worse than the cite they replace because nothing reports them. The
size of that displacement needs measuring before anything gates on this
grammar.

**The shape-appropriate future gate is merge-time.** A hard cite is a finding
in the very diff that writes it, so the place to stop one is that diff's merge.
Deciding and building that, either as a ledger-ratchet gate or as a
Medium-to-High flip, is #8822.

## 8. Consumers

- **`/audit --pattern PLINECITE`**, opt-in. Routing, follow-up grouping and
  dedupe live in `.claude/skills/audit/references/severity-routing.md`.
- **Not the default sweep.** At about 20 new cites a week, a default-sweep
  slot would turn every bare `/audit` into follow-up-task traffic. Every other
  structural lane is opt-in too.
- **Not the docs-truth sweep.** `docs/notes/docs-truth-sweep.md` admits only
  detectors whose findings arise *outside* the doc's own diff. A syntactic
  cite is a finding in the diff that writes it, and PLINECITE's corpus is
  source comments with `docs/` excluded. It is merge-gate-shaped, not
  sweep-shaped.

## 9. Relation to siblings

- **PCITE** (#6233, landed by #6931) checks that a capability manifest's
  `grep:` evidence names a symbol that exists. PLINECITE checks that source
  comments do not cite by line. Same report-only shape and finding
  conventions.
- **PPRDSTATUS** (#6346, landed by #6932) checks PRD status prose against the
  task graph. Disjoint corpus.
- **The docs-truth sweep** (#6347) aggregates prose-doc detectors. PLINECITE is
  excluded on its membership criterion (§8).
- **#7302**, the semantic `path:LINE` guard, was cancelled. #8822 subsumes it
  with a syntactic widening rather than the semantic design §3 rejects.
- **#8305** de-numbers the stdlib residual and non-example `.ri` files. Its
  stdlib rows leave PLINECITE's ledger as stale rows when it lands.

## 10. Follow-ups

- #8822: merge-time ratchet gate, widened scope and cite targets, `--rename`.
- The ledger shrinks as cites are de-numbered; regenerate it with the default,
  shrink-only generator.

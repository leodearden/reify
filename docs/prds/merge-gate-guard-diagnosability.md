# PRD: kLOC-cap guard diagnosability + at-source trigger

**Date:** 2026-07-22 · **Status:** approved for decomposition · version-agnostic
(root `docs/prds/`). **Approach: B** (two point-hardenings of an existing,
landed guard — no new mechanism, no new seam).
**Provenance:** reify task-5260 stranding RCA. During three merge-gate
failures of `tests/infra/test_harness_kloc_cap.sh`, no investigation — four
`unblock_auto` dry-runs and two commissioned forensics — could read *which
file* violated the ratchet from the archived verify logs: Section 5 captures
the live scan's structured `HARNESS_KLOC_CAP FAIL … file=<path>
reason=<reason>` lines (rule (c) exists precisely for this) but swallows them,
printing only the assert verdict. Separately, introducers (5213, 5053) only
learn they forgot the same-diff baseline row at the *merge gate*, 40+ min in,
because the branch-scope trigger doesn't key on `crates/*/tests/*.rs`
additions.

## Consumer + observable surface (G1)

- **R1** — consumer: every merge-gate failure reader: the DF orchestrator's
  dry-run investigator and disposition classifier (the DF sibling PRD's
  `foreign_drift` leaf parses exactly these lines — grammar owner is THIS
  guard, rule (c), unchanged), and the human reading
  `data/verify-logs/<task>/attempt-*.test-*.log`.
- **R2** — consumer: the task-branch verify path (`scripts/verify.sh`
  branch scope) — introducers of new standalone `tests/*.rs` files see the
  violation in their own workflow verify, pre-merge-queue, in seconds (the
  guard is hermetic: static file reads, no cargo).

## Pre-conditions (G3 — verified)

Guard + baseline + rule-(c) grammar live on `main`
(`tests/infra/test_harness_kloc_cap.sh`, `harness-layout-baseline.manifest`);
Section 5's live-scan capture demonstrably drops the FAIL lines (07-20 logs);
branch-scope verify (`verify.sh test --scope branch --include-infra`) exists.
No grammar work, no new substrate. `grammar_confirmed: true` on both leaves.

## Resolved decisions

1. R1 emits the captured live-scan output (the structured lines, verbatim) to
   stderr/log on Section-5 assert failure — no grammar change, no new lines.
2. R2 triggers the existing guard within branch-scope verify when the branch
   diff (merge-base three-dot) **adds** a top-level `crates/*/tests/*.rs`;
   no-op otherwise. Merge-gate behavior unchanged (it already runs the guard
   wholesale). Neither leaf adds a new gate-resident test → no
   run-all-classification / wallclock-bounds registrations needed (overlay
   drift-guard rule consulted; not triggered).

## Out of scope

DF-side disposition/adoption logic (DF sibling PRDs); ratchet policy changes
(grandfather semantics unchanged); un-swallowing output of any other
tests/infra member.

## Decomposition plan

- **R1 — Section 5 offender emission.** Signal: a fixture-forced live-scan
  failure prints `HARNESS_KLOC_CAP FAIL crate=<c> file=<path>
  reason=unsanctioned-standalone` in the test's captured output (and thus in
  the archived merge-verify log); the guard's self-tests stay green.
  `metadata.complexity=simple`, files: `tests/infra/test_harness_kloc_cap.sh`.
- **R2 — branch-scope at-source trigger.** Signal: a scratch branch adding an
  un-grandfathered `crates/reify-eval/tests/zz_prd_probe.rs` fails
  branch-scope verify naming that file, in < 60s; removing the file (or adding
  the baseline row) turns it green. Files: `scripts/verify.sh` (+ the guard
  invocation seam it exposes).

Independent leaves; R1 has no dep on R2.

## Open (tactical) questions

- R2: exact hook point inside verify.sh's branch-scope path (pre-test cheap
  checks section vs test enumeration) — implementer's call.

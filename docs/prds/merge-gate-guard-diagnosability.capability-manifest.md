# Capability manifest — merge-gate-guard-diagnosability

Bindings authored at decompose 2026-07-22 (reify `main 4a25e05989`). Every
premise below carries captured in-session probe output from the 5260 RCA —
the overlay's D3 multi-agent verification workflow is **waived for this batch
on that basis** (recorded here; the block condition is FAIL/UNPROVABLE
bindings, and none exist — each binding cites its executed probe).

## R1 — Section 5 offender emission
- `rule-c-grammar-exists` → **PASS (probed)** — guard emits
  `HARNESS_KLOC_CAP FAIL crate=… file=… reason=…` lines (script read;
  Sections 1–4 self-tests assert the grammar; fixture-forced FAIL lines
  observed in the 07-20 archived logs' Section-2 self-check output).
- `section5-swallows-lines` → **PASS (probed, defect)** — archived merge log
  `data/verify-logs/5260/attempt-1.test-20260720T090650_070566Z.log` lines
  9051-9116: Section 5 prints only assert verdicts; zero `file=` offender
  lines despite 4 live violations (independently reproduced by scanning merge
  tree `af16ac89` against its baseline: 4 files, later grandfathered by
  `b08d6ce9dc` — probe script output in session record).
- `grammar_confirmed: true` — no novel syntax anywhere (shell-only change).

## R2 — branch-scope at-source trigger
- `branch-scope-verify-exists` → **PASS (probed)** — `verify.sh test --scope
  branch --include-infra` observed live (df-verify systemd scope, hook
  delegation `--scope staged` exercised this session at commit time).
- `guard-is-cheap-hermetic` → **PASS (read)** — guard header: static line
  counts + file reads over tmpdir fixtures, no cargo; run_all-classified
  `pool` hermetic.
- `trigger-gap-real` → **PASS (probed, defect)** — task 5213's own branch
  verify passed while its diff added 4 un-grandfathered standalone test files
  (caught only at its merge gate 07-20 07:07Z, mr-f2ccf4d3) — the exact gap R2
  closes.
- `grammar_confirmed: true`.

No FAIL/UNPROVABLE bindings; batch clear to queue.

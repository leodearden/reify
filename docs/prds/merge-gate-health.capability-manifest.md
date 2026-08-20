# Capability manifest — merge-gate-health (decomposed 2026-07-19)

PRD: `docs/prds/merge-gate-health.md`. Sidecar: `merge-gate-health.capability-manifest.yaml`. Program milestone: 5254.
Batch: reify **5256–5261**; dark-factory **2828–2831**. Folded into existing tasks (no new filing): W2a/W2c → DF 2821 (in-progress; its description already mandates the cross-attempt deterministic discriminator), W3b → DF 2822 (evidence appended 2026-07-19), trivial-pass red-main precheck → DF 2823 (in-progress), manifest-row mechanical gate → reify 5252 (in-progress). Config-only leaves landed directly with the 2026-07-19 config batch (operator-authorized): `verify_use_cgroup_scope: true` (W1b-reify) + `REIFY_RUN_ALL_FLAKY_LEDGER` durable path (W5a) in `dark-factory-orchestrator.yaml`. 5251 (false-premise de-flake) was cancelled independently the same evening; census evidence appended to its record.

| Leaf | Capability asserted | Evidence binding | Status |
|---|---|---|---|
| 5256 (W3a) | `requires-full-gate` exits 0 for `tests/infra/test_*.sh` | guard oracle exists on main: `scripts/verify-pipeline-guard.sh` + manifest `scripts/verify-pipeline-paths.txt`; current rc=1 hole reproduced 2026-07-19 (survey §flake-RCA) | VERIFIED (hole reproduced; fix pending) |
| 5257 (W4a) | detector distinguishes measurement vs config-constant | detector + `_wc_var_sfx` branch exist (`test_no_new_wallclock_upper_bounds.sh:60-228`); ep3 false-positive corpus preserved in survey + falsification experiment | VERIFIED |
| 5258 (W4b) | causal flock-probe barrier replaces sleep-grace | technique already in-tree (Tests 19/21A ready-file barriers); live-flake repro: 07-19 FLAKY-ledger event on 5252's gate | VERIFIED |
| 5259 (W4c) | baseline-red detectable before hostile re-run | derivative-failure census 7/7 (survey); `test_run_all_ambient_isolation.sh:344` re-invocation seam confirmed | VERIFIED |
| 5260 (W4d) | plan-capture lib reusable; marker-suffix idiom exists | `plan_capture_lib.sh` (task 4866) on main; Part-2 pre-clean suffix idiom at `test_proc_reaper.sh:331` | VERIFIED |
| 5261 (W4e+W5b) | run_all can self-check INFRA_DIR + Phase-2.5 extensible to serial bucket | Phase-2.5 + ledger mechanics on main (`run_all.sh:121-154, 323-418, 1043`); DF consumer for ledger/markers already done (DF 2358) | VERIFIED |
| DF 2828 (W1a+W1c) | lease/reset seams exist for a startup barrier + defer branch | `git_ops.py:7737` reset, `:1902-1911` fail-open, `merge_liveness.py:715` call site; 96.9% restart-window failure stat (runs.db) | VERIFIED |
| DF 2829 (W1b-DF) | cgroup-scope mechanism shipped, off only by config | `verify.py:2890-2911`; `config.py:2481` default False; reify yaml flip in config batch | VERIFIED |
| DF 2830 (W1d) | CLI span lock divergence | `cli.py:495-503` vs `git_ops.py:1890-1895` (2685's documented deliberate deferral); remote twins 5034/5187 | VERIFIED |
| DF 2831 (W2b) | ENV_TRANSIENT pattern seam exists; replay corpus archived | `_VERIFY_ENV_BROKEN_RE` (`verify_classify.py:160-163`, DF 2756); reify `data/verify-logs/{5164,5071}` outputs preserved | VERIFIED |

Numeric-floor / negative-assertion notes (G6): no numeric-accuracy claims in this batch; every "X is rejected/classified" leaf carries a replay- or fixture-observed rejection (5256 fixture branch refusal, 2831 replayed classification), not an assumed one.

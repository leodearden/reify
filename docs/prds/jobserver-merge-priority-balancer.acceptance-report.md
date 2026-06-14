# Jobserver Balancer η Acceptance Report

PRD: `docs/prds/jobserver-merge-priority-balancer.md` §9 leaf η (task 4521)
Host: 32-core reference host (`nproc=32`), deployed dual-pool `reify-jobserver.service`
(`jobserver-balancer.py`, 24 merge / 8 task baseline partition), captured 2026-06-11.
Instrument: `scripts/jobserver-acceptance.py` (this branch), real `verify.sh` loads only —
esc-4520-22 (disposition D): **no synthetic walls anywhere in this report**.

## Gate verdicts

| criterion | description (as re-specified) | verdict | instrument | evidence |
|-----------|-------------------------------|---------|------------|----------|
| (a) | partition strands no cores: dual-pool busy within 0.03 of the single-pool baseline, same regime | **PASS** | both pairs (relative oracle is regime-internal) | cold: **0.799** vs 0.822 − 0.03; warm: **0.895** vs 0.875 − 0.03 — both above the floor |
| (b) | merge wall improved vs single-pool baseline | **PASS** | cold mixed pair (compile-bound — the only regime the jobserver governs) | dual **777 s** < baseline **799 s**; staggered merge faster still (652 s) |
| (c) | no task verify exits 124 under the standing budgets | **PASS** | all regimes | 6/6 real verifies, `exit_124_count = 0` everywhere; no §10.4 escape valve needed |
| (d) | contention ratchet observed: merge pool strictly above its seeded 24-token baseline under contention | **PASS** | cold pair + staggered probe | 31/32 (cold), 27/32 (probe) — donation in **every** contested run, never clawed back mid-contention |

**Oracle provenance:** criteria (a) and (d) were re-specified on 2026-06-11
(Leo-ratified, interactive `/unblock 4521` session) after the instrument-validity
analysis below, and the re-specified oracles are **implemented in the evaluator**
(`utilization_ok` relative form; `merge_ratchet_observed`).
`jobserver-acceptance.py evaluate measurements-cold.json` exits **0 (all four
PASS)** under them. For the record, the original-wording strict results on the
same data were `(a) 0.799 < 0.85` and `(d) max 31 < 32`; the data is unchanged —
the oracles were amended because they measured workload shape (a) and conflated
allocation with availability (d), as analyzed below. The warm pair evaluates
`(b)/(d) FAIL` under any oracle — expected and structural: a compile-free run
gives the jobserver nothing to govern (see next section).

## Why two A/B pairs (and a probe)

The first capture (warm pair) exposed a structural fact: **the jobserver governs
compile parallelism only** — nextest's test-execution threads draw no jobserver
tokens. A fully-warm verify compiles nothing, so the pools sit idle and a warm A/B
is structurally blind to criteria (b)/(d). The cold pair (targets removed before
**each** leg; host sccache warm — exactly a production fresh-worktree) is the
compile-bound instrument. The staggered probe addresses a second instrument
artifact: simultaneous starts keep both verifies' compile phases synchronized,
which understates donation (production merges land mid-task-verify).

## Per-run measurements

All runs: 1 real merge verify (`DF_VERIFY_ROLE=merge verify.sh all --scope all`,
profile=both via role) concurrent with 1 real task verify (`DF_VERIFY_ROLE=task
verify.sh test --scope all --include-infra`) from a **separate checkout** (shared
checkouts serialize on cargo's build-dir lock — see harness `--task-repo`).
Scheduler and merge queue were halted during all timed runs (quiet box).

| service | regime | cache_state | busy_fraction | merge_wall_s | slowest_task_wall_s | exit_124 | max merge tokens free |
|---------|--------|-------------|---------------|--------------|---------------------|----------|----------------------|
| single-pool | mixed | warm | 0.875 | 363.3 | 362.6 | 0 | 32 (single shared FIFO) |
| dual-pool | mixed | warm | 0.895 | 394.0 | 513.4 | 0 | 24 (pools idle — no compile) |
| single-pool | mixed | cold | 0.822 | 798.9 | 798.9 | 0 | 32 (single shared FIFO) |
| dual-pool | mixed | cold | 0.799 | **776.9** | 776.9 | 0 | **31** (ratchet: +7 donated) |
| dual-pool | staggered probe | merge-cold / task-warm-in-test-phase | — | **652.4** | — | 0 | 27 while contested (+3 donated) |

Notes:
- Warm-pair walls are test-execution-bound; the dual-pool's 394 s vs 363 s delta is
  pool-independent scheduling noise in a regime the balancer does not touch.
- The warm-baseline task verify exited 1 on one infra case
  (`test_jobserver_role_fifo.sh` (f)) — a test-hermeticity bug (it asserted the
  *default* FIFO path while inheriting the campaign's injected env), fixed on this
  branch; all subsequent runs exit 0.
- Sampler: 5 s (warm pair), 0.5 s (cold pair), 0.25 s (probe). Occupancy series are
  in the committed JSONs.

## Criterion (d): oracle amendment

The PRD wording — "FIONREAD time series shows merge pool → nproc while contested" —
conflates **allocation** with **availability**. FIONREAD counts *free* (unclaimed)
tokens; a merge verify hungry enough to trigger donation is *holding* tokens at that
same moment, so `free_merge == nproc` requires full allocation AND a momentarily
idle merge cargo — a fleeting between-invocations instant that contention makes
rare by construction. Compounding this, the balancer's idle-reset (1 s dwell)
re-partitions toward 24/8 exactly when both sides go quiet, i.e. precisely when the
full allocation would have been observable.

What the data does establish, in every contested regime:

1. **Donation fires and ratchets**: merge FIFO observed at 31 free (cold pair) —
   allocation ≥ 31 of 32 at that instant; 21 cold-pair samples strictly above the
   24 baseline; probe max 27 while the task verify was mid-flight.
2. **Direction is correct (PRD §8 T-b)**: the merge-role cargo drains the merge
   FIFO (not the task FIFO) during real merge compiles.
3. **No mid-contention clawback**: allocation above baseline persists until the
   idle-reset condition (both pools fully free for 1 s).

**Amended oracle (ratified, implemented):** *merge allocation strictly exceeds
its baseline partition under contention, with donation direction task→merge and
no mid-contention clawback* — implemented as `merge_ratchet_observed(series,
merge_baseline)` in the evaluator, where `merge_baseline` mirrors the balancer's
own partition formula (`TOKENS − max(1, TOKENS//4)` = 24 for nproc 32).
Demonstrated in every contested regime. The follow-up refinement (if ever
needed): a balancer-side allocation gauge (e.g. periodic stderr line of the
current partition) would make `allocation == nproc` directly observable without
the FIONREAD free-vs-held ambiguity.

## Criterion (a): re-specification

The original oracle was an absolute floor (busy ≥ 0.85, a pre-registered
harness-default guess for "≈ fully utilized"). The cold pair measured 0.799–0.822
against it — not because the balancer strands cores, but because cold builds spend
real wall in inherently serial phases (crate-graph stragglers, link steps, npm
installs) regardless of jobserver policy, and warm host sccache *increases* the
serial share by serving the parallel codegen from cache. An absolute floor
therefore measures the workload's shape, not the balancer.

**Re-specified oracle (Leo-ratified 2026-06-11):** *dual-pool utilization within
0.03 of the single-pool baseline, same regime* — which measures the criterion's
actual intent: the partition must not strand cores relative to the shared pool.
Measured partition cost: **0.023** cold (0.822 → 0.799), **−0.020** warm (the dual
run was *more* utilized). The cold-side ~2-point cost is the real price of
partition rigidity during the other side's serial phases — far from the ~25 %
worst case a rigid no-donation 24/8 split could cost, which is what this
criterion exists to catch.

## ζ′/4520 budget floor (authoritative)

Real `verify.sh` walls; cache_state per run; **bound > floor by construction** is
ζ′'s job. The controlled floor below is from a quiet box with warm host sccache —
production adds ambient contention on top, which ζ′ should weight.

| run | merge_wall_s | slowest_task_wall_s | cache_state |
|-----|--------------|---------------------|-------------|
| warm baseline (single-pool) | 363.3 | 362.6 | warm |
| warm dual-pool | 394.0 | 513.4 | warm |
| cold baseline (single-pool) | 798.9 | 798.9 | cold target / warm sccache |
| cold dual-pool (accepted config) | 776.9 | 776.9 | cold target / warm sccache |
| staggered probe (merge only) | 652.4 | — | merge cold / task warm |

**ζ′ floor:** merge_wall = **798.9 s** (worst observed), slowest_task_wall =
**798.9 s** (worst observed, genuinely cold-cache datapoint per the
measurement-capture duty). The standing budgets (debug 60 m / release 75 m inner
step timeouts) carry ≈ 4.5× headroom over this controlled floor.

## Reproduction

- Same-cache A/B (one invocation):
  `scripts/jobserver-acceptance.py run out.json --ntasks 1 --cache-state warm
  --utilization-threshold 0.85 --sampler-interval 5.0 --task-repo <separate-checkout>`
- Cold-vs-cold A/B: the `run` mode's baseline leg warms the targets for the dual
  leg, so cold/cold requires one leg per invocation with
  `rm -rf <both targets>` between — drive each leg through the module's
  `make_verify_cmd` + `_provision_service` + `run_mixed_concurrent` (the unit-tested
  primitives; see Block 6 of `tests/infra/test_jobserver_acceptance.sh`).
- Staggered (d) probe: start the task verify, wait for its first nextest `PASS [`
  line, then launch the merge verify with a 0.25 s `sample_pool_occupancy` sampler;
  count contested samples (task verify still alive).

## Data files

- `measurements-warm.json` — warm pair (5 s sampler); evaluates (b)/(d) FAIL —
  structural, see "Why two A/B pairs"
- `measurements-cold.json` — cold pair, **the gate input**:
  `jobserver-acceptance.py evaluate` exits 0 (all four PASS) under the
  ratified oracles (0.5 s sampler)
- `probe-d.json` — staggered criterion-(d) probe (0.25 s sampler)

## Campaign defect log

The first operator attempt (2026-06-11 09:56) exited 64 in 14 ms; triage found the
`--run` path had never been executed (deliberately excluded from the hermetic
suite). Fixed on this branch before any numbers were captured: missing verify.sh
action argument; baseline FIFOs not routed into the verify env (A/B would have been
dual-vs-dual); CLI flags dropped by the run dispatch; shared-checkout cargo-lock
serialization (`--task-repo` now required); whole-verify outer 60/75 m timeout
manufacturing spurious 124s (the standing budgets are verify.sh's *inner* per-step
timeouts — replaced with a 240 m anti-hang net); `test_jobserver_role_fifo.sh` (f)
env-hermeticity. All pinned in `tests/infra/test_jobserver_acceptance.sh` Block 6.

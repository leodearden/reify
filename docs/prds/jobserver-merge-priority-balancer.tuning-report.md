# Jobserver Balancer ε Tuning Report

PRD: `docs/prds/jobserver-merge-priority-balancer.md` §9/§10  
nproc: **32**  
MARGIN: **1.5**  
Acceptance: **PASS**

## A/B Comparison: single-pool (baseline) vs dual-pool (balancer)

Baseline **single-pool** (A): single FIFO seeded to nproc, no balancer.  
Balancer **dual-pool** (B): merge + task FIFOs managed by `jobserver-balancer.py`.  

## Regime: just-task

| service | cache_state | busy_fraction | merge_wall_s | task_wall_s | exit_124 |
|---------|-------------|---------------|--------------|------------|----------|
| single-pool | warm | 0.9929 | 0.0 | 0.7 | 0 |
| single-pool | cold | 0.9929 | 0.0 | 1.0 | 0 |
| dual-pool | warm | 0.9852 | 0.0 | 0.9 | 0 |
| dual-pool | cold | 0.9900 | 0.0 | 1.0 | 0 |

## Regime: just-merge

| service | cache_state | busy_fraction | merge_wall_s | task_wall_s | exit_124 |
|---------|-------------|---------------|--------------|------------|----------|
| single-pool | warm | 0.9922 | 1.2 | 0.0 | 0 |
| single-pool | cold | 0.9912 | 0.9 | 0.0 | 0 |
| dual-pool | warm | 0.9972 | 0.9 | 0.0 | 0 |
| dual-pool | cold | 0.9910 | 1.0 | 0.0 | 0 |

## Regime: mixed

| service | cache_state | busy_fraction | merge_wall_s | task_wall_s | exit_124 |
|---------|-------------|---------------|--------------|------------|----------|
| single-pool | warm | 0.9910 | 1.6 | 1.0 | 0 |
| single-pool | cold | 0.9929 | 1.0 | 1.0 | 0 |
| dual-pool | warm | 0.9919 | 0.8 | 1.1 | 0 |
| dual-pool | cold | 0.9755 | 1.1 | 0.6 | 0 |

## Derived Constants

Balancer wired: `POLL_INTERVAL` and `EPSILON` are updated in `scripts/jobserver-balancer.py`. The timeout constants (`task_timeout_secs`, `merge_timeout_secs`) and `utilization_threshold` are informational — consumed by downstream tasks (ζ reads timeouts; η reads the A/B narrative) but not wired into the balancer directly.

| constant | value |
|----------|-------|
| merge_baseline | 24 |
| task_baseline | 8 |
| poll_interval | 0.1 |
| epsilon | 1 |
| task_timeout_secs | 2 |
| merge_timeout_secs | 2 |
| utilization_threshold | 0.7943387881468378 |

Split: merge_baseline=24 + task_baseline=8 = 32 (nproc)  
Merge-favored: True  

## Findings

- **[WARNING] SYNTHETIC_TIMEOUT_NOT_AUTHORITATIVE**: task_timeout_secs=2s is derived from the harness's synthetic load and must NOT be wired into verify.sh; obtain authoritative timeouts by re-running --measure with a real cold-verify load_cmd
- **[WARNING] SYNTHETIC_TIMEOUT_NOT_AUTHORITATIVE**: merge_timeout_secs=2s is derived from the harness's synthetic load and must NOT be wired into verify.sh; obtain authoritative timeouts by re-running --measure with a real cold-verify load_cmd

Overall acceptance: **PASS**  
(escape-valve findings are soft warnings; only hard-fail findings set acceptance to FAIL)

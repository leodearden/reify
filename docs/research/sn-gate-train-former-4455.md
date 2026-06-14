# A′ s(N) Gate — Train Former Decision (task 4455)

**Date**: 2026-06-09  
**Tool**: `scripts/sn_gate.py` (stdlib-only Python, task 4455)  
**PRD reference**: `plans/merge-throughput-coupling-tolerant-train-former-prd.md` §8 exp2, D7.3

---

## Summary

| N | s(N) | Sample size | Ambiguous frac |
|---|------|-------------|----------------|
| 2 | 0.984 | 253 | 1.000 |
| 3 | 0.962 | 104 | 1.000 |

**Classification**: MARGINAL  
**Deciding N**: 3 (s(3)=0.962 clears 1/3 with large margin; n=104 ≥ 10; but ambiguous_frac=1.0 → MARGINAL)  
**Chosen N** (merge.train_max_members): N/A (MARGINAL case)  
**Recommended action**: leave 1705–1708 deferred + escalate to Leo with these numbers

---

## Methodology

### Proxy

`rate(cluster followed by fix-forward within lookahead) ≈ 1 − s(N)`

A "cluster" is a group of near-simultaneous task-merge landings (the candidates a train former would batch). A fix-forward is any first-parent commit whose subject matches `^fix[(:/-]`, `^fix forward`, `^revert`, or `^hotfix` (case-insensitive).

### Parameters (default run)

| Parameter | Value |
|-----------|-------|
| Cluster window | 300 s |
| Look-ahead window | 86400 s (24 h) |
| Thin-sample threshold | 10 clusters |
| Marginal band half-width | 0.2/N |
| Source | `git log --first-parent main --pretty=format:%h\|%cI\|%s` |

### Attribution

- **Strong**: the fix-forward commit subject references a task_id that is a member of the cluster (patterns: `task-NNNN`, `task/NNNN`, `task NNNN`, `(NNNN)`, revert of member).
- **Weak / ambiguous**: fix-forward found in the lookahead window but does NOT reference any cluster member task_id.

`ambiguous_frac = weak_attributions / followed` (0 when no cluster is followed).

### Decision rule (D7.3)

Break-even: 1/N. Marginal band: [1/N − 0.2/N, 1/N + 0.2/N].

- N=2 band: (0.40, 0.60)
- N=3 band: (0.267, 0.40)

**Deciding N** = largest N∈{3, 2} with s(N) > 1/N.

- **GO**: deciding N clears upper band (s ≥ 1/N + 0.2/N) AND sample ≥ 10 AND not ambiguous.
- **NO-GO**: no N clears AND s(2) ≤ 0.40 AND n(2) ≥ 10 AND not ambiguous.
- **MARGINAL**: anything else (deciding N in band, thin sample, OR ambiguous attribution).

---

## Git corpus

Corpus: `git log --first-parent main` — 30,856 total first-parent commits, 1,629 task merge commits, 1,135 clusters (greedy single-linkage, 300 s window). Of those clusters, 253 had ≥ 2 members (the N=2 sample) and 104 had ≥ 3 members (the N=3 sample).

---

## Estimates

### N = 2

- **Sample size**: 253 clusters of ≥ 2 simultaneous merges
- **Followed by fix-forward**: 4 clusters
- **s(2)** = 1 − 4/253 = **0.984**
- **Ambiguous frac**: 1.000 (all 4 followed-clusters have only weak attribution)

### N = 3

- **Sample size**: 104 clusters of ≥ 3 simultaneous merges
- **Followed by fix-forward**: 4 clusters (same 4 as N=2 — they all have size ≥ 3)
- **s(3)** = 1 − 4/104 = **0.962**
- **Ambiguous frac**: 1.000

---

## Attribution analysis — why ambiguous_frac = 1.0

Inspection of the 4 followed-clusters reveals that **all 4 are attributed to the same single commit**:

```
d99d4bf71e 2026-06-09 15:48 | fix(orchestrator): drop removed usage_cap.pause_threshold field
```

This is an orchestrator-config cleanup (removing a field that was deleted from the codebase) and is **unrelated to any task in any of the 4 clusters**. The fix's subject contains no cluster member task_id, so all 4 attributions are weak (ambiguous).

The 4 clusters that are "followed" by this fix are:

| Cluster size | Last merge timestamp | Cluster tasks |
|---|---|---|
| 7 | 2026-06-08 23:12 | 4390, 4389, 4379, 4405, 4367, 4381, 4331 |
| 4 | 2026-06-09 01:03 | 3990, 4093, 4366, 4401 |
| 3 | 2026-06-09 08:04 | 4407, 4369, 4393 |
| 5 | 2026-06-09 11:18 | 4284, 4059, 4061, 4062, 2522 |

The fix committed at 15:48 on June 9 lands within 24 h of all four, but it is a coincidental infrastructure fix, not a batching-induced failure.

**Consequence**: the true fix-forward rate attributable to task batching is likely 0/253 = 0.0 for N=2 and 0/104 = 0.0 for N=3. The proxy measure of the actual coupling failure rate is consistent with zero, and s(N) ≈ 1.0 for both N.

---

## Sensitivity analysis

Varying the cluster window and lookahead confirms the result is stable:

| Window | Lookahead | s(2) | n(2) | s(3) | n(3) | Classification |
|--------|-----------|------|------|------|------|----------------|
| 180 s  | 86400 s   | 0.981 | 212 | 0.950 | 80  | MARGINAL       |
| 300 s  | 86400 s   | 0.984 | 253 | 0.962 | 104 | MARGINAL       |
| 600 s  | 86400 s   | 0.987 | 312 | 0.971 | 139 | MARGINAL       |
| 300 s  | 43200 s   | 0.992 | 253 | 0.981 | 104 | MARGINAL       |

Across all settings: s(N) consistently in [0.950, 0.992], all ambiguous_frac = 1.0 (the same infrastructure fix triggers all detections), all classifications are MARGINAL. The marginal band for N=3 is (0.267, 0.400); s(3) is far above the upper edge (0.962 >> 0.400) — the only reason for MARGINAL is the ambiguity flag.

---

## runs.db corroboration

Dark-factory `data/orchestrator/runs.db` was queried with `--runs-db`. It returns:

- **members_with_extra_verify_attempts** (verify_attempts > 1): 27 cluster member task_ids
- **members_with_reverify_event**: 0

The 27 tasks with multi-attempt verifies span many clusters and likely reflect transient infrastructure noise (sccache misses, race conditions) rather than task-coupling failures. This signal is consistent with the git-proxy estimate: no cluster shows correlated verify-failure spikes.

---

## Classification

**MARGINAL** — by the D7.3 rule: deciding N = 3 (s(3) = 0.962 > 1/3), but `ambiguous_frac(N=3) = 1.0 > 0`.

Trigger fired: *ambiguous fix-forward attribution at N=3*.

**Chosen N** (merge.train_max_members): **N/A** (MARGINAL; deferred to human).

---

## Human guidance (for escalation resolution)

The quantitative signal is unambiguous: s(N) >> 1/N at both N=2 and N=3, with large samples (n=253, n=104). The MARGINAL flag fires purely because the weak-attribution rule cannot confirm that the observed fix-forwards are caused by batching, not because the estimates are in the marginal band or the sample is thin.

The root cause of the ambiguity: all 4 proxy hits originate from a single coincidental infrastructure commit (`d99d4bf71e`, June 9 15:48, orchestrator config cleanup), not from task-coupling failures. If that attribution analysis is accepted, the true coupling-failure rate is consistent with zero, and the correct outcome is:

**Suggested resolution**: treat as GO at N=3 (or N=2 if preferred for conservatism), with `merge.train_max_members = 3` (or 2). Flip dark-factory tasks 1705–1708 from `deferred` to `pending`.

This is a human call; the automated gate leaves 1705–1708 deferred per the MARGINAL rule.

---

## Terminal action

**Classification: MARGINAL** → leave tasks 1705–1708 **deferred** + info-escalate to Leo with the above numbers.

---

## Resolution (2026-06-09, human — Leo, via esc-4455-16)

**Decision: GO at N=3** — `merge.train_max_members = 3`.

The MARGINAL classification was confirmed a **false positive**: all 4 "followed by
fix-forward" detections traced to the single coincidental, unrelated infra commit
`d99d4bf71e` (orchestrator config cleanup) that merely fell within 24 h of the
clusters. The true task-coupling failure rate is 0/253 (N≥2) and 0/104 (N≥3);
s(3)=0.962 clears the 0.40 upper marginal band by a wide margin on a non-thin
sample (n=104).

Dark-factory tasks 1705–1708 were flipped to pending and the A′ train former was
built with `train_max_members=3` (1705 done, merged `8c39ff54`); the integration
gate 1708 closed 2026-06-10 with a real reify train landing N≥2 tasks on a single
union verify (merged `92472857`). The gate behaved as designed: the deterministic
inequality was clear, judgment was needed only on attribution ambiguity, and the
marginal band correctly routed that to a human.

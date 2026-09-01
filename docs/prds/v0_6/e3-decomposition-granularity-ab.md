# E3 — PRD decomposition-granularity A/B experiment: protocol & pre-registration

**Status:** REGISTERED 2026-08-28 (arms frozen and filed; pre-registration committed before any
coarse-arm dispatch). Landing vehicle: task #6897, branch `task/6897`, via the merge queue
(Leo's 2026-08-27 ruling: no direct-to-main commits while the queue is live).
**Owner:** the task-sizing investigation (dark-factory session 2026-08-27/28, Leo + Claude);
executed by the spawned e3-granularity-ab session, 2026-08-28, under Leo's authorization
(`/spawn (fable) design, prepare and either file or execute E3`).
**Resolution:** all arms terminal (every arm PRD's close leaf done or cancelled) **or
2026-10-09** (6 weeks), whichever first.

This document is the pre-registration: the hypotheses, predictions, endpoints, analysis plan
and randomization record below were committed before `commit_planning` released any coarse-arm
task to the scheduler. Coarse-arm task text lives in the task store and in
`e3-granularity/coarse-*.md`; analysis tooling in `e3-granularity/analysis/`.

## 1. Question and background

Does **coarser PRD decomposition** (fewer, larger tasks) improve cost per delivered scope and
rework burden without degrading work quality, in the reify orchestrator target?

Motivating evidence (observational, 2,191 dark-factory + 2,031 reify landed tasks; full record:
dark-factory memory `project_task_sizing_investigation_2026_08_27.md`):
- Per-task fixed overhead dominates: cost ≈ $9.20/task + $3.13/kLOC (reify OLS). Sub-100-LOC
  tasks spend ~80% of budget on plan+review scaffolding. $/100LOC falls ~15–20× from smallest to
  largest size bucket, monotone, in both repos.
- Risk rises with task size but **sub-linearly per delivered scope** (df: 5×300-LOC tasks ⇒ ~63%
  chance of ≥1 blocking escalation at ~3× the cost of one >1000-LOC task at ~35%). Reify's
  size-risk slope is steeper (+37pp vs df +19pp) and its >15-file cliff is soft.
- Era effects are confounded (ops churn, config drift, task-mix drift; df median landed task
  drifted 70→1,457 LOC Mar→Aug organically). **Observational data cannot settle the
  quality/risk question — hence this prospective experiment.**
- E4 (2026-08-27): the old cross-package pre-split rule's basis is gone under Opus 5; the
  binding margin is the architect turn cap, not budget. NOTE: reify's architect routing config
  at registration is opus/max/$25/**180 turns** (read from live task routing records
  #6706/#6768), so df's 100-turn margin concern is relaxed here; coarse tasks were still kept to
  ≤3 constituent leaves each.

## 2. Design

- **Unit of randomization: the PRD** (tasks within a PRD are correlated).
- **Population:** the 7 untouched fresh v0_6 PRD decompositions (decomposed 2026-08-26/27),
  stratified:
  - **Stratum A** (5 clean PRDs, all leaves pending at freeze): geometry-algebra-solver-unification,
    gui-on-demand-measurement, gui-purpose-surface, solution-set-completeness, solver-driver-parity.
  - **Stratum B** (2 strong-coalescing PRDs, 1.2–1.5 avg files/task): driver-contract-implementation,
    spec-conformance-suite.
  - **Excluded:** solver-legibility-telemetry (3 tasks already progressed).
- **Treatment (coarse arm):** re-decomposition into 2–3× fewer tasks, each ≤ ~12 declared files
  and roughly 300–1,500 LOC expected scope; integration-gate and PRD-close leaves preserved as
  singletons (the gate rule predates this experiment; the close leaf is a reify overlay
  obligation). Control (standard arm): the decomposition as filed 2026-08-26/27, untouched.
- **Coarse-arm authoring method — deliberate deviation from "same pipeline", recorded:** the
  brief asked for the same /prd overlay pipeline with a coarseness instruction where possible.
  We instead **coalesced the verified standard leaves text-preservingly** (full constituent
  descriptions/details/delivered_checks/signals carried into the coarse tasks verbatim,
  restructured under one preamble). Rationale: a fresh pipeline run would re-derive leaf CONTENT
  and could drift semantically from the already-D3-verified standard text, confounding the
  granularity treatment with content variance. Coalescing holds content constant, so the arms
  differ in granularity only. Costs of this choice: (a) the coarse arm's authoring process
  (one Fable session coalescing) differs from the standard arm's (/prd decompose sessions) —
  a process-identity limitation; (b) the prd-decompose-verify workflow was NOT re-run over the
  coarse batch — its premises are the constituents' premises, verified by that workflow at the
  standard decompose 1–2 days earlier and frozen since (the pool was deferred before authoring
  began); the deterministic lock-charter-guard WAS run per coarse task. Both recorded as
  limitations in §9.
- **Large-plan routing:** coarse tasks will trip plan_min_steps/plan_min_modules routing
  (opus, 120 implementer turns). That is part of the treatment — recorded, not fought.
  No `metadata.model_overrides` pins were added to either arm (model_overrides can pin model
  only, and a pin would confound the treatment).

## 3. Randomization record

Procedure (committed before rolling): seed = full sha of reify `main` HEAD at randomization
time; RNG = Python `random.Random(seed_string)`; stratum B (alphabetical:
[driver-contract-implementation, spec-conformance-suite]) `rng.sample(B, 1)` → coarse; then
stratum A (alphabetical: [geometry-algebra-solver-unification, gui-on-demand-measurement,
gui-purpose-surface, solution-set-completeness, solver-driver-parity]) `rng.sample(A, 3)` →
coarse. One roll, no re-rolls.

- **Seed:** `72b9c5b08322094677af181a8742a3790c5330da` (main HEAD, 2026-08-28 ~06:30Z; also the
  base commit of branch task/6897).
- **Result — COARSE arm (4):** spec-conformance-suite (B); gui-purpose-surface,
  gui-on-demand-measurement, solution-set-completeness (A).
- **Result — STANDARD arm (3):** driver-contract-implementation (B);
  geometry-algebra-solver-unification, solver-driver-parity (A).

## 4. State at freeze (verified live, 2026-08-28 ~06:40–07:00Z)

- All 5 stratum-A PRDs fully pending (verified via get_statuses over all stamped leaf ids).
- driver-contract-implementation: all 25 leaves pending (6798 had been unblocked since the
  08-27 briefing).
- spec-conformance-suite: α #6758 **done**, β #6759 **in-progress** at freeze — both are
  carve-outs, EXCLUDED from every endpoint in both arms; remaining 22 leaves pending.
- Orchestrator: up (port 8100, started 03:20Z), merge queue moving (verify in flight, 4
  landings in-run), not halted; laptop verify host quarantined (Tailscale unreachable) —
  capacity halved at registration. Restart cadence: regular ~8–9h run boundaries (runs.db,
  last 7 days) — the known steady-state drain pattern; df cadence fixes 4754/4755/4763 still
  pending ⇒ censoring rule §7.4 applies.
- The 6873 decompose-workflow cost trap: fix landed on main (`54ad3dfde3`, all four
  prd-decompose-verify roles model-pinned) — closed.

## 5. Arms as filed (mapping tables)

Umbrella/landing task: **#6897** (this doc's branch). Arm marking: every new coarse task carries
Tier-C metadata `x_e3_arm: "coarse"`, `x_e3_prd: "<slug>"`, `x_e3_constituents`; reused
singletons carry `x_e3_arm: "coarse"` + `x_e3_reused: true`; retired standard leaves carry
`x_e3_arm: "standard-retired"` + `x_e3_replacement` + `x_e3_flip_condition`. Standard-ARM tasks
(the 3 control PRDs) are identified by PRD membership only and were not touched.

### gui-on-demand-measurement — 9 → 4 (2.25×)

| Coarse | Constituents (retired standard ids) | Deps as filed |
|---|---|---|
| **#6898** E3C1 | α #6740 + β #6741 + γ #6742 | 6667, 6723 |
| **#6899** E3C2 | δ #6743 + ε #6744 + θ #6747 + η #6746 | 6898, 6666 |
| **#6745** (reused) | ζ gate, unchanged text | 6898 |
| **#6748** (reused) | ι PRD close, unchanged text | 6898, 6899, 6745 |

### solution-set-completeness (P3) — 12 → 6 (2.0×)

| Coarse | Constituents | Deps as filed |
|---|---|---|
| **#6706** (reused) | α carrier, unchanged | — |
| **#6900** E3C2 | γ #6708 + ε #6710 + λ #6716 | 6655, 6706 |
| **#6901** E3C3 | β #6707 + δ #6709 | 6706, 6653, 6900 |
| **#6902** E3C4 | ζ #6711 + η #6712 + κ #6718 | 6901, 6691, 6699 |
| **#6903** E3C5 | θ #6713 + ι #6715 | 6706, 6901, 6659, 6677 |
| **#6719** (reused) | μ PRD close, unchanged text | 6706, 6900–6903 |

### gui-purpose-surface — 10 → 5 (2.0×)

| Coarse | Constituents | Deps as filed |
|---|---|---|
| **#6904** E3C1 | α #6803 + β #6831 | 5748, 6898 |
| **#6905** E3C2 | γ #6832 + δ #6833 + ε #6834 | 6904, 6723, 6898 |
| **#6906** E3C3 | ζ #6835 + θ #6837 + ι #6838 | 6905, 6899, 6904 |
| **#6836** (reused) | η gate, unchanged text | 6904, 6905, 6906 |
| **#6839** (reused) | κ PRD close, unchanged text | 6904–6906, 6836 |

### spec-conformance-suite — 22 → 10 (2.2×; carve-outs #6758 done / #6759 in-flight excluded)

| Coarse | Constituents | Deps as filed |
|---|---|---|
| **#6907** E3C1 | γ #6761 + ε #6763 + ι #6767 | 6758, 6759 |
| **#6908** E3C2 | δ #6762 + ο #6774 | 6907 |
| **#6909** E3C3 | ζ #6764 + η #6765 + θ #6766 | 6907, 6908 |
| **#6910** E3C4 | μ #6770 + ν #6771 + ξ #6772 | 6907 |
| **#6911** E3C5 | π #6776 + ρ #6778 | 6909, 6907, 6768 |
| **#6912** E3C6 | σ #6780 + τ #6782 + φ #6785 | 6909, 6015, 6904 |
| **#6913** E3C7 | λ #6769 + υ #6783 + χ #6787 | 6907, 6909, 5403, 5404, 6800, 6806, 5517–5521 |
| **#6768** (reused) | κ ratchet root, unchanged | — |
| **#6789** (reused) | ψ coverage promotion, unchanged text | 6909, 6911, 6912, 6913 |
| **#6791** (reused) | ω PRD close, unchanged text | 6758, 6759, 6907–6913, 6768, 6789 |

**Totals:** coarse arm = 53 standard leaves → 25 tasks (16 newly filed + 9 reused singletons);
2.12× overall, 2.6× over the genuinely coalesced subset (44 → 16).

### Cross-arm dependency remaps (inbound edges into retired leaves)

Found by exhaustive reverse-dependency scan (1,097 non-terminal tasks, 11 pages; positive
control: all 53 intra-set edges read back correctly). Exactly five, all standard-arm
driver-contract leaves; each got add-new-edge-then-remove-old:

| Dependent (standard arm) | Old dep | New dep |
|---|---|---|
| #6773 DC-α | 6803 | **6904** |
| #6777 DC-γ | 6740 | **6898** |
| #6779 DC-δ | 6740 | **6898** |
| #6804 DC-φ | 6803 | **6904** |
| #6807 DC-ω | 6837 | **6906** |

No `metadata.external_deps` in either repo referenced a retired id. Reverse direction
(coarse→standard-arm ids, e.g. 6913→6800/6806, 6912→6015) uses real unchanged ids — no remap
needed. Cross-PRD prose citing retired ids (e.g. P4's #6751 stamping #6706/#6711) stays
resolvable: the retired records exist (deferred) and carry `x_e3_replacement`.

## 6. Endpoints

Constraint (Leo): mixed wide-lock/narrow-lock populations structurally starve wide-lock tasks —
**no calendar-time endpoint anywhere**; filing-to-completion wall-clock is banned as a metric.

**Primary** (per arm, aggregated per PRD):
1. **Cost per delivered kLOC** — invocation cost (runs.db `invocations`/`invocation_end`)
   attributed to each task, summed per arm, over net LOC delivered in the task's landed merge
   diff (same metric as `analysis/reify_task_size_analysis.py`).
2. **Rework** — dispatch attempts per task (task_started events per task id; re-dispatches after
   failure), aggregated per PRD; reported both per-task and per delivered kLOC.
3. **Blocking escalations per delivered kLOC** — escalation_created events with blocking
   class, per arm.

**Secondary:** reviewer budget saturation via cost/budget ratios per invocation
(`invocations.capped` is DEAD — 0 across ~15.7k rows; never use it); turn saturation where
recoverable from transcripts/events; merge conflicts (merge_attempt outcomes per arm).

**Lock-wait — a measured OUTCOME, reported per arm, never folded into any time metric:**
`task_skipped` streaks per task, `reservation_installed` counts, and eligible-to-dispatch delay
measured from ALL-DEPS-DONE to first dispatch (this definition is arm-fair: coarse tasks have
larger dep fan-in by construction). This doubles as the contention experiment.

**Quality** (estimation, not hypothesis tests — see §8):
4. **Blinded multi-lens judge panel** over landed diffs per PRD (§7.5).
5. **Post-landing defect escape** — fix-task density in each PRD's touched modules over the 30
   days following its close, normalized per delivered kLOC.

## 7. Analysis plan

### 7.1 Attribution
Coarse tasks by `x_e3_arm` metadata; standard arm by PRD membership (§5 tables + the three
control PRDs' stamped leaf lists: P1 #6689–#6704 (15), P2 #6668–#6687 minus #6674 (19), DCI
#6773–#6808 odd-stamped 25). Carve-outs excluded from both arms: #6758, #6759. Reused
singletons count in the coarse arm. The umbrella task #6897 and this doc's landing costs are
infrastructure, excluded from both arms.

### 7.2 Data sources
`data/orchestrator/runs.db` (events, invocations, task_results), the fused-memory task store
(read via MCP), git main history (merge commits per task branch; LOC and file footprints;
widening = touched-but-undeclared, per the parent scripts). Scripts: `e3-granularity/analysis/`
(copied from the parent investigation session; `task_rows.json`/`reify_task_rows.json` snapshots
were NOT copied — they are re-derivable; `e2_df_task_models.json` WAS copied because transcript
retention (~37 days) makes it unrecoverable later).

### 7.3 Comparison
n=7 PRDs (4 coarse / 3 standard) ⇒ only large effects are testable. Report per-PRD point
estimates, arm medians, and bootstrap-by-PRD intervals; a Mann-Whitney U on per-PRD $/kLOC as
the single confirmatory test (α=0.1, one-sided per P1's direction). Everything else is
descriptive. Within-stratum-B comparison (SCS coarse vs DCI standard) is the
closest-matched pair — report it separately.

### 7.4 Censoring (restart drains)
The orchestrator restarts on a ~8–9h cadence; df fixes 4754/4755/4763 were pending at
registration. A dispatch attempt terminated by an orchestrator shutdown (invocation_end
subtype / cancellation at a run boundary) is a **censoring event**: it does not count as
rework for either arm, and censored-attempt counts are REPORTED per arm. If the cadence fixes
land mid-experiment, record the date; do not otherwise adjust.

### 7.5 Blinded judge panel (design now, run after landings)
At resolution: for each arm PRD, assemble the landed diffs grouped by capability area (not by
task, so arm identity is not inferable from diff sizes alone — recorded as an imperfect blind).
Three lenses — architectural coherence, seam quality, test quality — each scored 1–7 with
rationale by fresh-context sessions on a mid-tier model (sonnet-class; never the session's own
context), 2 independent judges per lens per PRD, PRD identity visible, arm identity never
stated. Report per-PRD medians by arm. The judge prompt template is written at panel time and
committed beside the results.

### 7.6 Era guard
Registration-time environment: reify orchestrator config as of 2026-08-28 (architect
opus/max/$25/180t; merge verify 2 hosts, laptop quarantined), main HEAD 72b9c5b083. If a major
orchestrator config change or model-lineage default switch lands mid-experiment, record the
date and split descriptives at it; do not discard data.

## 8. Predictions (pre-registered, direction + rough magnitude)

1. **Cost per delivered kLOC: coarse 25–50% LOWER** than standard (mechanism: ~28 fewer
   per-task fixed overheads ≈ $260 saved on comparable scope; reify fixed ≈ $9.20/task).
   Highest-confidence prediction.
2. **Rework per task: coarse 1.2–1.5× HIGHER** (bigger tasks fail verify/review more per
   attempt); **rework per delivered kLOC: coarse LOWER** (~equal or better).
3. **Blocking escalations: coarse per-task probability HIGHER** (~35–50% vs ~16–25%), **per
   delivered kLOC 20–40% LOWER** (sub-linear risk scaling). Hedge: reify's steeper size-risk
   slope (+37pp) may erode this; if any coarse task exceeds 15 declared files at implementation
   (widening), expect the cliff shape.
4. **Reviewer budget saturation: coarse HIGHER** (larger diffs per review); some coarse
   reviews expected at >80% budget.
5. **Turn saturation:** occasional coarse implementer runs near the 120-turn large-plan cap;
   architect runs comfortably under 180 (constituent count capped at 3–4).
6. **Merge conflicts:** fewer conflict events in the coarse arm (25 vs 53 merges at ~1–2%
   conflict rate each); no rate-per-merge difference detectable.
7. **Lock-wait:** coarse arm shows FEWER task_skipped streaks and reservations (inter-task
   waits become intra-task sequencing); per-task lock-hold durations LONGER.
8. **Quality (panel):** no detectable difference at this n; weak prior of slight coherence
   advantage to coarse (single-context implementation of related seams) and slight test-quality
   risk in coarse (reviewer attention thins on big diffs).
9. **Defect escape:** no strong prior; weakly, fewer cross-task seam defects in coarse.

## 9. Limitations and threats to validity

- n=7 PRDs; randomization can hand either arm a harder mix. The PRDs also differ in kind
  (GUI-heavy vs harness-heavy) — stratification only partially compensates.
- Authoring-process asymmetry (§2): coarse arms coalesced by one Fable session from the
  standard leaves; standard arms authored by /prd decompose sessions. Content was held
  constant, process was not. Authoring costs are excluded from endpoints and are asymmetric.
- The prd-decompose-verify (D3) workflow was not re-run over coarse batches (premises
  identical by construction; deterministic lock-charter-guard was run). If a coarse preamble
  introduced a false premise, D3 would not have caught it.
- The coarse arm's dep fan-in delays its first dispatches behind out-of-PRD deps (e.g. GOM-C1
  waits on 6667+6723 where standard α was dep-free). Eligible-to-dispatch delay is measured
  from deps-met, which is arm-fair, but total arm throughput is not a readable endpoint.
- Judges can partially infer arm from diff structure; the blind is imperfect (§7.5).
- Concurrent factory evolution (config, models, df fixes) over the ~6-week window; §7.6 era
  guard mitigates descriptively only.
- 6759 (in-flight carve-out) lands mid-experiment and its scope borders SCS-C1's; both arms
  treat it as substrate, and it is excluded from endpoints.
- The scheduler treats the two arms identically but they coexist in one factory: cross-arm
  resource contention (verify hosts, CPU admission) is shared, which is realistic but means
  arms are not independent samples in the strict sense.

## 10. Execution log (state changes, 2026-08-28, UTC)

1. ~06:30 randomization (seed = main HEAD 72b9c5b083, §3).
2. 06:47–07:07 freeze: all 53 coarse-arm-PRD standard pending leaves → `deferred`
   (batched set_task_status; every transition verified pending→deferred; no dispatch race).
3. 07:10–07:22 coarse batch filed via submit_task(planning_mode=True) → #6898–#6913, all
   created `deferred`; lock-charter-guard PASS on every files list beforehand; umbrella #6897
   filed the same way at 06:45.
4. 07:24–07:27 dependency rewires: reused singletons' dep lists replaced (6745, 6748, 6719,
   6836, 6839, 6789, 6791); the 5 inbound edges remapped add-first-then-remove (§5).
5. 07:30–08:0x metadata pass: x_e3 tags + flip conditions on all retired leaves; authored
   memory_hints re-asserted on 6740/6741/6834/6835/6837/6838 (the deferred-transition
   reconciliation hint-clobber is live and was observed; restoration verified by readback).
6. Protocol committed on branch task/6897 and submitted to the merge queue (request id in
   #6897's record). **Pre-registration point.**
7. After (6): curator_action audit on #6898–#6913 (none combined), then
   commit_planning(#6898–#6913 → pending) + set_task_status(reused singletons → pending) +
   full readback verification. **Dispatch opens here.**
8. Any deviation after step 7 gets a dated addendum below; the body above is frozen.

### Addendum 2026-09-01 — scope refinement on GOM-E3C1 #6898 (A5-R)

**What changed.** A refinement note (`A5-R`) was appended to #6898's `details`, fixing the
*carrier type* for the structured measured-value channel that A5 already required and that Leo
had already ruled to on 2026-08-28. It imports `solver-legibility-telemetry` §8.1 item 2's
structural constraint (sibling field on `ConstraintCheckEntry`, never a payload on
`Satisfaction`) into #6898's own text, and requires a dimension-bearing carrier because the
kernel-measured values are heterogeneous (deviation/min-wall are Lengths; overhang/draft are
Angles, so an undifferentiated `Option<f64>` is the INV-AD-4 erasure). Two scope cuts were named
(`achieved_repr_tol` subsumption; deriving the diagnostic string from the record).

**Mirrored to** retired standard leaf #6741 (a §11 rollback target, so its text must not go
stale), the coarse spec doc, and the PRD's capability-manifest twins. A matching bidirectional
coordination note was recorded on #6722 (P4 ε, not an E3 task).

**Endpoint impact — declare, do not hide.** This is a scope *refinement*, not new scope:
C-STATUS already promised measured values `ConstraintCheckEntry` cannot carry, so the work was
latent in the task as filed and would have surfaced at dispatch as a block or an escalation.
Stating it pre-dispatch moves that cost from the endpoint-bearing implement phase into
authoring, which is *excluded* from endpoints (§9) — so the likely direction is a small
FAVOURABLE bias to #6898's measured blocks/turns, not an unfavourable one. No control-arm PRD
received an equivalent pre-dispatch refinement. **Analysis should annotate #6898 rather than
censor it**; if the blinded judge panel (§7.5) sees this task, its brief should note the
refinement exists.

**Protocol gap this exposes.** §11 covers abort/rollback but the protocol has no *amendment*
procedure for a live coarse task. This addendum is the ad-hoc instance; if amendments recur,
E3 should adopt an explicit rule (amend-both-arms, or freeze-and-defer).

## 11. Abort / rollback procedure

If E3 must be aborted before coarse landings: set the 16 coarse tasks (#6898–#6913) and, if
not yet landed, their in-progress work to `cancelled` ONLY after flipping the 44 retired
standard leaves back to `pending` first and re-reversing the §5 edge remaps (add old edge,
remove coarse edge) — that order keeps every dependent continuously blocked, never dangling on
a cancelled dep (cancelling a task ARMS its dependents). Reused singletons flip back by
restoring their original dep lists (recorded in §5). Partial landings: a coarse task that
already landed stays done; its constituents stay retired; only unlanded groups roll back.

## 12. Analysis-runner note

Mining scripts live in `docs/prds/v0_6/e3-granularity/analysis/` (copied 2026-08-28 from the
parent investigation's scratchpad
`/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad/`
before /tmp cleanup):
- `reify_task_size_analysis.py` / `task_size_analysis.py` — the landed-task miners (LOC, cost,
  escalations, widening) for reify / dark-factory; the E3 analysis adapts these with an
  x_e3-arm filter.
- `reify_refine.py`, `refine2.py`, `reify_e2.py`, `e2_df_*.py` — era/model analyses (E2).
- `e2_df_task_models.json` — transcript-derived exact-model ground truth (UNRECOVERABLE after
  transcript retention expiry; that is why it is committed).
- NOT copied: `task_rows.json`, `reify_task_rows.json` (1.2–1.3MB mined snapshots,
  re-derivable from runs.db + task store), `e4/` (E4 artifacts, unrelated).
Model-per-run is NOT recorded anywhere in runs.db (lineage aliases only) — exact-model
attribution needs run-timestamp × lineage-switch-date cross-referencing, per the parent
investigation.

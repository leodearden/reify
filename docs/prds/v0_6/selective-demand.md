# PRD (forward-stub): Selective demand — make the two-cone scheduling model real

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub with ACTIVE milestone tracker · **Date:** 2026-06-11
**Parent intent:** `docs/reify-implementation-architecture.md` §3.2/§3.3 (demand registry + two-cone scheduling).
**Tracker:** `[MILESTONE]` task **4533** (active-intervention pattern: pending, dep-gated on
4361+4530+4531+4532; escalates for human expansion when pre-conditions land — replaces silent
deferred parking).

## Why deferred

Demand is currently **degenerate-total**: `build_demand_for_graph` (`engine_eval.rs:169`) marks every
value/constraint/realization node always-demanded, so the `dirty ∩ demand` intersection in
`edit_param` never prunes. Making demand selective is the spec's intent (§3.2 names the sources:
viewport-visible realizations, constraint-panel constraints, property-editor cells) — but the
binding seams do not exist yet, and two correctness prerequisites are open:

1. **No demand-consulting scheduler.** Geometry work is caller-invoked whole-pass
   (`tessellate_snapshot` walks every realization); demand cannot gate work that is not scheduled.
   The seam arrives with the unified-DAG warm stage (θ **4361**: demand-scoped driver seeding) and
   the edit-path re-homing (θ2 **4531**).
2. **Staleness → wrong-pruning conversion.** `edit_param` does not rebuild
   `reverse_index`/`trace_map`/`demand` after structural re-elaboration (collection grow / forall
   re-emission) — repro-confirmed 2026-06-11, task **4530**. Under total demand this is
   missed-propagation; under selective demand the same staleness becomes **silent wrong-pruning**.
   4530's fix is a hard prerequisite.
3. **G6 premise unproven.** Whether the win is real (and whether coarse per-realization demand
   suffices vs. fine per-cell) awaits the measurement artifact from task **4532**
   (`docs/design/selective-demand-measurement.md`).

## Substrate gap (verified 2026-06-11)

- `DemandRegistry` API exists (`demand.rs`: `add_demand`/`remove_demand`/`rebuild_cone`) but has no
  production selective caller; Resolution/Compute nodes are not demand-seeded at all.
- **Pending-from-pruning semantics do not exist.** `Freshness::Pending` arises today from
  failure-gating and in-flight compute, never from demand pruning. The spec's contract
  (§7.2: Pending "retains the most recent substantive result … but does NOT trigger downstream
  re-evaluation") is the intended shape. GUI badge *rendering* for 4-variant Freshness exists
  (task 2337, done); what is missing is feeding it `Pending{last_substantive}` for pruned-but-
  displayed cells (property panel shows last-substantive value + staleness badge, never a silently
  stale number).
- **Cold `build()` stays eager-over-reachable by decision** (`engine-unified-build-dag-option-a.md`
  ~line 61: lazy DCE rejected — it would suppress declared-but-unconsumed geometry errors).
  Selective demand is a **warm/GUI-only** concern.

## Sketch (when activated)

1. Promote 4532's observational registry to enforced demand sources (viewport visibility,
   property panel, constraint panel), with registration updates on visibility/panel change.
2. Warm driver seeding consults the demand cone (`dirty ∩ demand` becomes a real filter on the
   unified driver's warm paths; granularity per 4532 data — likely coarse per-realization first).
3. Pruned-but-displayed nodes → `Pending { last_substantive }` + GUI badge/value surfacing.
4. Demand-cone incremental maintenance across structural edits (inherits 4530's invariant).
5. Differential gate: with all sources registered "everything visible", results must be
   byte-identical to total demand.

## Consumer (G1)

GUI interactive edit loop — the spec §3.3 P1-slow tier becomes scheduler-real: hidden bodies stop
costing kernel time on every slider drag. Secondary: LSP/editor warm paths (`eval_cached`) skip
cones feeding nothing displayed.

## Pre-conditions for activating

- θ **4361** + θ2 **4531** landed (demand-scoped driver on warm + edit paths).
- **4530** landed (dep-structure rebuild invariant).
- **4532** measurement artifact committed and the win confirmed (G6); it also decides granularity.
- Cutover ι **4362** desirable but not strictly required (can develop behind the unified-dag flag).

## Out of scope

- Cold `build()` demand-gating (declined by design — eager error surfacing).
- Selective realization **eviction** (sibling stub: `selective-realization-eviction.md` — demand
  prunes *invisible* work; eviction prunes *unaffected* work).

## Decomposition (when activated — not filed now)

α enforce demand sources + differential gate · β driver-seeding demand filter (warm) ·
γ Pending-from-pruning + last-substantive GUI surfacing · δ demand-cone incremental maintenance ·
ε e2e: hidden-body slider session does zero kernel work for the hidden body (debug-MCP observable).

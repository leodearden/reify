# PRD (forward-stub): Selective realization eviction — executed-result-hash incremental geometry

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub with ACTIVE milestone tracker · **Date:** 2026-06-11
**Parent:** `docs/prds/v0_6/engine-unified-build-dag.md` D8 + "Out of scope" ("Selective realization
eviction … a follow-up after `RealizationNodeData` result hashing exists").
**Tracker:** `[MILESTONE]` task **4534** (active-intervention pattern: pending, dep-gated on
4361+4531; escalates for human expansion when pre-conditions land).

## Why deferred

Every edit conservatively flushes the **entire** `RealizationCache`
(`Engine::clear_realization_cache`, called at `edit_param`/`edit_source` entry —
`engine_edit.rs:901`, contract-locked by task 2874 steps 17/19/20/22): the engine cannot prove
which cached `GeometryHandleId` entries survive a given edit without per-entry input-cone
analysis it does not maintain. So a one-param slider drag re-executes kernel ops for **every**
realization on the next build surface, affected or not.

The unified-DAG red-team scoped the fix out of θ deliberately (D8): the incremental machinery
that exists is **not usable as-is** —

- `compute_dirty_cone_with_realizations` (`dirty.rs:95`) has **no production caller** (staged for
  the ComputeNode pipeline, exercised only by tests).
- `diff_realizations` keys on the **static IR `content_hash`** (id + ops), which never moves on a
  value-driven geometry change — wiring propagation on it would silently no-op
  ("a guaranteed future 4317-class stale", design doc §5.2).

## Substrate gap (verified 2026-06-11)

- **No executed-result / input-cone hash on `RealizationNodeData`.** The stable identity that
  exists is GHR-β's `(realization_ref, upstream_values_hash)` on `Value::GeometryHandle`
  (`kernel_handle` is ephemeral and excluded from `content_hash` by design) — a likely seed for
  the eviction key, but it is recorded on the *value*, not the realization node, and only after
  GHR hydration.
- `RealizationCache` keying is `(entity_id, repr_kind, options_hash, tol)` with the
  tighter-satisfies-looser tolerance partial order (`realization_cache.rs`) — eviction must
  compose with that partial order, not bypass it.
- The wholesale-flush behavior is **pinned by contract-lock tests**
  (`tests/tolerance_wiring_e2e.rs`, task 2874): activation must consciously supersede those
  pins, not break them silently.

## Sketch (when activated)

1. Record a per-realization **input-cone hash** (upstream value reads + upstream realization
   input-hashes, mirroring spec §3.6's "input hashes, not output hashes" rule) at execution time
   on `RealizationNodeData` / the cache entry.
2. On edit, recompute-then-compare input-cone hashes to seed `changed_realizations`; give
   `compute_dirty_cone_with_realizations` its first production caller.
3. Replace the wholesale `clear_realization_cache()` in edit paths with keyed eviction of only
   the changed cone; supersede the task-2874 contract-lock tests with equivalent
   "stale entries never survive" pins on the selective path.
4. Differential gate: selective eviction must never serve a handle the wholesale flush would
   have evicted (staleness corpus: param edit, guard flip, collection grow, source edit).

## Consumer (G1)

GUI warm edit loop (P1-slow latency): a slider drag re-realizes only the affected bodies.
Complements `selective-demand.md` — demand prunes *invisible* work; eviction prunes
*unaffected* work. Both ride the unified driver's warm paths.

## Pre-conditions for activating

- θ **4361** landed (warm surfaces on the driver; D8 explicitly keeps full-flush through θ).
- θ2 **4531** landed (edit paths on the driver — eviction modifies edit-path flush behavior;
  building it against the legacy `edit_param` loop would be rework).

## Out of scope

- Changing the `RealizationCache` tolerance partial-order semantics.
- Cold-build eagerness (unchanged).
- Warm-state (`OpaqueState`) pool policy — separate machinery (`warm_pool.rs`).

## Decomposition (when activated — not filed now)

α input-cone hash recorded at execution · β recompute-then-compare seeding + first production
caller of `compute_dirty_cone_with_realizations` · γ keyed eviction replaces wholesale flush +
contract-lock supersession · δ staleness differential corpus · ε e2e: one-param slider drag
re-executes kernel ops only for the affected body (dispatch-count observable).

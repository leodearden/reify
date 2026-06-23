# PRD (forward-stub): Warm deferred full realization pass — instant-unhide + idle proactive error surfacing

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub (stays-deferred bookmark) · **Date:** 2026-06-23
**Parent:** `docs/prds/v0_6/selective-demand.md` D2 + §11 ("Low-priority deferred full realization pass …
deferred").
**Tracker:** stays-deferred bookmark task (no auto-dispatch trigger). Activate **on human judgment** when
instant-unhide latency or idle proactive error surfacing is wanted — there is no dep-gated activation.

## Why deferred

`selective-demand.md` (D2) made `self.demand` the single **selective** source of truth for warm operation,
with a synchronous full-scope override for cold `build()`/`check()` (deterministic eager errors). That
foundation deliberately did **not** build a low-priority background realization lane, because:

- The deferred full pass cannot be relied on **synchronously**, so `check()`/CI keep a sync full path
  regardless — it is strictly *additional* infrastructure, never a replacement.
- To preserve the kernel saving it must be **cancelled on every slider tick**; during active dragging it
  surfaces nothing, so its error-surfacing benefit only materialises at idle.
- It pushes a background executor + cancellation/coalescing + generalized in-flight `Pending` onto the core
  scheduler seam — its own milestone-sized concern.

But it **layers onto the selective-demand foundation with no re-architecting**: it is "after the demanded
warm tessellate completes, schedule a full-scope tessellate at low priority." Its two real wins are genuine
UX upgrades, just separable.

## Substrate gap (verified 2026-06-23)

- **No background/priority-tiered realization executor.** The unified driver
  (`run_unified_pass_seeded`, `engine_fixpoint.rs:284`) is synchronous, single-pass. The closest async +
  in-flight-`Pending` precedent is `begin_compute_dispatch` for `@optimized` ComputeNodes
  (`cache.rs:1019`) — it would need generalizing to arbitrary realizations.
- **No drag-coalescing / cancellation** for an in-flight realization wave keyed to a superseded parameter
  generation.

## Sketch (when activated)

1. After the demanded warm tessellate returns (visible bodies realized, interactive latency met), enqueue a
   **low-priority full-scope** tessellate over the complement (hidden bodies) on a background lane.
2. The background pass (a) **pre-warms hidden bodies' `RealizationCache`** → un-hide is instant, and (b)
   surfaces hidden-body realization errors into the GUI at idle (proactive, vs the parent PRD's
   "errors surface on the cold path / on un-hide").
3. **Cancellation/coalescing:** a new `edit_param` cancels the in-flight background pass for the superseded
   parameter generation (it would realize stale geometry). Compose with the parent PRD's `Pending` markers.
4. Determinism guard: the background pass NEVER becomes the error-surfacing path that `check()`/CI rely on —
   the synchronous full-scope override remains authoritative.

## Consumer (G1)

GUI warm edit loop — un-hiding a body is instant (no kernel stall), and hidden-body errors appear at idle
without an explicit `check()` or un-hide. Both are UX upgrades over `selective-demand.md`'s baseline.

## Pre-conditions for activating

- `selective-demand.md` batch landed (selective `self.demand` + demand-scoped warm tessellate + cold
  full-scope override).
- A concrete consumer need (human judgment): measured un-hide latency that hurts, or a desire for idle
  proactive error surfacing in the GUI.

## Out of scope

- Changing the synchronous cold `build()`/`check()` semantics (stays authoritative for errors).
- The selective pruning itself (owned by the parent PRD β).

## Decomposition (when activated — not filed now)

α background/priority-tiered realization lane (generalize `begin_compute_dispatch`) · β complement-scope
enqueue after the demanded warm pass · γ drag-cancellation/coalescing keyed to parameter generation ·
δ e2e: un-hide-after-idle is instant (cache pre-warmed, `last_dispatch_count == 0` on reveal) + a
hidden-body error surfaces at idle without an explicit `check()`.

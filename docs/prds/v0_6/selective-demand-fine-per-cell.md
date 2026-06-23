# PRD (forward-stub): Fine per-cell selective demand — property/constraint-panel cells as demand roots

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub (stays-deferred bookmark) · **Date:** 2026-06-23
**Parent:** `docs/prds/v0_6/selective-demand.md` D1 + §11 ("Fine per-cell demand … deferred").
**Tracker:** stays-deferred bookmark task (no auto-dispatch trigger). Activate **only on human judgment**
when a future feature or perf profile exposes a concrete benefit — there is no dep-gated activation.

## Why deferred

`selective-demand.md` enforces **coarse per-realization** demand (one `Realization` demand root per
viewport-visible body). The 4532 measurement (`docs/design/selective-demand-measurement.md` §5) showed
coarse captures the **dominant realization-kernel saving**; fine per-cell only reduces false-positive
`would_prune.value` (i.e. it keeps *displayed* property/constraint cells fresh instead of letting them go
`Pending`). Under the coarse design, displayed-but-pruned cells are handled honestly by the parent PRD's γ
(`Freshness::Pending` + last-substantive surfacing) — they show their last good value + a stale badge.
So fine per-cell is a **refinement that trades recompute for display-freshness**, not a correctness or
kernel-cost fix. No current feature requires it.

## Substrate gap (verified 2026-06-23)

- **No "displayed-cell subset" state in the GUI.** `PropertyEditor` renders *all* of
  `EngineState.values` and `ConstraintPanel` renders *all* of `EngineState.constraints`
  (`gui/src/stores/engineStore.ts:64-78`); there is no notion of which cells are actually on-screen
  (scrolled into view / panel expanded). The 4532 `displayedCells`/`panelConstraints` inputs are currently
  "every value/constraint key" (`engineStore.ts:450-464`). Fine per-cell demand requires building a real
  on-screen-subset notion first.
- The `Value`/`Constraint` `NodeId` demand roots and `add_demand`/`rebuild_cone` plumbing already exist
  (`demand.rs`); the gap is purely the GUI "what is displayed" source + the registration trigger on
  panel scroll/expand/collapse.

## Sketch (when activated)

1. Track a genuine displayed-cell / displayed-constraint subset in the GUI (viewport-of-the-panel), updated
   on scroll / expand / collapse.
2. Register those cells/constraints as **additional** demand roots on top of the coarse per-realization
   roots (the two granularities compose — register realizations first, then displayed cell ids).
3. Pruned-but-displayed cells that become demanded stop going `Pending` and stay `Final` (fresh).
4. Differential: with all cells displayed + all bodies visible, results remain byte-identical to total
   demand (reuses the parent PRD's α differential spine).

## Consumer (G1)

GUI property/constraint panels — a displayed cell of an otherwise-hidden body shows a *live* value instead
of a `Pending` last-substantive value. Marginal over the parent PRD; activate only when this freshness
upgrade is actually wanted.

## Pre-conditions for activating

- `selective-demand.md` batch landed (coarse per-realization demand + γ Pending surfacing).
- A concrete consumer need (human judgment): a feature or perf finding where `Pending`-badged displayed
  cells are an unacceptable UX, or where fine-grained pruning of value/constraint compute is measured to
  matter.

## Out of scope

- Realization-kernel pruning (owned by the parent coarse PRD).
- The `Pending` display mechanism (owned by the parent PRD γ).

## Decomposition (when activated — not filed now)

α displayed-subset tracking in the GUI · β register displayed cells/constraints as demand roots + trigger on
panel change · γ differential gate (all-displayed == total) · δ e2e: a displayed cell of a hidden body stays
`Final` (live), not `Pending`.

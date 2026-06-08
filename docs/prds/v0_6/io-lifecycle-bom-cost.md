# PRD (forward-stub): `Buy`/`Discard`/`Provenance` lifecycle eval (BOM / cost / waste)

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub · **Date:** 2026-06-03
**Parent:** `io-export-import-completion.md` §8 (deferred row 3). **Tracker:** task λ.

## Why deferred

Closes gap-register P15 `io-trait-surface-only-no-eval`: the `Source`/`Sink`/`Buy`/`Discard`/
`Provenance` surface in `io.ri` is **compile-time only** — no eval path reads `Buy.unit_cost`,
`Discard.reason`, or `Provenance` fields, or rolls them up. The parent PRD wires the **export/import**
half of `std.io` (the `Output`/`Input` occurrences); this lifecycle half is deferred because it has
**no consumer surface today** — building it now would be a producer-orphan (G1). It is really a
**reporting** PRD (BOM, cost roll-up, waste/recyclability report), not an io-format PRD.

## Substrate (verified 2026-06-03)

- `io.ri` already declares `Buy` (supplier/part_number/unit_cost/lead_time), `Discard`
  (reason/disposal_method), `Costed : Buy` (with `let line_cost = unit_cost * quantity_produced`),
  and `Provenance`. `examples/cost_aggregation.ri` shows the intended BOM idiom in DSL — but nothing
  *evaluates a report* across a design's `Buy`/`Discard` occurrences.
- `Money` dimension + `line_cost` money-aggregation already work at eval (tasks 2377/2380/2381).

## Sketch (when activated)

1. Define the **consumer surface first** (G1): a `reify report --bom <f>.ri` CLI emitting a
   cost/BOM/waste table, or an analysis result value samplable from the design. Without it, do not
   build.
2. An eval pass that enumerates `Buy`/`Costed`/`Discard` occurrence instances and aggregates
   `unit_cost`/`line_cost`/`reason` into a report value.
3. Provenance read-out (import audit trail) ties into the parent's geometry-import provenance.

## Pre-conditions for activating

- A **named consumer** (the report CLI / result surface) — the blocking G1 question.
- Parent `io-export-import-completion.md` landed (shared io.ri occurrence-enumeration substrate
  the export driver introduces is reusable for occurrence roll-ups).

## Decomposition (when activated — not filed now)

α report consumer surface (CLI/result) · β `Buy`/`Costed` cost roll-up eval · γ `Discard`/waste
report · δ `Provenance` audit read-out · ε `.ri` BOM example in CI.

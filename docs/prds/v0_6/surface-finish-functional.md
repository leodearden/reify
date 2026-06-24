# PRD (forward-stub): Functional surface finish / treatment / coating

**Milestone:** v0_6+ (deferred capstone) · **Status:** DEFERRED forward-stub · **Date:** 2026-06-24
**Parents (cosmetic precursor):** `appearance-substrate.md` + `appearance-viewport-egress.md` (the cosmetic
appearance system). **Umbrella:** task 4291. **Tracker:** a PENDING `[MILESTONE]` task that escalates this
PRD's authoring + decomposition to L2 on dispatch.

## Why deferred (and why the trigger exists now)

The cosmetic appearance system (PRD-1 `appearance-substrate`, PRD-2 `appearance-viewport-egress`) ships a
**display-only** notion of finish: `Finish { Matte, Satin, Gloss }` and a `Color` (rgb + optional named
standard like RAL9001) drive the viewport and 3MF color — *purely how a part looks*. Real **surface finish,
treatment, and coating** is a deep, spec-bearing engineering subject (Leo, 2026-06-24): RAL9001 is a real
paint; "Gloss" is a real finish; anodize/plate/passivate/heat-treat are real processes with real
consequences. That belongs in its own PRD (or PRDs), not folded into the cosmetic slice.

This stub + its `[MILESTONE]` tracker is the **design-it-now trigger**: the tracker stays PENDING, dep-wired
on the cosmetic-appearance leaves, and on dispatch **escalates to L2** to author + decompose this PRD —
rather than letting the functional model be forgotten or back-filled ad hoc.

## The subsumption relationship (the load-bearing forward-compat invariant)

PRD-1 established that **`Appearance` is the stable, source-agnostic, renderer/export-facing contract**, and
made *materials* its first producer; PRD-2 added the *`DisplayOutput.style` display override* as a second
source. The functional-finish model becomes a **third producer of `Appearance`** and **subsumes the cosmetic
`Finish` enum** into a richer cosmetic+functional definition — without changing the `Appearance` contract the
renderer/export already consume. The cosmetic `Finish { Matte, Satin, Gloss }` becomes a *projection* of (or
is replaced by) a functional `SurfaceFinish`/`Coating` that *also* yields the cosmetic look.

## Scope sketch (when activated — NOT decomposed now)

Functional surface finish / treatment / coating as model-level, spec-bearing properties of a part, each of
which also *produces* an `Appearance` (so the cosmetic look stays automatic):

- **Surface finish**: roughness (Ra/Rz), lay/direction, machining vs. ground vs. polished.
- **Coating / plating / paint**: type + thickness + process (anodize, powder-coat, electroplate, passivate,
  paint with a *real* RAL/Pantone spec), with the appearance derived from the coating.
- **Treatment**: heat treatment, case hardening, shot peening — and their interaction with the
  mechanical/material model (hardness, residual stress) where relevant.
- **Downstream consequences**: mass/cost/BOM roll-up (ties to the deferred `io-lifecycle-bom-cost.md`),
  drawing/GD&T callouts (ties to `gdt-*`), and richer 3MF/STEP material+color/appearance export.

## Pre-conditions for activating

1. PRD-1 `appearance-substrate` shipped — concretely **task 4763** (δ: 3MF per-body color egress) done:
   cosmetic color proven on the export surface + the `Appearance` contract + material `Visual` trait landed.
2. PRD-2 `appearance-viewport-egress` shipped — concretely **task 4775** (ε: dev-GUI integration gate) done:
   cosmetic appearance + `DisplayOutput.style` override + precedence proven end-to-end in the viewport.

Both being done means the cosmetic `Appearance` contract is stable and exercised on *both* the export and
viewport surfaces — the thing the functional model must subsume without breaking.

## G4 — cross-PRD relationship

| Other PRD / seam | Direction | Mechanism | Owner |
|---|---|---|---|
| `appearance-substrate.md` / `appearance-viewport-egress.md` | subsumes | the cosmetic `Finish`/`Color` → projected from the functional finish; `Appearance` contract unchanged | **this** PRD re-owns finish; cosmetic PRDs keep the contract |
| `io-lifecycle-bom-cost.md` (deferred stub) | may produce | coating/treatment cost + mass + BOM roll-up | coordinate at authoring |
| `gdt-*` / drawing callouts | may produce | finish/treatment as a drawing-callout property | coordinate at authoring |

## Decomposition

Not filed now. On the trigger firing (deps met → dispatch → **escalate to L2**), a human authors + decomposes
this PRD via `/prd`, running the full G1–G6 gates against then-current substrate.

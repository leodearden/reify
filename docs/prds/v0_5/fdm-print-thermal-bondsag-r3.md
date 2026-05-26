# PRD: FDM Print Model — Rung 3 (transient thermal, bridge-sag, flow-resolved orientation, multi-material)

Status: **stub — deferred, research-grade.** Top refinement rung of `fdm-as-printed-fea.md`. Filed 2026-05-26 from the umbrella's fidelity ladder. Activates only on concrete demand after R2 ships; the most speculative rung.

## Goal

The high-fidelity endpoint: a spatially-resolved transient simulation of the print that R1's cheap analytical thermal/orientation models approximate. Captures (a) a transient thermal-history field (element birth/activation, conduction/convection/radiation) that drives accurate per-interface bond strength and residual stress; (b) bridge-sag as quantitative deflected geometry; (c) flow-resolved (Folgar-Tucker) fibre orientation through the nozzle; (d) dissimilar-material / multi-nozzle interface bonding. Registers as the R3 rung of the umbrella's `as_printed_material` progressive node — the sharpest, slowest result the user's compute budget can reach.

## Background

Per the 2026-05-26 research these are the genuinely **research-grade** pieces, flagged explicitly as harder than R0–R2:
- **Full-part transient thermal history** — mesh activation (element birth-death), convective/radiative BCs; validation is hard. (Digimat-AM and the ORNL/UMaine BAAM workflow are the reference architectures; ANSYS APDL birth-death the technique.)
- **Bridge-sag** — mostly empirical/qualitative in the literature; no clean validated predictive model. Expect a coarse viscous-beam approximation, flagged as approximate.
- **Flow-resolved fibre orientation** — Folgar-Tucker through the converging nozzle; do-able but heavy. R1's bead-aligned-orientation shortcut is what most FDM work actually uses.
- **Dissimilar-material welding** — reptation/healing models assume self-diffusion; cross-polymer welding is far less established.

This rung is large FE + coupled physics; it should not be decomposed until a concrete user need justifies the cost.

## Sketch of approach

- Transient thermal FE with element activation along the toolpath deposition timeline → T(t) field; feeds the R1 bond/sintering models with a real (not lumped) history, plus residual-stress recovery.
- Coarse viscous/viscoelastic bridge-sag deflection of unsupported spans during cooling → as-printed geometry deviation.
- Folgar-Tucker fibre-orientation evolution through the nozzle → orientation tensor field (supersedes R1's bead-aligned assumption).
- Multi-material / multi-nozzle interface bonding (cross-polymer) → per-interface dissimilar-weld strength.

## Pre-conditions for activating

- `fdm-print-rve-homogenization-r2.md` shipped (R2 effective-tensor machinery; R3 feeds it a thermal-/sag-/orientation-corrected mesostructure).
- `anisotropic-heterogeneous-elastostatics.md` shipped.
- A transient thermal FE capability (large net-new kernel surface — the dominant cost of this rung).
- **Documented concrete demand** — this rung waits for a real use case, like `composite-laminated-shells.md` waits for composite demand.

## Relationship to other PRDs

- **Top refinement rung of `fdm-as-printed-fea.md`** — registers as R3; umbrella owns the progressive mechanism, this PRD owns the transient physics.
- **Above `fdm-print-rve-homogenization-r2.md`** — supplies thermal-/sag-/orientation-corrected inputs to R2's homogenisation.
- **May seed a residual-stress / warpage PRD** — the transient thermal field also predicts warpage, a natural sibling.
- **Multi-material bonding may seed a dissimilar-weld-strength PRD** — cross-polymer welding is its own research domain.

## Out of scope

- Metal-AM (LPBF/DED) meltpool/grain simulation — a separate domain (ANSYS Additive territory), not polymer FDM.
- Print-failure prediction (warping-induced detachment, etc.) — downstream of the residual-stress field.
- Real-time / in-process control coupling — unrelated to as-printed structural analysis.

## Open design questions

- Thermal FE solver: reuse `reify-solver-elastic` assembly machinery for the conduction operator vs a dedicated transient solver. Defer.
- Bridge-sag fidelity floor: ship the coarse viscous-beam approximation with an explicit accuracy caveat, or wait for a validated model. Lean: coarse + caveat (some signal beats none).
- Whether R3 is one PRD or splits (transient-thermal / bridge-sag / flow-orientation / multi-material as separate activations). Likely splits on activation; kept as one stub until demand shapes it.

# PRD: FDM Print Model — Rung 1 (analytical mesostructure + orientation micromechanics + bond model)

Status: **stub — deferred.** Sibling/refinement rung of `fdm-as-printed-fea.md`. Filed 2026-05-26 from the umbrella's fidelity ladder. Activates after the umbrella's R0 slice ships.

## Goal

Refine the `FDMPrint` as-printed material field from R0's closed-form correlations to an analytical **mesostructure** model: explicit bead/void geometry, orientation-resolved micromechanics for fibre-filled filaments, and a thermal-history-driven inter-bead/inter-layer **bond strength** model — all still analytical/ODE (no large FE). Wires into the umbrella's `as_printed_material` progressive node as the R1 rung; users on a part with fibre-filled filament or where interlayer bonding dominates see the as-printed FEA result sharpen automatically over R0.

## Background

Per the 2026-05-26 prior-art research, the FDM bonding picture is a well-cited analytical stack that is engineering-tractable *given a thermal history*:
- **Bead/void geometry** — oval/stadium cross-section + neck width → periodic void lattice.
- **Inter-bead/inter-layer bonding** — Frenkel/Pokluda sintering neck-growth ODE (Bellehumeur 2004; Sun 2008) + de Gennes/Wool reptation healing integral over the interface thermal history (Coogan & Kazmer 2017/2020). Build-Z is weakest because inter-layer interfaces spend least time above the healing temperature.
- **Intra-filament anisotropy** — short-carbon-fibre orientation aligned to the bead by extrusion; modelled with Halpin-Tsai (cheap) or Mori-Tanaka + Advani-Tucker orientation averaging (HomoPy is the Python equation reference; port to Rust). Core/skin bicomponent filaments = concentric two-phase rule-of-mixtures.
- **Thermal history** at R1 is a cheap lumped (Sun) / 1-D interface conduction (Coogan) model — *not* a full transient field (that is R3).

No off-the-shelf OSS engine exists; the work is a clean-room Rust implementation of published equations.

## Sketch of approach

- Bead cross-section + neck/void geometry model from `Toolpath` bead widths/spacing.
- Sintering ODE + reptation healing integral → per-interface degree-of-healing and cohesive/weld strength; the dominant build-Z knockdown becomes *computed* (not a tabulated ratio).
- Halpin-Tsai / Mori-Tanaka + Advani-Tucker orientation averaging for SCF; concentric rule-of-mixtures for core/skin → per-bead anisotropic lamina stiffness.
- Compose into the `AnisotropicMaterial` field the umbrella's solver consumes; orientation frame from bead direction + build-Z.

## Pre-conditions for activating

- `fdm-as-printed-fea.md` R0 slice shipped (the `Toolpath` type, the `as_printed_material` progressive node, the FDMPrint R0 rung).
- `anisotropic-heterogeneous-elastostatics.md` shipped (constitutive field consumer).
- A lumped/1-D thermal-history component (this PRD ships the cheap version; R3 supersedes with a transient field).

## Relationship to other PRDs

- **Refinement rung of `fdm-as-printed-fea.md`** — registers as the R1 rung of `as_printed_material`; the umbrella owns the progressive mechanism, this PRD owns the R1 physics.
- **Below `fdm-print-rve-homogenization-r2.md`** — R2 replaces R1's closed-form/orientation-averaged stiffness with numerical RVE homogenisation of the same bead/void geometry.
- **Consumes `anisotropic-heterogeneous-elastostatics.md`** — emits the same `AnisotropicMaterial` field type.
- **Shares micromechanics with `composite-laminated-shells.md`** — Mori-Tanaka/Advani-Tucker and orthotropic constants overlap; reuse the foundation's `ConstitutiveLaw` surface.

## Out of scope

- Full transient thermal field / element-birth simulation — `fdm-print-thermal-bondsag-r3.md`.
- Numerical RVE FE — `fdm-print-rve-homogenization-r2.md`.
- Flow-resolved (Folgar-Tucker) fibre orientation — R3; R1 assumes bead-aligned orientation tensors.
- Dissimilar-material weld bonding (cross-polymer) — R3; R1 covers self-diffusion bonding.
- Quantitative bridge-sag geometry — R3.

## Open design questions

- Bead cross-section model: oval vs stadium vs hypotrochoid void parameterisation. Lean: oval + neck width (cheapest defensible).
- Whether weld strength feeds elastic stiffness only (R1 scope) or also a strength/failure field (defer to a strength PRD).
- Orientation-tensor source: assumed bead-aligned `a₁₁≈0.7–0.9` (R1) vs flow-solved (R3).

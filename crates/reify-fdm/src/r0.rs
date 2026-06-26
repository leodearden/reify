// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM as-printed constitutive mapping — **R0 fidelity rung** (task θ / 3790).
//!
//! Where the R-fast rung (task δ, `reify-eval` `as_printed_material`) derives
//! per-zone effective properties from the stdlib `FDMProcess` alone
//! (Gibson-Ashby infill knockdown + a fixed build-Z modulus ratio of 0.67),
//! the R0 rung maps a **real sliced [`Toolpath`]** (task ζ,
//! [`crate::toolpath`]) to per-zone orthotropic constants using closed-form
//! physics computed from the *measured* deposition:
//!
//! - **Rodríguez 2003 orthotropic** ([`rodriguez_orthotropic`]) — the FDM
//!   mesostructure (continuous bead axis vs inter-bead necks/voids vs
//!   inter-layer bonds) gives a genuine `E1 > E2 > E3` ordering: stiffest
//!   along the bead, knocked down transverse in-plane, weakest in build-Z.
//! - **Halpin-Tsai fibre** ([`halpin_tsai_modulus`] /
//!   [`halpin_tsai_reinforced`]) — short-fibre stiffening of the base
//!   filament. Opt-in; **inert by default** (`vol_fraction = 0` returns the
//!   matrix modulus exactly), since the stdlib `FDMProcess` carries no fibre
//!   fields yet.
//! - **Lumped-cooling build-Z knockdown** ([`lumped_cooling_z_ratio`]) — the
//!   R0 replacement for R-fast's fixed `0.67`: a lumped-capacitance cooling
//!   model converts the interface deposition temperature and the inter-layer
//!   time into an inter-layer bond fraction `∈ (0, 1)` that scales `E3`.
//!
//! [`r0_region_materials`] buckets the toolpath beads by [`BeadRole`] into the
//! wall / skin / infill zones, measures each zone's mean width / height /
//! nominal temperature + dominant bead-centerline direction, and composes the
//! three laws into a per-zone [`OrthotropicConstants`] plus the frame x-axis
//! (the dominant local bead direction). The downstream θ `FDMPrint` trampoline
//! (`reify-eval`) wraps these into the `AsPrintedZones` material field.
//!
//! # Units
//!
//! [`crate::toolpath`] stores native G-code **millimetres**; this module owns
//! the **mm → SI** conversion (its module doc explicitly delegates it here).
//! Lengths exposed to the field (mean widths/heights, the toolpath AABB) are
//! converted to metres; constitutive moduli stay SI throughout (base material
//! Pa × dimensionless R0 factors).

// Public symbols are added incrementally by the θ implementation steps
// (rodriguez_orthotropic, halpin_tsai_modulus, lumped_cooling_z_ratio,
// r0_region_materials, …) and re-exported from `lib.rs`.

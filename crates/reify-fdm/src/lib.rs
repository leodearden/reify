// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM (fused deposition modeling) as-printed FEA support — R-fast tier.
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` for the umbrella PRD. This
//! crate hosts two pure, dependency-free pieces of the FDM as-printed model:
//!
//! - [`zone`] (task γ): the geometric zone classifier — a pure algorithmic
//!   mapping from per-point distance probes to a Wall / Skin / Infill zone,
//!   parameterised by the mechanically relevant fields of the stdlib
//!   `FDMProcess` structure.
//! - [`correlation`] (task β): the effective-property correlation library —
//!   turns a base filament material plus infill density/pattern into the
//!   foundation's transverse-isotropic / orthotropic constitutive constants,
//!   with coupon-override entry points.
//! - [`toolpath`] (task ζ): the structured [`Toolpath`] value type + the
//!   PrusaSlicer G-code-comment parser that builds it. This is the one module
//!   with an intra-workspace dependency — it reuses `reify-gcode`'s low-level
//!   `parse_marlin` for move lines (see the module doc for why the parser
//!   walks comments itself rather than calling `parse_marlin` on the whole
//!   source).
//!
//! Apart from the `toolpath` module's intra-workspace `reify-gcode` leaf-parser
//! dependency, the crate has **no** external dependencies: `zone`,
//! `correlation`, and `as_printed` remain pure `f64` / `[f64; 3]` code.
//! Real-body integration (OCCT distance queries, `Field<Point3,
//! AnisotropicMaterial>` production, `ComputeNode` registration) is owned by
//! the downstream δ-task, which consumes the classifier/correlation modules; γ
//! ships the classifier in isolation with an analytic `AxisAlignedBox` helper
//! that the integration test exercises.

pub mod zone;

pub mod correlation;

pub mod as_printed;

pub mod toolpath;

// task η — PrusaSlicer subprocess invocation core (discover / compose / run /
// slice_body). Symbols are re-exported as they land across steps 2–12.
pub mod slice;

// task θ — R0 constitutive mapping (Toolpath → orthotropic per-zone constants).
pub mod r0;

pub use r0::{
    Fibre, R0Options, R0Region, R0RegionMaterials, RasterMesostructure, halpin_tsai_modulus,
    halpin_tsai_reinforced, lumped_cooling_z_ratio, r0_region_materials, rodriguez_orthotropic,
};

pub use zone::{
    AxisAlignedBox, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, Zone, ZoneProbe, ZoneProcessParams,
    classify_zone, is_top_or_bottom_normal,
};

pub use as_printed::{
    Rung, classify_point, material_constants_at, orthotropic_constants_at, select_rungs,
    zone_solid_fraction,
};

pub use correlation::{
    BUILD_Z_MODULUS_RATIO, BUILD_Z_MODULUS_RATIO_PROVENANCE, BUILD_Z_STRENGTH_RATIO,
    BUILD_Z_STRENGTH_RATIO_PROVENANCE, BaseElastic, CorrelationProvenance, CouponOverride,
    DIRECTIONAL_FACTOR_PROVENANCE, DIRECTIONAL_STRONG_FACTOR, DIRECTIONAL_WEAK_FACTOR,
    GIBSON_ASHBY_C, GIBSON_ASHBY_C_PROVENANCE, GIBSON_ASHBY_N, GIBSON_ASHBY_N_PROVENANCE,
    InfillPattern, NEAR_ISOTROPIC_FACTOR, OrthotropicConstants, PatternFactors,
    TransverseIsoConstants, effective_orthotropic, effective_transverse_isotropic,
    gibson_ashby_infill_factor, pattern_factors,
};

// task ζ — Toolpath value type + PrusaSlicer parser.
pub use toolpath::{
    Bead, BeadRole, Layer, Toolpath, ToolpathParseError, parse_prusaslicer_gcode,
    role_from_prusaslicer_type,
};

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
//!
//! The crate intentionally has **no** external dependencies. Real-body
//! integration (OCCT distance queries, `Field<Point3, AnisotropicMaterial>`
//! production, `ComputeNode` registration) is owned by the downstream
//! δ-task, which consumes both modules; γ ships the classifier in isolation
//! with an analytic `AxisAlignedBox` helper that the integration test
//! exercises.

pub mod zone;

pub mod correlation;

pub use zone::{
    AxisAlignedBox, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, Zone, ZoneProbe, ZoneProcessParams,
    classify_zone, is_top_or_bottom_normal,
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

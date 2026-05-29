// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM (fused deposition modeling) as-printed FEA support — R-fast tier.
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` for the umbrella PRD. This
//! crate currently hosts the geometric zone classifier (task γ): a pure
//! algorithmic mapping from per-point distance probes to a Wall / Skin /
//! Infill zone, parameterised by the mechanically relevant fields of the
//! stdlib `FDMProcess` structure.
//!
//! The crate intentionally has **no** external dependencies. Real-body
//! integration (OCCT distance queries, `Field<Point3, AnisotropicMaterial>`
//! production, `ComputeNode` registration) is owned by the downstream
//! δ-task; γ ships the classifier in isolation with an analytic
//! `AxisAlignedBox` helper that the integration test exercises.

pub mod zone;

mod correlation;

pub use zone::{
    AxisAlignedBox, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, Zone, ZoneProbe, ZoneProcessParams,
    classify_zone, is_top_or_bottom_normal,
};

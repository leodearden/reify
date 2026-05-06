//! Shared types that are always compiled regardless of whether OCCT is
//! available at build time.
//!
//! Re-exported from the crate root so that call sites compile under both
//! `has_occt` and `!has_occt` without `#[cfg]` noise.

/// Curvature properties at a parametric point on a face surface.
///
/// Returned by [`crate::OcctKernel::curvature_at`]. All direction vectors
/// are unit-length tangent vectors lying in the tangent plane at `(u, v)`.
///
/// Defined in this always-compiled module (not gated on `has_occt`) so that
/// both the real kernel (`lib.rs`) and the stub kernel (`stubs.rs`) share
/// exactly one definition and cannot silently drift out of sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curvature {
    /// Gaussian curvature K = κ₁·κ₂. Invariant under normal-direction reversal.
    pub gaussian: f64,
    /// Mean curvature H = (κ₁ + κ₂) / 2. Sign follows the outward normal
    /// convention (negated for `TopAbs_REVERSED` faces).
    pub mean: f64,
    /// Minimum principal curvature κ_min ≤ κ_max.
    pub kappa_min: f64,
    /// Maximum principal curvature κ_max ≥ κ_min.
    pub kappa_max: f64,
    /// Principal direction corresponding to κ_min (unit tangent vector).
    pub dir_min: [f64; 3],
    /// Principal direction corresponding to κ_max (unit tangent vector).
    pub dir_max: [f64; 3],
}

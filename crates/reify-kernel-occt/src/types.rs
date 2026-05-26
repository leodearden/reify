//! Shared types that are always compiled regardless of whether OCCT is
//! available at build time.
//!
//! Re-exported from the crate root so that call sites compile under both
//! `has_occt` and `!has_occt` without `#[cfg]` noise.

/// Rigid-body transform encoding a unit-quaternion rotation and translation.
///
/// Mirrors `Value::Transform { rotation: Orientation { w, x, y, z }, translation: Vector }`
/// from `crates/reify-types/src/value.rs`. Defined in this always-compiled module so it
/// is importable under both `has_occt` and `!has_occt` builds without `#[cfg]` noise.
///
/// Field order: `{ qw, qx, qy, qz, tx, ty, tz }` — quaternion scalar-first
/// (`qw + qx·i + qy·j + qz·k`); translation last.
///
/// **Invariant**: the quaternion `(qw, qx, qy, qz)` must be a unit quaternion
/// (`|q|² ∈ [1-1e-6, 1+1e-6]`). The C++ `build_trsf` helper validates this and
/// returns `QueryError::QueryFailed` if the norm deviates — passing a non-unit
/// quaternion (e.g. from accumulated float drift in a kinematic chain) silently
/// produces a non-rigid `gp_Trsf`, so callers must normalise before constructing
/// a `Transform3`.
///
/// On the C++ side, OCCT's `gp_Quaternion` constructor takes `(x, y, z, w)`, so the
/// single point of field-order translation is the explicit `gp_Quaternion(t.qx, t.qy,
/// t.qz, t.qw)` line in `cpp/occt_wrapper.cpp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform3 {
    /// Quaternion scalar (w) component.
    pub qw: f64,
    /// Quaternion x component.
    pub qx: f64,
    /// Quaternion y component.
    pub qy: f64,
    /// Quaternion z component.
    pub qz: f64,
    /// Translation x component (millimetres, same unit as Reify geometry).
    pub tx: f64,
    /// Translation y component.
    pub ty: f64,
    /// Translation z component.
    pub tz: f64,
}

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

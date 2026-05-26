//! FFI-level helpers for transform-aware geometry queries.
//!
//! This module provides `pub(crate)` wrappers that call into the cxx-bridge
//! and map FFI errors to [`reify_types::QueryError`].
//!
//! Compiled only when `has_occt` is set (the C++ FFI exists).
//! The public API surface exposed to integration tests is the handle-based
//! [`crate::OcctKernel`] methods that call these helpers.
//!
//! **Scope**: PRD kinematic-constraints §6.2 + §9.2, task 3841.

use crate::ffi::ffi::{OcctShape, Transform3Props};
use crate::Transform3;
use reify_types::QueryError;

/// Conversion from `Transform3` (always-compiled public type) to the cxx-bridge
/// `Transform3Props` POD (available only under `has_occt`).
///
/// Both structs have identical field order `{ qw, qx, qy, qz, tx, ty, tz }`, so
/// this `From` impl is the single enforced coupling point — any future field addition
/// must touch both definitions (the compiler will catch a missing initialiser here).
impl From<&Transform3> for Transform3Props {
    fn from(t: &Transform3) -> Self {
        Transform3Props {
            qw: t.qw,
            qx: t.qx,
            qy: t.qy,
            qz: t.qz,
            tx: t.tx,
            ty: t.ty,
            tz: t.tz,
        }
    }
}

/// Minimum BREP distance between `a` and `b` after pre-composing `t_rel` into
/// the cheaper-by-topology side.
///
/// See [`crate::OcctKernel::distance_with_transform`] for the full contract.
pub(crate) fn distance_with_transform(
    a: &OcctShape,
    b: &OcctShape,
    t: &Transform3,
) -> Result<f64, QueryError> {
    crate::ffi::ffi::distance_with_transform(a, b, &Transform3Props::from(t))
        .map_err(|e| QueryError::QueryFailed(e.to_string()))
}

/// Probe whether `a` and `b` interfere after pre-composing `t_rel` into the
/// cheaper-by-topology side.
///
/// Returns `true` iff `dist_with_pre_compose(a, b, t_rel) <= 0.0`.
///
/// See [`crate::OcctKernel::interferes_with_transform`] for the full contract.
pub(crate) fn interferes_with_transform(
    a: &OcctShape,
    b: &OcctShape,
    t: &Transform3,
) -> Result<bool, QueryError> {
    crate::ffi::ffi::interferes_with_transform(a, b, &Transform3Props::from(t))
        .map_err(|e| QueryError::QueryFailed(e.to_string()))
}

//! The `sweep` degenerate-pose guard: [`validate_sweep_pose`] and the angular
//! band it enforces.

use reify_ir::{BRepKind, GeometryError};

use crate::ffi;
use crate::shape_store::brep_kind_of_shape;

/// Minimum angle, in degrees, between a `sweep` path's start tangent and its
/// profile's normal. Below this the profile is posed in (or effectively in) the
/// spine's own plane and the sweep is refused.
///
/// This is the ONE place the band is written. The predicate takes its sine and
/// the error text quotes the degrees, so the number a designer is told and the
/// number they are measured against cannot drift apart.
///
/// It is a band rather than exact coplanarity because the degeneracy is
/// continuous: OCCT builds a valid sliver solid down to 0.001 degrees
/// off-plane, so an exact predicate is evadable by float noise.
/// `crates/reify-eval/tests/harness_sweep/sweep_pose_guard_e2e.rs` pins the
/// band on both sides: its rungs 2 and 2b straddle it at ±0.2 degrees, and
/// rung 2c is the nearest real keeper.
const SWEEP_DEGENERATE_POSE_DEG: f64 = 4.0;

/// Normalise a vector, or `None` when it carries no usable direction.
///
/// `None` covers every non-finite component — a NaN or infinite component makes
/// the length non-finite — as well as a vector too short to normalise.
fn unit_vector(v: ffi::ffi::Point3) -> Option<(f64, f64, f64)> {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if !len.is_finite() || len < 1e-12 {
        return None;
    }
    Some((v.x / len, v.y / len, v.z / len))
}

/// Reject a `sweep` whose profile is posed within [`SWEEP_DEGENERATE_POSE_DEG`]
/// of the path's own plane.
///
/// `sweep` transports the profile AS POSED — deliberately, since designs pose
/// their sections themselves — so a section lying in the spine's plane sweeps
/// edge-on. The swept volume is exactly `(profile area) · (displacement · n̂)`,
/// so at 0 degrees it is 0 and just off 0 it is a sliver; OCCT builds both
/// without complaint. This guard is what turns that silence into a diagnostic.
///
/// # Fails OPEN
///
/// Returns `Ok(())` whenever the pose predicate is UNDEFINED rather than
/// satisfied: the profile is not a PLANAR `Face`, either FFI query errors, or
/// either vector carries no direction. Planarity is part of that test, not a
/// refinement of it: `query_face_normal` samples the normal at the face's AREA
/// CENTROID only, so on a curved face — reachable via a `let`-bound opaque
/// profile, which the compiler's Closed-and-Planar precondition does not
/// inspect — that one sample describes no plane and `|t̂·n̂|` is not the
/// quantity the refusal would be claiming to report.
///
/// The fences that pin this fail-open scope are
/// `sweep_with_history_fails_open_for_a_wire_profile` and
/// `a_non_planar_face_profile_is_not_judged_on_pose`, in
/// `crates/reify-kernel-occt/tests/harness_occt/sweep_pose_guard_integration.rs`.
pub(crate) fn validate_sweep_pose(
    profile_shape: &ffi::ffi::OcctShape,
    path_shape: &ffi::ffi::OcctShape,
) -> Result<(), GeometryError> {
    if !matches!(brep_kind_of_shape(profile_shape), Ok(BRepKind::Face)) {
        return Ok(());
    }
    let profile_is_planar = ffi::ffi::face_surface_kind(profile_shape).is_ok_and(|kind| {
        matches!(
            reify_ir::FaceSurfaceKind::try_from_str(&kind),
            Ok(reify_ir::FaceSurfaceKind::Plane)
        )
    });
    if !profile_is_planar {
        return Ok(());
    }

    let (Ok(normal), Ok(tangent)) = (
        ffi::ffi::query_face_normal(profile_shape),
        ffi::ffi::wire_start_tangent(path_shape),
    ) else {
        return Ok(());
    };
    let (Some(n), Some(t)) = (unit_vector(normal), unit_vector(tangent)) else {
        return Ok(());
    };

    let abs_dot = (t.0 * n.0 + t.1 * n.1 + t.2 * n.2).abs();
    if abs_dot < SWEEP_DEGENERATE_POSE_DEG.to_radians().sin() {
        let off_plane_deg = abs_dot.asin().to_degrees();
        let (tx, ty, tz) = t;
        return Err(GeometryError::OperationFailed(format!(
            "sweep profile is posed (nearly) edge-on to the path: the path's \
             start tangent ({tx:.6}, {ty:.6}, {tz:.6}) lies within \
             {SWEEP_DEGENERATE_POSE_DEG} degrees of the profile's plane, so the \
             swept body encloses almost nothing (nothing at all at 0 degrees). \
             Measured |tangent . normal| = {abs_dot:.9}, i.e. {off_plane_deg:.3} \
             degrees off-plane, where at least {SWEEP_DEGENERATE_POSE_DEG} \
             degrees is required. Fix: pose the profile normal along that \
             start tangent. The rotation depends on the tangent above; for \
             a +Y tangent it is `sweep(rotate(circle(r), 1.0, 0.0, 0.0, \
             -90deg), path)`."
        )));
    }
    Ok(())
}

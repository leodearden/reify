//! Per-op argument decode and validation shared by `OcctKernel::execute` and
//! the `*_with_history` primitives.

use reify_ir::{GeometryError, GeometryOp, QueryError, Value};

use crate::ffi::ffi::Point3;

/// Minimum squared magnitude for axis/direction vectors.
///
/// Vectors with mag² below this threshold are treated as zero-length and rejected.
/// This catches physically meaningless axes (e.g. sub-micrometer) while allowing
/// any vector representable in normal CAD geometry.
///
/// Value: 1e-12 → minimum magnitude ~1e-6 → ~1 micrometer.
pub(crate) const AXIS_MAG_SQ_MIN: f64 = 1e-12;

/// Minimum absolute angle (radians) for revolve operations.
/// Angles below this are treated as effectively zero.
/// Matches the C++ ANGLE_ABS_MIN threshold (1e-30).
/// Value: 1e-30 radians ≈ 5.7e-29 degrees — far below any physical relevance.
pub(crate) const ANGLE_ABS_MIN: f64 = 1e-30;

/// Extract an f64 from a Value (Int, Real, or Scalar → SI value).
///
/// Length-semantic fields go through [`extract_length_f64`] instead. This
/// context-free form survives for the FIVE deliberately ungated OCCT fields —
/// `HalfSpace`'s `nx`/`ny`/`nz` (dimensionless unit-normal components) and
/// `CircularPattern.angle` / `Draft.angle` (ANGLE, PRD 3's surface) — each
/// marked at its call site with a `// not length-semantic:` comment.
pub(crate) fn extract_f64(v: &Value) -> Result<f64, GeometryError> {
    v.as_f64()
        .ok_or_else(|| GeometryError::OperationFailed("expected numeric value".into()))
}

/// Length-semantic numeric extraction + the C4 kernel LENGTH tripwire.
///
/// Identical accept/reject disposition and `Ok` payload to [`extract_f64`];
/// the only differences are (a) a `tracing::warn!` naming op kind and field
/// when a non-LENGTH value arrives, (b) the opt-in debug assertion, and (c) an
/// `Err` string that names op kind and field instead of the bare
/// `"expected numeric value"`.
///
/// **A tripwire, not a gate** (PRD D5 / ratified decision 4): a violation is
/// *reported* and execution proceeds exactly as before. A fired tripwire means
/// a hole in the eval-layer gate (`required_length_value` /
/// `required_length_values`, `crates/reify-eval/src/geometry_ops.rs:482`/`:523`),
/// not a kernel bug. Canonical rationale, plus the C2 corollary about
/// `GeometryOp` construction routes, lives with the shared classifier in
/// `crates/reify-ir/src/kernel_validation.rs`. Contract:
/// `docs/prds/v0_6/units-length-gate-completion.md` C4/D5, boundary rows 13-14.
///
/// It is the SECOND, INDEPENDENT detection layer: the first is the closure
/// guard (leaf ι), which is not yet landed. The closure guard reasons about
/// where a value came from; this observes what actually arrived at the kernel
/// boundary. Neither subsumes the other, so this one keeps working if the
/// first is bypassed or has a hole.
///
/// `OcctKernel::execute`'s 47 numeric-extraction sites split 47 = 42 + 3 + 2:
/// the 42 LENGTH-semantic ones come here, while `HalfSpace`'s `nx`/`ny`/`nz`
/// (dimensionless unit-normal components) and `CircularPattern.angle` /
/// `Draft.angle` (ANGLE — PRD 3's surface) stay on the context-free
/// [`extract_f64`], each marked at its call site with a
/// `// not length-semantic:` comment.
///
/// The warn is emitted HERE rather than from `reify-ir` because the house
/// pattern for a kernel diagnostic is a `tracing::warn!` whose `target:` names
/// the emitting crate (`reify_kernel_gmsh::repair`,
/// `reify_kernel_manifold::kernel`), and because it keeps `reify-ir` — a
/// dependency of 13+ crates — free of a `tracing` edge. The message string
/// still comes from the single `kernel_validation.rs` formatter, so the two
/// kernels cannot drift.
pub(crate) fn extract_length_f64(
    v: &Value,
    op: &GeometryOp,
    field: &'static str,
) -> Result<f64, GeometryError> {
    let op_kind = op.kind_name();
    if let Some(msg) = reify_ir::check_length_field(op_kind, field, v) {
        tracing::warn!(
            target: "reify_kernel_occt::length_tripwire",
            reason = "non_length_field",
            op_kind = op_kind,
            field = field,
            "{msg}"
        );
    }
    v.as_f64().ok_or_else(|| {
        GeometryError::OperationFailed(reify_ir::non_numeric_kernel_field_message(op_kind, field))
    })
}

/// Validate that `value` is a finite, strictly positive number.
///
/// Returns an `OperationFailed` error with the message
/// `"{label} must be a finite positive value"` if the check fails. `label`
/// is intended to be a specific dimension name (e.g. `"tube outer radius"`,
/// `"pipe radius"`) so the caller does not need to construct bespoke error
/// strings for each dimension.
pub(crate) fn validate_positive_finite(value: f64, label: &str) -> Result<(), GeometryError> {
    if !is_positive_finite(value) {
        return Err(GeometryError::OperationFailed(format!(
            "{label} must be a finite positive value"
        )));
    }
    Ok(())
}

/// Validate that every one of `values` is a finite, strictly positive number.
///
/// For checks whose message is a combined or cross-kernel constant rather
/// than a per-dimension label: returns `OperationFailed(message)` unless every
/// value passes.
pub(crate) fn validate_all_positive_finite(
    values: &[f64],
    message: &str,
) -> Result<(), GeometryError> {
    if !values.iter().all(|&v| is_positive_finite(v)) {
        return Err(GeometryError::OperationFailed(message.into()));
    }
    Ok(())
}

fn is_positive_finite(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// Validate an extrude distance: finite, then non-zero. `op_label` names the
/// op in the message (`"extrude"`, `"extrude_symmetric"`).
pub(crate) fn validate_extrude_distance(
    distance: f64,
    op_label: &str,
) -> Result<(), GeometryError> {
    if !distance.is_finite() {
        return Err(GeometryError::OperationFailed(format!(
            "{op_label} distance must be finite"
        )));
    }
    if distance == 0.0 {
        return Err(GeometryError::OperationFailed(format!(
            "{op_label} distance must not be zero"
        )));
    }
    Ok(())
}

/// Validate revolve parameters, in order: origin finite, axis finite, angle
/// finite, angle non-zero ([`ANGLE_ABS_MIN`]), axis non-zero-length
/// ([`AXIS_MAG_SQ_MIN`]).
///
/// Defense in depth: this Rust layer validates with stricter thresholds than
/// the C++ FFI layer (`occt_wrapper.cpp`, 1e-30 for mag²) and names the
/// parameter and its value in the message. The C++ checks stay as a safety net
/// for any path that calls the FFI directly.
pub(crate) fn validate_revolve_params(
    axis_origin: [f64; 3],
    axis_dir: [f64; 3],
    angle_rad: f64,
) -> Result<(), GeometryError> {
    if !axis_origin.iter().all(|c| c.is_finite()) {
        return Err(GeometryError::OperationFailed(format!(
            "revolve axis_origin must be finite: [{}, {}, {}]",
            axis_origin[0], axis_origin[1], axis_origin[2]
        )));
    }
    if !axis_dir.iter().all(|c| c.is_finite()) {
        return Err(GeometryError::OperationFailed(format!(
            "revolve axis_dir must be finite: [{}, {}, {}]",
            axis_dir[0], axis_dir[1], axis_dir[2]
        )));
    }
    if !angle_rad.is_finite() {
        return Err(GeometryError::OperationFailed(format!(
            "revolve angle must be finite: {}",
            angle_rad
        )));
    }
    if angle_rad.abs() < ANGLE_ABS_MIN {
        return Err(GeometryError::OperationFailed(format!(
            "revolve angle must not be zero, got {}",
            angle_rad
        )));
    }
    let mag_sq = axis_dir[0].powi(2) + axis_dir[1].powi(2) + axis_dir[2].powi(2);
    if mag_sq < AXIS_MAG_SQ_MIN {
        return Err(GeometryError::OperationFailed(format!(
            "revolve axis_dir must not be zero-length: [{}, {}, {}]",
            axis_dir[0], axis_dir[1], axis_dir[2]
        )));
    }
    Ok(())
}

/// Validate that a loft has at least two profile sections.
pub(crate) fn validate_loft_profile_count(count: usize) -> Result<(), GeometryError> {
    if count < 2 {
        return Err(GeometryError::OperationFailed(
            "Loft requires at least 2 profiles".into(),
        ));
    }
    Ok(())
}

/// Validate that `u` and `v` are both finite (not NaN and not ±Inf).
///
/// This is the shared NaN/Inf rejection idiom for surface differential FFI
/// primitives (`surface_normal_at`, `curvature_at`). The check runs at the
/// Rust FFI boundary so the C++ wrapper never receives non-finite parametric
/// inputs — OCCT may silently produce nonsensical output for NaN/Inf inputs
/// rather than reporting a structured error, so we reject upfront.
///
/// Returns `QueryError::NonFiniteParameter { u, v }` if either component
/// is non-finite (NaN or ±Infinity). The variant echoes the bad inputs
/// so callers can surface structured diagnostics without inspecting a
/// message string.
pub(crate) fn validate_uv_finite(u: f64, v: f64) -> Result<(), QueryError> {
    if !u.is_finite() || !v.is_finite() {
        return Err(QueryError::NonFiniteParameter { u, v });
    }
    Ok(())
}

/// Validate that a pipe start-tangent is all-finite (defense-in-depth for `gp_Dir`).
///
/// Returns `OperationFailed` if any component is non-finite (NaN or ±Infinity).
///
/// # Rationale
///
/// The tangent is fed into OCCT's `gp_Dir` (via the oriented-circle profile
/// construction) to align the circular profile with the path's start frame, and
/// `gp_Dir` requires a finite, non-degenerate vector. `BRepAdaptor_CompCurve::D1`
/// already yields a normalized tangent, so a non-finite component here signals a
/// malformed wire or an FFI contract violation; this Rust-side guard rejects it
/// explicitly rather than passing NaN/∞ across the FFI boundary.
///
/// Any finite orientation is accepted: `gp_Ax2(point, dir)` builds a valid
/// orthonormal frame for ANY unit normal (including −Z), so pipes are no longer
/// restricted to a +Z start-tangent.
pub(crate) fn validate_pipe_start_tangent(t: Point3) -> Result<(), GeometryError> {
    if !t.x.is_finite() || !t.y.is_finite() || !t.z.is_finite() {
        return Err(GeometryError::OperationFailed(format!(
            "pipe start-tangent has non-finite component (got ({:.3}, {:.3}, {:.3}))",
            t.x, t.y, t.z
        )));
    }
    Ok(())
}

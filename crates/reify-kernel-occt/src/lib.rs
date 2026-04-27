//! OpenCASCADE geometry kernel implementation for Reify.
//!
//! Provides two public types:
//!
//! - [`OcctKernel`] — the raw kernel, `!Send + !Sync` (contains `cxx::UniquePtr`).
//!   Useful for single-threaded test scenarios where channel overhead is unwanted.
//!
//! - [`OcctKernelHandle`] — a `Send + Sync` handle that communicates with a
//!   dedicated OS thread owning an `OcctKernel`. Implements [`GeometryKernel`]
//!   and is the recommended API for all production and cross-thread usage.
//!
//! [`GeometryKernel`]: reify_types::GeometryKernel

/// Whether OCCT libraries were found at build time.
///
/// This constant is `true` when the build detected OCCT include/lib
/// directories, and `false` otherwise (stub types are used instead).
/// Downstream crates can check this to skip OCCT-dependent tests
/// at runtime.
pub const OCCT_AVAILABLE: bool = cfg!(has_occt);

#[cfg(has_occt)]
#[allow(dead_code)]
mod ffi;
#[cfg(has_occt)]
pub use ffi::ffi::TopologyCacheBuildCounts;
mod floor_constants;
pub use floor_constants::RUST_GUARD_MARKER;
#[cfg(has_occt)]
mod handle;
#[cfg(has_occt)]
pub use handle::OcctKernelHandle;
#[cfg(has_occt)]
use floor_constants::{CPP_LINE_WIRE_MIN_LENGTH_SQ, RUST_LINE_WIRE_MIN_LENGTH_SQ};
/// Compile-time invariant: the Rust primary floor must stay strictly below the C++ floor.
/// Changing either constant in `src/floor_constants.rs` re-evaluates this at `cargo check`.
#[cfg(has_occt)]
const _: () = assert!(RUST_LINE_WIRE_MIN_LENGTH_SQ < CPP_LINE_WIRE_MIN_LENGTH_SQ);

#[cfg(not(has_occt))]
mod stubs;
#[cfg(not(has_occt))]
pub use stubs::{OcctKernel, OcctKernelHandle, TopologyCacheBuildCounts};

#[cfg(has_occt)]
use std::collections::HashMap;

#[cfg(has_occt)]
use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp,
    GeometryQuery, Mesh, OpaqueState, QueryError, ReprKind, TessError, Value, WarmStartable,
};

#[cfg(has_occt)]
/// Send-safe payload for OCCT warm-start state.
///
/// Contains BRep ASCII serializations of all shapes in the kernel, plus the
/// next handle ID counter. BRep format is valid UTF-8 text, so `String` is
/// used instead of `Vec<u8>`.
struct OcctWarmState {
    /// Map from handle ID to BRep ASCII string of the corresponding shape.
    shapes: HashMap<u64, String>,
    /// The next handle ID to assign (preserves ID namespace across warm-start).
    next_id: u64,
}

#[cfg(has_occt)]
/// Minimum squared magnitude for axis/direction vectors.
///
/// Vectors with mag² below this threshold are treated as zero-length and rejected.
/// This catches physically meaningless axes (e.g. sub-micrometer) while allowing
/// any vector representable in normal CAD geometry.
///
/// Value: 1e-12 → minimum magnitude ~1e-6 → ~1 micrometer.
const AXIS_MAG_SQ_MIN: f64 = 1e-12;

/// Minimum absolute angle (radians) for revolve operations.
/// Angles below this are treated as effectively zero.
/// Matches the C++ ANGLE_ABS_MIN threshold (1e-30).
/// Value: 1e-30 radians ≈ 5.7e-29 degrees — far below any physical relevance.
#[cfg(has_occt)]
const ANGLE_ABS_MIN: f64 = 1e-30;

#[cfg(has_occt)]
/// Extract an f64 from a Value (Int, Real, or Scalar → SI value).
fn extract_f64(v: &Value) -> Result<f64, GeometryError> {
    v.as_f64()
        .ok_or_else(|| GeometryError::OperationFailed("expected numeric value".into()))
}

#[cfg(has_occt)]
/// Validate that `value` is a finite, strictly positive number.
///
/// Returns an `OperationFailed` error with the message
/// `"{label} must be a finite positive value"` if the check fails. `label`
/// is intended to be a specific dimension name (e.g. `"tube outer radius"`,
/// `"pipe radius"`) so the caller does not need to construct bespoke error
/// strings for each dimension.
fn validate_positive_finite(value: f64, label: &str) -> Result<(), GeometryError> {
    if !(value.is_finite() && value > 0.0) {
        return Err(GeometryError::OperationFailed(format!(
            "{label} must be a finite positive value"
        )));
    }
    Ok(())
}

#[cfg(has_occt)]
/// Tolerance for the pipe start-tangent +Z check.
///
/// The guard is symmetric: `|t.z - 1| < PIPE_START_TANGENT_Z_EPSILON`.
/// For a true unit vector the per-axis residual satisfies x²+y² < 2ε,
/// so |x|,|y| < √(2ε).
const PIPE_START_TANGENT_Z_EPSILON: f64 = 1e-6;

#[cfg(has_occt)]
/// Validate that a pipe start-tangent is approximately +Z and all-finite.
///
/// Returns `OperationFailed` if any component is non-finite (NaN or ±Infinity) or if
/// `t.z` is outside `[1 - PIPE_START_TANGENT_Z_EPSILON, 1 + PIPE_START_TANGENT_Z_EPSILON]`
/// (tangent not close enough to the unit +Z vector).
///
/// # Rationale
///
/// The circular profile face is built in the XY plane (normal = +Z).
/// `BRepOffsetAPI_MakePipe` requires the profile plane to align with the path's
/// start-tangent. For non-+Z paths the swept solid is degenerate (zero volume);
/// this helper detects that upfront and returns an explicit error rather than
/// silently producing unusable geometry. General orientation support is deferred
/// future work (option (a) from task-2095 review).
fn validate_pipe_start_tangent(t: ffi::ffi::Point3) -> Result<(), GeometryError> {
    if !t.x.is_finite() || !t.y.is_finite() || !t.z.is_finite() {
        return Err(GeometryError::OperationFailed(format!(
            "pipe start-tangent has non-finite component (got ({:.3}, {:.3}, {:.3}))",
            t.x, t.y, t.z
        )));
    }
    if t.z < 1.0 - PIPE_START_TANGENT_Z_EPSILON || t.z > 1.0 + PIPE_START_TANGENT_Z_EPSILON {
        return Err(GeometryError::OperationFailed(format!(
            "pipe currently only supports paths whose start-tangent is +Z \
             (tolerance {:e}) (got tangent ({:.3}, {:.3}, {:.3}))",
            PIPE_START_TANGENT_Z_EPSILON, t.x, t.y, t.z
        )));
    }
    Ok(())
}

#[cfg(has_occt)]
/// OpenCASCADE geometry kernel (raw, `!Send + !Sync`).
///
/// Contains `cxx::UniquePtr<OcctShape>` handles which are `!Send`, so the
/// kernel cannot cross thread boundaries. For cross-thread usage, use
/// [`OcctKernelHandle`] which runs the kernel on a dedicated OS thread.
pub struct OcctKernel {
    shapes: HashMap<u64, cxx::UniquePtr<ffi::ffi::OcctShape>>,
    next_id: u64,
    /// Number of shapes that failed deserialization during the last `with_warm_state()` call.
    last_warm_start_failures: usize,
}

// Note: OcctKernel is !Send + !Sync because cxx::UniquePtr<OcctShape> is !Send.
// Use OcctKernelHandle for cross-thread usage — it communicates with a dedicated
// OS thread that owns the kernel.

#[cfg(has_occt)]
impl OcctKernel {
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            next_id: 1,
            last_warm_start_failures: 0,
        }
    }

    /// Store a shape and return the next handle (defaults to `ReprKind::Solid`).
    fn store(&mut self, shape: cxx::UniquePtr<ffi::ffi::OcctShape>) -> GeometryHandle {
        self.store_with_repr(shape, ReprKind::Solid)
    }

    /// Store a shape with an explicit `ReprKind`.
    fn store_with_repr(
        &mut self,
        shape: cxx::UniquePtr<ffi::ffi::OcctShape>,
        repr: ReprKind,
    ) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, shape);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr,
        }
    }

    /// Look up a shape by handle ID.
    fn get_shape(&self, id: GeometryHandleId) -> Result<&ffi::ffi::OcctShape, GeometryError> {
        let ptr = self
            .shapes
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))?;
        ptr.as_ref()
            .ok_or_else(|| GeometryError::OperationFailed("shape handle is null".into()))
    }

    /// Return the topology-map cache build counts for the shape identified by
    /// `handle`. Each counter is 0 on a fresh shape and increments to 1 when
    /// the corresponding lazy cache slot is first populated.
    ///
    /// Returns [`GeometryError::InvalidReference`] if `handle` is unknown.
    pub fn topology_cache_build_counts(
        &self,
        handle: GeometryHandleId,
    ) -> Result<ffi::ffi::TopologyCacheBuildCounts, GeometryError> {
        let shape = self.get_shape(handle)?;
        Ok(ffi::ffi::topology_cache_build_counts(shape))
    }
}

#[cfg(has_occt)]
impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(has_occt)]
/// Inherent methods — same bodies as the former `GeometryKernel` impl.
/// Called directly by the kernel thread in `OcctKernelHandle`.
impl OcctKernel {
    pub fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        let shape = match op {
            GeometryOp::Box {
                width,
                height,
                depth,
            } => {
                let w = extract_f64(width)?;
                let h = extract_f64(height)?;
                let d = extract_f64(depth)?;
                if !(w.is_finite()
                    && w > 0.0
                    && h.is_finite()
                    && h > 0.0
                    && d.is_finite()
                    && d > 0.0)
                {
                    return Err(GeometryError::OperationFailed(
                        "box dimensions must be finite positive values".into(),
                    ));
                }
                ffi::ffi::make_box(w, h, d)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Cylinder { radius, height } => {
                let r = extract_f64(radius)?;
                let h = extract_f64(height)?;
                if !(r.is_finite() && r > 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "cylinder radius must be a finite positive value".into(),
                    ));
                }
                if !(h.is_finite() && h > 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "cylinder height must be a finite positive value".into(),
                    ));
                }
                ffi::ffi::make_cylinder(r, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Sphere { radius } => {
                let r = extract_f64(radius)?;
                if !(r.is_finite() && r > 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "sphere radius must be a finite positive value".into(),
                    ));
                }
                ffi::ffi::make_sphere(r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Tube {
                outer_r,
                inner_r,
                height,
            } => {
                let outer = extract_f64(outer_r)?;
                let inner = extract_f64(inner_r)?;
                let h = extract_f64(height)?;
                validate_positive_finite(outer, "tube outer radius")?;
                validate_positive_finite(inner, "tube inner radius")?;
                validate_positive_finite(h, "tube height")?;
                // Both values are already validated finite+positive above,
                // so `>=` is unambiguous here (no NaN possible).
                if inner >= outer {
                    return Err(GeometryError::OperationFailed(
                        "tube inner radius must be strictly less than outer radius"
                            .into(),
                    ));
                }
                // Compose: outer cylinder - inner cylinder via boolean_cut.
                // Reuses validated FFI primitives rather than adding a
                // bespoke C++ wrapper (mirrors ExtrudeSymmetric pattern).
                let outer_shape = ffi::ffi::make_cylinder(outer, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let inner_shape = ffi::ffi::make_cylinder(inner, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ffi::ffi::boolean_cut(&outer_shape, &inner_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Union { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                ffi::ffi::boolean_fuse(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Difference { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                ffi::ffi::boolean_cut(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Intersection { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                ffi::ffi::boolean_common(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Fillet { target, radius } => {
                let shape = self.get_shape(*target)?;
                let r = extract_f64(radius)?;
                if !(r.is_finite() && r > 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "fillet radius must be a finite positive value".into(),
                    ));
                }
                ffi::ffi::fillet_all_edges(shape, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Chamfer { target, distance } => {
                let shape = self.get_shape(*target)?;
                let d = extract_f64(distance)?;
                if !(d.is_finite() && d > 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "chamfer distance must be a finite positive value".into(),
                    ));
                }
                ffi::ffi::chamfer_all_edges(shape, d)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Translate { target, dx, dy, dz } => {
                let shape = self.get_shape(*target)?;
                if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() {
                    return Err(GeometryError::OperationFailed(format!(
                        "translate components must be finite values: dx={dx}, dy={dy}, dz={dz}"
                    )));
                }
                ffi::ffi::translate_shape(shape, *dx, *dy, *dz)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Rotate {
                target,
                axis,
                angle_rad,
            } => {
                let shape = self.get_shape(*target)?;
                if !axis[0].is_finite()
                    || !axis[1].is_finite()
                    || !axis[2].is_finite()
                    || !angle_rad.is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "rotate parameters must be finite values: axis=[{}, {}, {}], angle_rad={}",
                        axis[0], axis[1], axis[2], angle_rad
                    )));
                }
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "rotation axis must not be zero-length".into(),
                    ));
                }
                ffi::ffi::rotate_shape(shape, axis[0], axis[1], axis[2], *angle_rad)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            } => {
                let shape = self.get_shape(*target)?;
                let sp = extract_f64(spacing)?;
                if *count == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear pattern count must be >= 1".into(),
                    ));
                }
                ffi::ffi::linear_pattern(
                    shape,
                    direction[0],
                    direction[1],
                    direction[2],
                    *count as u32,
                    sp,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            } => {
                let shape = self.get_shape(*target)?;
                let total_angle = extract_f64(angle)?;
                if *count == 0 {
                    return Err(GeometryError::OperationFailed(
                        "circular pattern count must be >= 1".into(),
                    ));
                }
                ffi::ffi::circular_pattern(
                    shape,
                    axis_origin[0],
                    axis_origin[1],
                    axis_origin[2],
                    axis_dir[0],
                    axis_dir[1],
                    axis_dir[2],
                    *count as u32,
                    total_angle,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            } => {
                let shape = self.get_shape(*target)?;
                // Validate plane normal is non-zero
                let mag_sq = plane_normal[0] * plane_normal[0]
                    + plane_normal[1] * plane_normal[1]
                    + plane_normal[2] * plane_normal[2];
                if !plane_normal[0].is_finite()
                    || !plane_normal[1].is_finite()
                    || !plane_normal[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "mirror plane normal must be a finite non-zero vector".into(),
                    ));
                }
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "mirror plane normal must not be zero-length".into(),
                    ));
                }
                ffi::ffi::mirror_shape(
                    shape,
                    plane_origin[0],
                    plane_origin[1],
                    plane_origin[2],
                    plane_normal[0],
                    plane_normal[1],
                    plane_normal[2],
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Loft { profiles } => {
                if profiles.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "Loft requires at least 2 profiles".into(),
                    ));
                }
                let mut vec = ffi::ffi::new_shape_vec();
                for &pid in profiles {
                    let shape = self.get_shape(pid)?;
                    ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
                }
                ffi::ffi::loft_profiles(&vec)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Draft {
                target,
                angle,
                plane,
            } => {
                let shape = self.get_shape(*target)?;
                let angle_rad = extract_f64(angle)?;
                let plane_shape = self.get_shape(*plane)?;
                ffi::ffi::draft_shape(shape, angle_rad, plane_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Thicken { target, offset } => {
                let shape = self.get_shape(*target)?;
                let off = extract_f64(offset)?;
                ffi::ffi::thicken_shape(shape, off)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Shell {
                target,
                thickness,
                faces_to_remove,
            } => {
                let shape = self.get_shape(*target)?;
                let th = extract_f64(thickness)?;
                let face_indices: Vec<u32> = faces_to_remove.iter().map(|&i| i as u32).collect();
                ffi::ffi::shell_shape(shape, th, &face_indices)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Scale { target, factor } => {
                let shape = self.get_shape(*target)?;
                if !factor.is_finite() || *factor == 0.0 {
                    return Err(GeometryError::OperationFailed(format!(
                        "scale factor must be finite and non-zero, got {factor}"
                    )));
                }
                ffi::ffi::scale_shape(shape, *factor, 0.0, 0.0, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                let shape = self.get_shape(*target)?;
                if !point[0].is_finite()
                    || !point[1].is_finite()
                    || !point[2].is_finite()
                    || !axis[0].is_finite()
                    || !axis[1].is_finite()
                    || !axis[2].is_finite()
                    || !angle_rad.is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "rotate_around parameters must be finite: point={:?}, axis={:?}, angle={}",
                        point, axis, angle_rad
                    )));
                }
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "rotate_around axis must not be zero-length".into(),
                    ));
                }
                ffi::ffi::rotate_around_shape(
                    shape, point[0], point[1], point[2], axis[0], axis[1], axis[2], *angle_rad,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Extrude { profile, distance } => {
                let dist = extract_f64(distance)?;
                if !dist.is_finite() {
                    return Err(GeometryError::OperationFailed(
                        "extrude distance must be finite".into(),
                    ));
                }
                if dist == 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "extrude distance must not be zero".into(),
                    ));
                }
                let profile_shape = self.get_shape(*profile)?;
                ffi::ffi::make_prism(profile_shape, 0.0, 0.0, dist)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                // Revolve validation (Rust layer — DEFENSE-IN-DEPTH)
                // This Rust layer validates inputs with stricter thresholds (AXIS_MAG_SQ_MIN=1e-12,
                // ANGLE_ABS_MIN=1e-30) and produces descriptive error messages including parameter
                // names and values. The C++ FFI layer (occt_wrapper.cpp) has its own validation
                // with relaxed thresholds (1e-30 for mag_sq) as a safety net for any future code
                // paths that may call FFI directly, bypassing this Rust layer.
                if !axis_origin[0].is_finite()
                    || !axis_origin[1].is_finite()
                    || !axis_origin[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "revolve axis_origin must be finite: [{}, {}, {}]",
                        axis_origin[0], axis_origin[1], axis_origin[2]
                    )));
                }
                if !axis_dir[0].is_finite() || !axis_dir[1].is_finite() || !axis_dir[2].is_finite()
                {
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
                let profile_shape = self.get_shape(*profile)?;
                ffi::ffi::make_revolve(
                    profile_shape,
                    axis_origin[0],
                    axis_origin[1],
                    axis_origin[2],
                    axis_dir[0],
                    axis_dir[1],
                    axis_dir[2],
                    *angle_rad,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Sweep { profile, path } => {
                let profile_shape = self.get_shape(*profile)?;
                let path_shape = self.get_shape(*path)?;
                ffi::ffi::make_pipe(profile_shape, path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Pipe { path, radius } => {
                let r = extract_f64(radius)?;
                validate_positive_finite(r, "pipe radius")?;
                // Reject paths whose start-tangent is not approximately +Z; see validate_pipe_start_tangent.
                let path_shape = self.get_shape(*path)?;
                let t = ffi::ffi::wire_start_tangent(path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                validate_pipe_start_tangent(t)?;
                let circle_shape = ffi::ffi::make_circle_face(r, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ffi::ffi::make_pipe(&circle_shape, path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::ExtrudeSymmetric { profile, distance } => {
                let dist = extract_f64(distance)?;
                if !dist.is_finite() {
                    return Err(GeometryError::OperationFailed(
                        "extrude_symmetric distance must be finite".into(),
                    ));
                }
                if dist == 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "extrude_symmetric distance must not be zero".into(),
                    ));
                }
                // Compose: prism the profile by the full distance, then
                // translate by -distance/2 along the extrusion axis so the
                // resulting solid's centroid (in z) aligns with the
                // profile's centroid. This reuses the validated make_prism
                // + translate_shape FFI calls rather than adding a bespoke
                // C++ wrapper.
                let profile_shape = self.get_shape(*profile)?;
                let prism = ffi::ffi::make_prism(profile_shape, 0.0, 0.0, dist)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ffi::ffi::translate_shape(&prism, 0.0, 0.0, -dist / 2.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::SweepGuided {
                profile,
                path,
                guide,
            } => {
                let profile_shape = self.get_shape(*profile)?;
                let path_shape = self.get_shape(*path)?;
                let guide_shape = self.get_shape(*guide)?;
                ffi::ffi::make_pipe_shell(profile_shape, path_shape, guide_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::LoftGuided { profiles, guides } => {
                if profiles.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "loft_guided requires at least 2 profiles".into(),
                    ));
                }
                if guides.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "loft_guided requires at least 1 guide".into(),
                    ));
                }
                let mut profile_vec = ffi::ffi::new_shape_vec();
                for &pid in profiles {
                    let shape = self.get_shape(pid)?;
                    ffi::ffi::shape_vec_push(profile_vec.pin_mut(), shape);
                }
                let mut guide_vec = ffi::ffi::new_shape_vec();
                for &gid in guides {
                    let shape = self.get_shape(gid)?;
                    ffi::ffi::shape_vec_push(guide_vec.pin_mut(), shape);
                }
                ffi::ffi::loft_guided_profiles(&profile_vec, &guide_vec)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::LineSegment {
                x1, y1, z1, x2, y2, z2,
            } => {
                crate::floor_constants::line_segment_rust_guard(x2 - x1, y2 - y1, z2 - z1)
                    .map_err(GeometryError::OperationFailed)?;
                let shape = ffi::ffi::make_line_wire(*x1, *y1, *z1, *x2, *y2, *z2)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::Arc {
                center, radius, start_angle, end_angle, axis,
            } => {
                if !radius.is_finite() || *radius <= 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "arc radius must be finite and positive".into(),
                    ));
                }
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "arc axis must not be zero-length".into(),
                    ));
                }
                let shape = ffi::ffi::make_arc_wire(
                    center[0], center[1], center[2],
                    *radius,
                    *start_angle, *end_angle,
                    axis[0], axis[1], axis[2],
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::Helix { radius, pitch, height } => {
                if !radius.is_finite() || *radius <= 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "helix radius must be finite and positive".into(),
                    ));
                }
                if !pitch.is_finite() || *pitch <= 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "helix pitch must be finite and positive".into(),
                    ));
                }
                if !height.is_finite() || *height <= 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "helix height must be finite and positive".into(),
                    ));
                }
                let shape = ffi::ffi::make_helix_wire(*radius, *pitch, *height)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::InterpCurve { points } => {
                if points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "interp_curve requires at least 2 points".into(),
                    ));
                }
                let coords: Vec<f64> = points.iter().flat_map(|p| p.iter().copied()).collect();
                let shape = ffi::ffi::make_interp_curve(&coords, points.len())
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::BezierCurve { control_points } => {
                if control_points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "bezier_curve requires at least 2 control points".into(),
                    ));
                }
                let coords: Vec<f64> = control_points.iter().flat_map(|p| p.iter().copied()).collect();
                let shape = ffi::ffi::make_bezier_curve(&coords, control_points.len())
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::NurbsCurve {
                control_points, weights, knots, degree,
            } => {
                if control_points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve requires at least 2 control points".into(),
                    ));
                }
                if *degree < 1 {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve degree must be >= 1".into(),
                    ));
                }
                if weights.len() != control_points.len() {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve: weights count must equal control points count".into(),
                    ));
                }
                if knots.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve: knots vector must not be empty".into(),
                    ));
                }
                let expected_knots = control_points.len() + degree + 1;
                if knots.len() != expected_knots {
                    return Err(GeometryError::OperationFailed(
                        format!(
                            "nurbs_curve: expected {} knots (n_points + degree + 1), got {}",
                            expected_knots, knots.len(),
                        ),
                    ));
                }
                let coords: Vec<f64> = control_points.iter().flat_map(|p| p.iter().copied()).collect();
                let shape = ffi::ffi::make_nurbs_curve(
                    &coords, control_points.len(),
                    weights, knots,
                    *degree as i32,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                return Ok(self.store_with_repr(shape, ReprKind::Wire));
            }
            GeometryOp::LinearPattern2D {
                target,
                direction1,
                count1,
                spacing1,
                direction2,
                count2,
                spacing2,
            } => {
                let shape = self.get_shape(*target)?;
                let sp1 = extract_f64(spacing1)?;
                let sp2 = extract_f64(spacing2)?;
                // Validate direction vectors are finite (NaN/Inf would cause
                // undefined OCCT behavior).  Consistent with Mirror arm.
                if !direction1[0].is_finite()
                    || !direction1[1].is_finite()
                    || !direction1[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d direction1 must contain finite values".into(),
                    ));
                }
                if !direction2[0].is_finite()
                    || !direction2[1].is_finite()
                    || !direction2[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d direction2 must contain finite values".into(),
                    ));
                }
                if *count1 == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d count1 must be >= 1".into(),
                    ));
                }
                if *count2 == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d count2 must be >= 1".into(),
                    ));
                }
                ffi::ffi::linear_pattern_2d(
                    shape,
                    direction1[0],
                    direction1[1],
                    direction1[2],
                    *count1 as u32,
                    sp1,
                    direction2[0],
                    direction2[1],
                    direction2[2],
                    *count2 as u32,
                    sp2,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::ArbitraryPattern {
                target,
                transforms,
            } => {
                let shape = self.get_shape(*target)?;
                if transforms.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "arbitrary_pattern requires at least one transform".into(),
                    ));
                }
                let flat_transforms: Vec<f64> =
                    transforms.iter().flat_map(|t| t.iter().copied()).collect();
                let num_transforms = transforms.len() as u32;
                ffi::ffi::arbitrary_pattern(shape, &flat_transforms, num_transforms)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
        };
        Ok(self.store(shape))
    }

    pub fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        match query {
            GeometryQuery::Volume(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let vol = ffi::ffi::query_volume(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(vol))
            }
            GeometryQuery::SurfaceArea(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let area = ffi::ffi::query_area(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(area))
            }
            GeometryQuery::Centroid(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let pt = ffi::ffi::query_centroid(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                // Return centroid as a JSON string since Value has no tuple variant
                Ok(Value::String(format!(
                    "{{\"x\":{},\"y\":{},\"z\":{}}}",
                    pt.x, pt.y, pt.z
                )))
            }
            GeometryQuery::BoundingBox(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let bb = ffi::ffi::query_bbox(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(format!(
                    "{{\"xmin\":{},\"ymin\":{},\"zmin\":{},\"xmax\":{},\"ymax\":{},\"zmax\":{}}}",
                    bb.xmin, bb.ymin, bb.zmin, bb.xmax, bb.ymax, bb.zmax
                )))
            }
            GeometryQuery::Distance { from, to } => {
                let s1 = self
                    .get_shape(*from)
                    .map_err(|_| QueryError::InvalidHandle(*from))?;
                let s2 = self
                    .get_shape(*to)
                    .map_err(|_| QueryError::InvalidHandle(*to))?;
                let dist = ffi::ffi::query_distance(s1, s2)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(dist))
            }
            GeometryQuery::MomentOfInertia { handle, axis } => {
                let shape = self
                    .get_shape(*handle)
                    .map_err(|_| QueryError::InvalidHandle(*handle))?;
                let moi = ffi::ffi::query_moment_of_inertia(shape, axis[0], axis[1], axis[2])
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(moi))
            }
            // For uniform-density solids, the center of mass equals the geometric centroid.
            // density is intentionally ignored here (see GeometryQuery::CenterOfMass doc).
            GeometryQuery::CenterOfMass { handle, density: _ } => {
                let shape = self
                    .get_shape(*handle)
                    .map_err(|_| QueryError::InvalidHandle(*handle))?;
                let pt = ffi::ffi::query_centroid(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(format!(
                    "{{\"x\":{},\"y\":{},\"z\":{}}}",
                    pt.x, pt.y, pt.z
                )))
            }
            GeometryQuery::InertiaTensor { handle, density } => {
                let shape = self
                    .get_shape(*handle)
                    .map_err(|_| QueryError::InvalidHandle(*handle))?;
                let t = ffi::ffi::query_inertia_tensor(shape, *density)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(vec![
                    Value::List(vec![
                        Value::Real(t.m11),
                        Value::Real(t.m12),
                        Value::Real(t.m13),
                    ]),
                    Value::List(vec![
                        Value::Real(t.m21),
                        Value::Real(t.m22),
                        Value::Real(t.m23),
                    ]),
                    Value::List(vec![
                        Value::Real(t.m31),
                        Value::Real(t.m32),
                        Value::Real(t.m33),
                    ]),
                ]))
            }
            GeometryQuery::AdjacentFaces { shape, face_index } => {
                let s = self
                    .get_shape(*shape)
                    .map_err(|_| QueryError::InvalidHandle(*shape))?;
                let idx_u32: u32 = (*face_index).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "adjacent_faces: face_index {} exceeds u32::MAX",
                        face_index
                    ))
                })?;
                let neighbors = ffi::ffi::adjacent_faces(s, idx_u32)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(
                    neighbors
                        .into_iter()
                        .map(|i| Value::Int(i as i64))
                        .collect(),
                ))
            }
            GeometryQuery::SharedEdges {
                shape,
                face_a,
                face_b,
            } => {
                let s = self
                    .get_shape(*shape)
                    .map_err(|_| QueryError::InvalidHandle(*shape))?;
                let a_u32: u32 = (*face_a).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "shared_edges: face_a {} exceeds u32::MAX",
                        face_a
                    ))
                })?;
                let b_u32: u32 = (*face_b).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "shared_edges: face_b {} exceeds u32::MAX",
                        face_b
                    ))
                })?;
                let edges = ffi::ffi::shared_edges(s, a_u32, b_u32)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(
                    edges.into_iter().map(|i| Value::Int(i as i64)).collect(),
                ))
            }
            GeometryQuery::IsWatertight(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let v = ffi::ffi::is_watertight(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsManifold(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let v = ffi::ffi::is_manifold(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsOrientable(id) => {
                let shape = self
                    .get_shape(*id)
                    .map_err(|_| QueryError::InvalidHandle(*id))?;
                let v = ffi::ffi::is_orientable(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
        }
    }

    pub fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        let shape = self
            .get_shape(handle)
            .map_err(|_| ExportError::InvalidHandle(handle))?;

        match format {
            ExportFormat::Step => {
                let content = ffi::ffi::export_step(shape)
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                writer
                    .write_all(content.as_bytes())
                    .map_err(|e| ExportError::IoError(e.to_string()))
            }
            _ => Err(ExportError::FormatError(format!(
                "unsupported export format: {:?}",
                format
            ))),
        }
    }

    pub fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        let shape = self
            .get_shape(handle)
            .map_err(|_| TessError::InvalidHandle(handle))?;

        let result = ffi::ffi::tessellate_shape(shape, tolerance)
            .map_err(|e| TessError::TessellationFailed(e.to_string()))?;

        Ok(Mesh {
            vertices: result.vertices,
            indices: result.indices,
            normals: if result.normals.is_empty() {
                None
            } else {
                Some(result.normals)
            },
        })
    }
}

#[cfg(has_occt)]
impl WarmStartable for OcctKernel {
    fn warm_state(&self) -> Option<OpaqueState> {
        if self.shapes.is_empty() {
            return None;
        }
        let mut warm_shapes = HashMap::new();
        let mut total_bytes: usize = 0;
        for (&id, shape) in &self.shapes {
            let Some(shape_ref) = shape.as_ref() else {
                continue; // Skip null shapes (best-effort, like serialization failures)
            };
            match ffi::ffi::serialize_brep(shape_ref) {
                Ok(brep) => {
                    total_bytes += brep.len();
                    warm_shapes.insert(id, brep);
                }
                Err(_) => {
                    // Skip shapes that fail to serialize (best-effort)
                    continue;
                }
            }
        }
        if warm_shapes.is_empty() {
            return None;
        }
        let size_estimate = total_bytes + 64; // overhead for HashMap + struct
        Some(OpaqueState::new(
            OcctWarmState {
                shapes: warm_shapes,
                next_id: self.next_id,
            },
            size_estimate,
        ))
    }

    fn with_warm_state(&mut self, state: OpaqueState) {
        let warm = match state.downcast::<OcctWarmState>() {
            Some(w) => w,
            None => return, // Wrong type, silently ignore per trait contract
        };
        // Stage deserialization into a temporary map first. If all entries fail
        // to deserialize, we preserve the kernel's pre-existing state untouched.
        self.last_warm_start_failures = 0;
        let mut staged = HashMap::new();
        for (id, brep) in warm.shapes {
            cxx::let_cxx_string!(brep_cxx = brep.as_str());
            match ffi::ffi::deserialize_brep(&brep_cxx) {
                Ok(shape) => {
                    staged.insert(id, shape);
                }
                Err(e) => {
                    eprintln!("warning: warm-start deserialization failed for shape {id}: {e}");
                    self.last_warm_start_failures += 1;
                    continue;
                }
            }
        }
        // Atomic swap: only replace kernel state if at least one shape was
        // successfully deserialized. Otherwise the kernel state is untouched.
        if !staged.is_empty() {
            self.shapes = staged;
            self.next_id = warm.next_id;
        }
    }
}

#[cfg(all(test, has_occt))]
impl OcctKernel {
    /// Store a raw OcctShape and return its GeometryHandleId for testing.
    fn store_raw(&mut self, shape: cxx::UniquePtr<ffi::ffi::OcctShape>) -> GeometryHandleId {
        let h = self.store(shape);
        h.id
    }

    /// Inject a null `UniquePtr<OcctShape>` into the shapes map for testing.
    /// This simulates a corrupted shape handle (present in map but wrapping a
    /// null C++ pointer).
    fn insert_null_shape(&mut self, id: u64) {
        self.shapes.insert(id, cxx::UniquePtr::null());
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }

    /// Returns the number of shapes that failed deserialization during the
    /// last `with_warm_state()` call.
    pub fn warm_start_failures(&self) -> usize {
        self.last_warm_start_failures
    }
}

/// Test fixture helpers: exposed as `pub` (not gated on `cfg(test)`) so that
/// integration tests in `tests/` can call them.  These are named with a
/// `_for_test` suffix to signal their intended scope.
///
/// Integration tests are compiled as a separate crate that depends on this
/// library in its normal (non-test) build mode, so `#[cfg(test)]`-gated items
/// in this crate are NOT visible to integration tests.  Gating on `has_occt`
/// only (no `test` cfg) keeps these helpers out of stub builds while keeping
/// them available to all test binaries that link this crate.
#[cfg(has_occt)]
impl OcctKernel {
    /// Create a circle face at the given z-height via the OCCT FFI and store
    /// it in the kernel, returning its `GeometryHandleId`.
    ///
    /// Exposed for integration tests that need a `TopAbs_FACE` fixture without
    /// going through the production `GeometryOp` API, which has no standalone
    /// face constructor.
    pub fn store_circle_face_for_test(&mut self, radius: f64, z: f64) -> GeometryHandleId {
        let shape = ffi::ffi::make_circle_face(radius, z)
            .expect("make_circle_face should succeed in test fixture");
        let h = self.store(shape);
        h.id
    }

    /// Build a non-manifold compound (3 faces sharing 1 edge) and store it.
    ///
    /// Returns the `GeometryHandleId` of the stored compound. Used by
    /// `conformance_integration` tests to verify that `IsManifold` returns `false`
    /// when an edge has 3+ incident faces.
    pub fn store_nonmanifold_compound_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_nonmanifold_compound_for_test()
            .expect("make_nonmanifold_compound_for_test should succeed");
        let h = self.store(shape);
        h.id
    }

    /// Build a malformed solid (10×10×10 mm box missing one face) and store it.
    ///
    /// The solid's open shell causes `BRepCheck_Analyzer::IsValid()` to return
    /// `false`, exercising the analyzer branch of `is_watertight` (as opposed to
    /// the shape-type guard branch). Used by `conformance_integration` tests.
    pub fn store_malformed_solid_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_malformed_solid_for_test()
            .expect("make_malformed_solid_for_test should succeed");
        let h = self.store(shape);
        h.id
    }

    /// Build a non-orientable shell (2 faces using a shared edge with the same
    /// orientation) and store it.
    ///
    /// `ShapeAnalysis_Shell::CheckOrientedShells` returns `Standard_True` (problems
    /// found) for this shell, so `is_orientable` returns `false`. Used by
    /// `conformance_integration` tests.
    pub fn store_nonorientable_shell_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_nonorientable_shell_for_test()
            .expect("make_nonorientable_shell_for_test should succeed");
        let h = self.store(shape);
        h.id
    }
}

#[cfg(all(test, has_occt))]
mod tests {
    use super::*;
    use crate::floor_constants::RUST_GUARD_MARKER;

    /// Create a circle face of given `radius`, rotate it into the XZ plane, translate
    /// it `offset_r` along X, and store into `kernel`. Returns a GeometryHandleId
    /// suitable for revolving around the Z axis to produce a torus.
    fn make_torus_profile(kernel: &mut OcctKernel, radius: f64, offset_r: f64) -> GeometryHandleId {
        let face =
            ffi::ffi::make_circle_face(radius, 0.0).expect("make_circle_face should succeed");
        let rotated = ffi::ffi::rotate_shape(&face, 1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2)
            .expect("rotate_shape should succeed");
        let translated = ffi::ffi::translate_shape(&rotated, offset_r, 0.0, 0.0)
            .expect("translate_shape should succeed");
        kernel.store_raw(translated)
    }

    /// Create a rect face of given `width` × `height`, rotate it into the XZ plane,
    /// translate it `offset_r` along X, and store into `kernel`. Returns a
    /// GeometryHandleId suitable for revolving around the Z axis.
    fn make_rect_torus_profile(
        kernel: &mut OcctKernel,
        width: f64,
        height: f64,
        offset_r: f64,
    ) -> GeometryHandleId {
        let face = ffi::ffi::make_rect_face(width, height, 0.0, 0.0, 0.0)
            .expect("make_rect_face should succeed");
        let rotated = ffi::ffi::rotate_shape(&face, 1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2)
            .expect("rotate_shape should succeed");
        let translated = ffi::ffi::translate_shape(&rotated, offset_r, 0.0, 0.0)
            .expect("translate_shape should succeed");
        kernel.store_raw(translated)
    }

    /// Assert that the volume of the shape at `handle_id` is within `tolerance`
    /// relative error of `expected`. Panics with a descriptive message including `label`.
    fn assert_volume_near(
        kernel: &mut OcctKernel,
        handle_id: GeometryHandleId,
        expected: f64,
        tolerance: f64,
        label: &str,
    ) {
        let vol = kernel
            .query(&GeometryQuery::Volume(handle_id))
            .expect("Volume query should succeed")
            .as_f64()
            .expect("Volume should be numeric");
        let rel_err = (vol - expected).abs() / expected;
        assert!(
            rel_err < tolerance,
            "{label}: expected volume \u{2248} {expected:.2}, got {vol:.2} (rel_err={rel_err:.4})"
        );
    }

    /// Assert that a `Result` is an `Err(GeometryError::OperationFailed(msg))` where
    /// `msg` contains the given `expected_substring`. Panics on `Ok` or wrong error variant.
    fn assert_operation_fails_with(
        result: Result<GeometryHandle, GeometryError>,
        expected_substring: &str,
    ) {
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.to_lowercase()
                        .contains(&expected_substring.to_lowercase()),
                    "expected error containing '{expected_substring}', got: {msg}"
                );
            }
            Ok(_) => panic!("expected OperationFailed containing '{expected_substring}', got Ok"),
            Err(other) => panic!(
                "expected OperationFailed containing '{expected_substring}', got {:?}",
                other
            ),
        }
    }

    /// Parse a `{"x":_,"y":_,"z":_}` JSON string into `(x, y, z)`.
    /// Used by `CenterOfMass` query tests to decode the `Value::String` encoding returned
    /// by `query_centroid`.
    fn parse_centroid_json(s: &str) -> (f64, f64, f64) {
        let parse_field = |field: &str| -> f64 {
            let needle = format!("\"{field}\":");
            let start = s.find(needle.as_str()).unwrap_or_else(|| {
                panic!("field {field} not found in centroid JSON: {s:?}")
            }) + needle.len();
            let rest = &s[start..];
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            rest[..end].parse::<f64>().unwrap_or_else(|e| {
                panic!("failed to parse {field} in centroid JSON: {s:?}: {e}")
            })
        };
        (parse_field("x"), parse_field("y"), parse_field("z"))
    }

    /// Decode the 3-row × 3-col `Value::List` returned by an `InertiaTensor` query into
    /// a `[[f64;3];3]` array.  Panics with a descriptive message if the structure does not
    /// match the expected nested-list shape.
    fn extract_3x3_tensor_entries(value: &Value) -> [[f64; 3]; 3] {
        let rows = match value {
            Value::List(rows) => {
                assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
                rows
            }
            other => panic!("expected Value::List(rows), got {:?}", other),
        };
        let mut entries = [[0.0f64; 3]; 3];
        for (i, row) in rows.iter().enumerate() {
            let cols = match row {
                Value::List(cols) => {
                    assert_eq!(cols.len(), 3, "row {} expected 3 cols, got {}", i, cols.len());
                    cols
                }
                other => panic!("row {} expected Value::List, got {:?}", i, other),
            };
            for (j, col) in cols.iter().enumerate() {
                entries[i][j] = match col {
                    Value::Real(v) => *v,
                    other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
                };
            }
        }
        entries
    }

    #[test]
    fn occt_available_is_true_when_built_with_occt() {
        const {
            assert!(
                crate::OCCT_AVAILABLE,
                "OCCT_AVAILABLE should be true on a system with OCCT installed"
            )
        };
    }

    #[test]
    fn warm_state_returns_none_on_fresh_kernel() {
        let kernel = OcctKernel::new();
        assert!(
            kernel.warm_state().is_none(),
            "fresh kernel should have no warm state"
        );
    }

    #[test]
    fn warm_state_returns_some_after_ops() {
        let mut kernel = OcctKernel::new();
        // Execute a box and a cylinder
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();

        let state = kernel.warm_state();
        assert!(state.is_some(), "kernel with shapes should have warm state");
        let state = state.unwrap();
        assert!(
            state.estimated_size_bytes() > 0,
            "estimated size should be positive"
        );
    }

    #[test]
    fn warm_start_roundtrip_single_shape() {
        // Create kernel A with a box
        let mut kernel_a = OcctKernel::new();
        kernel_a
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Extract warm state
        let state = kernel_a.warm_state().expect("should have warm state");

        // Create fresh kernel B and restore warm state
        let mut kernel_b = OcctKernel::new();
        kernel_b.with_warm_state(state);

        // Query volume on kernel B using handle ID 1 (the box)
        let vol = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn warm_start_roundtrip_multi_shape() {
        let mut kernel_a = OcctKernel::new();

        // Create box (10x20x30), handle ID 1
        kernel_a
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Create cylinder (r=5, h=20), handle ID 2
        kernel_a
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();

        // Boolean union, handle ID 3
        kernel_a
            .execute(&GeometryOp::Union {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            })
            .unwrap();

        // Extract warm state
        let state = kernel_a.warm_state().expect("should have warm state");

        // Create fresh kernel B and restore
        let mut kernel_b = OcctKernel::new();
        kernel_b.with_warm_state(state);

        // Verify box volume (~6000)
        let vol_box = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol_box {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Verify cylinder volume (~pi*25*20 ≈ 1570.8)
        let vol_cyl = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(2)))
            .unwrap();
        match vol_cyl {
            Value::Real(v) => assert!((v - 1570.8).abs() < 1.0, "cyl vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Verify union volume (positive)
        let vol_union = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(3)))
            .unwrap();
        match vol_union {
            Value::Real(v) => assert!(v > 0.0, "union vol should be positive: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Verify next_id restored: new sphere should get handle ID 4
        let sphere_h = kernel_b
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(5.0),
            })
            .unwrap();
        assert_eq!(
            sphere_h.id,
            GeometryHandleId(4),
            "next_id should be restored"
        );
    }

    #[test]
    fn with_warm_state_ignores_wrong_type() {
        let mut kernel = OcctKernel::new();
        // Pass a String instead of OcctWarmState — should be silently ignored
        kernel.with_warm_state(OpaqueState::new(String::from("garbage"), 8));
        // Kernel should still function normally
        let gh = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();
        let vol = kernel.query(&GeometryQuery::Volume(gh.id)).unwrap();
        match vol {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    #[test]
    fn with_warm_state_preserves_state_on_total_deserialization_failure() {
        // Create a kernel with a box
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Verify box works
        let vol = kernel.query(&GeometryQuery::Volume(box_h.id)).unwrap();
        match &vol {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Construct a corrupted OcctWarmState manually
        let mut corrupted_shapes = HashMap::new();
        corrupted_shapes.insert(1, "INVALID_BREP_DATA".to_string());
        corrupted_shapes.insert(2, "ALSO_GARBAGE".to_string());
        let corrupted_warm = OcctWarmState {
            shapes: corrupted_shapes,
            next_id: 99,
        };
        let corrupted_state = OpaqueState::new(corrupted_warm, 64);

        // Apply corrupted warm state — should NOT destroy existing state
        kernel.with_warm_state(corrupted_state);

        // The original box should still be queryable
        let vol_after = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol_after {
            Value::Real(v) => assert!(
                (v - 6000.0).abs() < 1.0,
                "box volume should survive corrupted warm state, got {v}"
            ),
            other => panic!("expected Real, got {:?}", other),
        }

        // next_id should NOT have been advanced to 99 — new sphere should get ID 2
        let sphere_h = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(5.0),
            })
            .unwrap();
        assert_eq!(
            sphere_h.id,
            GeometryHandleId(2),
            "next_id should not advance on total deserialization failure"
        );
    }

    #[test]
    fn with_warm_state_partial_deserialization_replaces_state() {
        // Create a helper kernel to get a valid cylinder BRep string
        let mut helper = OcctKernel::new();
        helper
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let helper_state = helper.warm_state().expect("helper should have warm state");
        let helper_warm = helper_state
            .downcast::<OcctWarmState>()
            .expect("should downcast");
        let valid_cylinder_brep = helper_warm
            .shapes
            .get(&1)
            .expect("handle 1 should exist")
            .clone();

        // Create main kernel with a box
        let mut kernel = OcctKernel::new();
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Verify it's a box (volume ~6000)
        let vol_before = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match &vol_before {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Construct partially-corrupted warm state:
        // handle 1 = valid cylinder BRep, handle 2 = corrupt data
        let mut partial_shapes = HashMap::new();
        partial_shapes.insert(1, valid_cylinder_brep);
        partial_shapes.insert(2, "CORRUPT".to_string());
        let partial_warm = OcctWarmState {
            shapes: partial_shapes,
            next_id: 10,
        };
        let partial_state = OpaqueState::new(partial_warm, 64);

        // Apply partially-corrupted warm state
        kernel.with_warm_state(partial_state);

        // Handle 1 should now be a cylinder (not a box)
        let vol_after = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol_after {
            Value::Real(v) => {
                // Cylinder volume = pi * r^2 * h = pi * 25 * 20 ≈ 1570.8
                assert!(
                    (v - 1570.8).abs() < 1.0,
                    "handle 1 should be cylinder volume ~1570.8, got {v}"
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }

        // Handle 2 should NOT exist (corrupt, was not restored)
        let result = kernel.query(&GeometryQuery::Volume(GeometryHandleId(2)));
        assert!(
            result.is_err(),
            "handle 2 should not exist (corrupt BRep was skipped)"
        );

        // next_id should be updated to 10 (swap occurred)
        let new_h = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(3.0),
            })
            .unwrap();
        assert_eq!(
            new_h.id,
            GeometryHandleId(10),
            "next_id should be updated to 10 after partial restore"
        );
    }

    #[test]
    fn with_warm_state_partial_failure_logs_warning() {
        // Create a helper kernel to get a valid cylinder BRep string
        let mut helper = OcctKernel::new();
        helper
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let helper_state = helper.warm_state().expect("helper should have warm state");
        let helper_warm = helper_state
            .downcast::<OcctWarmState>()
            .expect("should downcast");
        let valid_brep = helper_warm
            .shapes
            .get(&1)
            .expect("handle 1 should exist")
            .clone();

        // Construct warm state: 1 valid + 1 corrupt
        let mut shapes = HashMap::new();
        shapes.insert(1, valid_brep);
        shapes.insert(2, "CORRUPT_DATA".to_string());
        let warm = OcctWarmState {
            shapes,
            next_id: 10,
        };
        let state = OpaqueState::new(warm, 64);

        let mut kernel = OcctKernel::new();
        kernel.with_warm_state(state);

        // The valid shape should be restored
        let vol = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1570.8).abs() < 1.0,
                    "handle 1 should be cylinder volume ~1570.8, got {v}"
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }

        // The failure counter should report 1 failed deserialization
        assert_eq!(
            kernel.warm_start_failures(),
            1,
            "should report 1 failed deserialization"
        );
    }

    #[test]
    fn with_warm_state_all_valid_zero_failures() {
        // Create a kernel with a box and extract its warm state
        let mut source = OcctKernel::new();
        source
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();
        let state = source.warm_state().expect("should have warm state");

        // Apply that fully-valid warm state to a fresh kernel
        let mut kernel = OcctKernel::new();
        kernel.with_warm_state(state);

        // All shapes were valid, so failure count should be 0
        assert_eq!(
            kernel.warm_start_failures(),
            0,
            "fully valid warm state should report 0 failures"
        );

        // Verify the shape was actually restored
        let vol = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 6000.0).abs() < 1.0,
                    "box volume should be ~6000, got {v}"
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }
    }

    #[test]
    fn get_shape_null_ptr_returns_error_not_panic() {
        let mut kernel = OcctKernel::new();
        // Create a valid box (id=1)
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Inject a null shape at id=42
        kernel.insert_null_shape(42);

        // Valid shape should still be accessible
        assert!(kernel.get_shape(GeometryHandleId(1)).is_ok());

        // Null shape should return Err(OperationFailed), not panic
        let result = kernel.get_shape(GeometryHandleId(42));
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("null"),
                    "error should mention null, got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for null shape, got Ok"),
        }

        // Non-existent key should still return InvalidReference
        match kernel.get_shape(GeometryHandleId(999)) {
            Err(GeometryError::InvalidReference(id)) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            Err(other) => panic!("expected InvalidReference, got {:?}", other),
            Ok(_) => panic!("expected error for missing shape, got Ok"),
        }
    }

    #[test]
    fn warm_state_skips_null_shapes() {
        let mut kernel = OcctKernel::new();

        // Create a valid box (id=1)
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // Inject a null shape at id=2
        kernel.insert_null_shape(2);

        // warm_state should NOT panic and should return Some (valid shape exists)
        let state = kernel.warm_state();
        assert!(state.is_some(), "should serialize the valid shape");

        // Roundtrip: restore in a new kernel
        let mut kernel_b = OcctKernel::new();
        kernel_b.with_warm_state(state.unwrap());

        // Valid box should be preserved (volume ~6000)
        let vol = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Null shape (id=2) should NOT be present in restored kernel
        let result = kernel_b.query(&GeometryQuery::Volume(GeometryHandleId(2)));
        assert!(result.is_err(), "null shape should not survive warm-start");
    }

    #[test]
    fn execute_box_zero_dimension_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(0.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-width box"),
        }
    }

    #[test]
    fn execute_box_negative_dimension_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(-5.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for negative-width box"),
        }
    }

    #[test]
    fn execute_cylinder_zero_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.0),
            height: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-radius cylinder"),
        }
    }

    #[test]
    fn execute_cylinder_negative_height_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(-1.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for negative-height cylinder"),
        }
    }

    #[test]
    fn execute_sphere_zero_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(0.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-radius sphere"),
        }
    }

    #[test]
    fn execute_sphere_negative_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(-1.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for negative-radius sphere"),
        }
    }

    #[test]
    fn execute_fillet_zero_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        // Create a valid box first
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Fillet {
            target: box_h.id,
            radius: Value::Real(0.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-radius fillet"),
        }
    }

    // --- NaN / Infinity parameter rejection tests (step-9) ---

    #[test]
    fn execute_box_nan_dimension_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(f64::NAN),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN-width box"),
        }
    }

    #[test]
    fn execute_box_infinity_dimension_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(f64::INFINITY),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity-width box"),
        }
    }

    #[test]
    fn execute_box_neg_infinity_dimension_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(f64::NEG_INFINITY),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for neg-infinity-width box"),
        }
    }

    #[test]
    fn execute_cylinder_nan_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Cylinder {
            radius: Value::Real(f64::NAN),
            height: Value::Real(10.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN-radius cylinder"),
        }
    }

    #[test]
    fn execute_cylinder_infinity_height_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(f64::INFINITY),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity-height cylinder"),
        }
    }

    #[test]
    fn execute_sphere_nan_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(f64::NAN),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN-radius sphere"),
        }
    }

    #[test]
    fn execute_sphere_infinity_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(f64::INFINITY),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity-radius sphere"),
        }
    }

    #[test]
    fn execute_fillet_nan_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Fillet {
            target: box_h.id,
            radius: Value::Real(f64::NAN),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN-radius fillet"),
        }
    }

    #[test]
    fn execute_fillet_infinity_radius_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Fillet {
            target: box_h.id,
            radius: Value::Real(f64::INFINITY),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity-radius fillet"),
        }
    }

    // --- Chamfer distance validation tests ---

    #[test]
    fn execute_chamfer_zero_distance_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Chamfer {
            target: box_h.id,
            distance: Value::Real(0.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-distance chamfer"),
        }
    }

    #[test]
    fn execute_chamfer_negative_distance_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Chamfer {
            target: box_h.id,
            distance: Value::Real(-1.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for negative-distance chamfer"),
        }
    }

    #[test]
    fn execute_chamfer_nan_distance_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Chamfer {
            target: box_h.id,
            distance: Value::Real(f64::NAN),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN-distance chamfer"),
        }
    }

    #[test]
    fn execute_chamfer_infinity_distance_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Chamfer {
            target: box_h.id,
            distance: Value::Real(f64::INFINITY),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity-distance chamfer"),
        }
    }

    // --- Rotate NaN/infinity/zero-axis rejection tests (step-3) ---

    #[test]
    fn execute_rotate_nan_angle_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: f64::NAN,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN angle in rotate"),
        }
    }

    #[test]
    fn execute_rotate_infinity_axis_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [f64::INFINITY, 0.0, 0.0],
            angle_rad: 1.0,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity axis in rotate"),
        }
    }

    #[test]
    fn execute_rotate_zero_axis_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [0.0, 0.0, 0.0],
            angle_rad: 1.0,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for zero-length axis in rotate"),
        }
    }

    #[test]
    fn execute_rotate_near_zero_axis_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // A near-zero axis (mag_sq = 1e-34, non-zero but physically meaningless)
        // should be rejected just like exact zero
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [1e-17, 0.0, 0.0],
            angle_rad: 1.0,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for near-zero-length axis in rotate"),
        }
    }

    // --- Near-degenerate axis rejection tests (task-311 step-13) ---

    #[test]
    fn rotate_near_degenerate_axis_rejected() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // axis=[1e-10, 0, 0] has mag_sq=1e-20, physically meaningless (0.1 nanometer)
        // but above the current threshold of f64::EPSILON^2 ≈ 4.9e-32
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [1e-10, 0.0, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_4,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                let lower = msg.to_lowercase();
                assert!(
                    lower.contains("zero"),
                    "error should mention 'zero', got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for near-degenerate axis [1e-10, 0, 0] in rotate"),
        }
    }

    #[test]
    fn rotate_around_near_degenerate_axis_rejected() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // axis=[0, 1e-8, 0] has mag_sq=1e-16, physically meaningless (10 nanometers)
        // but above the current threshold of f64::EPSILON^2 ≈ 4.9e-32
        let result = kernel.execute(&GeometryOp::RotateAround {
            target: box_h.id,
            point: [5.0, 0.0, 0.0],
            axis: [0.0, 1e-8, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_4,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                let lower = msg.to_lowercase();
                assert!(
                    lower.contains("zero"),
                    "error should mention 'zero', got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => {
                panic!("expected error for near-degenerate axis [0, 1e-8, 0] in rotate_around")
            }
        }
    }

    // --- Error message quality regression tests (step-7) ---

    #[test]
    fn translate_error_message_mentions_finite() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: f64::NAN,
            dy: 0.0,
            dz: 0.0,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("finite"),
                    "translate error should mention 'finite', got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn rotate_error_message_mentions_axis_or_finite() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [0.0, 0.0, 0.0],
            angle_rad: 1.0,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                let lower = msg.to_lowercase();
                assert!(
                    lower.contains("axis") || lower.contains("zero"),
                    "rotate zero-axis error should mention 'axis' or 'zero', got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error"),
        }
    }

    // --- Fillet too-large radius regression test (step-5) ---

    #[test]
    fn execute_fillet_radius_too_large_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Radius 100.0 is much larger than any edge of the 10x10x10 box
        let result = kernel.execute(&GeometryOp::Fillet {
            target: box_h.id,
            radius: Value::Real(100.0),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for too-large fillet radius"),
        }
    }

    // --- Translate NaN/infinity rejection tests (step-1) ---

    #[test]
    fn execute_translate_nan_dx_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: f64::NAN,
            dy: 0.0,
            dz: 0.0,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for NaN dx in translate"),
        }
    }

    #[test]
    fn execute_translate_infinity_dy_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 0.0,
            dy: f64::INFINITY,
            dz: 0.0,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for infinity dy in translate"),
        }
    }

    #[test]
    fn execute_translate_neg_infinity_dz_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 0.0,
            dy: 0.0,
            dz: f64::NEG_INFINITY,
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for neg-infinity dz in translate"),
        }
    }

    #[test]
    fn brep_serialization_roundtrip() {
        // Create a box shape
        let shape = ffi::ffi::make_box(10.0, 20.0, 30.0).unwrap();

        // Serialize to BRep
        let brep = ffi::ffi::serialize_brep(&shape).unwrap();
        assert!(
            !brep.is_empty(),
            "BRep serialization should produce non-empty output"
        );

        // Deserialize from BRep
        cxx::let_cxx_string!(brep_cxx = brep.as_str());
        let restored = ffi::ffi::deserialize_brep(&brep_cxx).unwrap();

        // Query volume of deserialized shape
        let vol = ffi::ffi::query_volume(&restored).unwrap();
        assert!(
            (vol - 6000.0).abs() < 1.0,
            "expected volume ~6000, got {vol}"
        );
    }

    // --- Pattern tests ---

    #[test]
    fn linear_pattern_4_boxes() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box (volume = 1000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Apply LinearPattern: 4 copies along X with spacing 20
        let pattern_h = kernel
            .execute(&GeometryOp::LinearPattern {
                target: box_h.id,
                direction: [1.0, 0.0, 0.0],
                count: 4,
                spacing: Value::Real(20.0),
            })
            .unwrap();
        // Volume should be approximately 4 * 1000 = 4000
        let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 4000.0).abs() < 50.0,
                    "expected linear pattern volume ~4000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn circular_pattern_6_instances() {
        let mut kernel = OcctKernel::new();
        // Create a small 5x5x5 box (volume = 125)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(5.0),
                height: Value::Real(5.0),
                depth: Value::Real(5.0),
            })
            .unwrap();
        // Translate to (20,0,0) to offset from center
        let translated_h = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 20.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        // Circular pattern: 6 instances, full circle (2*PI)
        let pattern_h = kernel
            .execute(&GeometryOp::CircularPattern {
                target: translated_h.id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                count: 6,
                angle: Value::Real(2.0 * std::f64::consts::PI),
            })
            .unwrap();
        // Volume should be approximately 6 * 125 = 750
        let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 750.0).abs() < 50.0,
                    "expected circular pattern volume ~750, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn linear_pattern_2d_3x4_grid() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box (volume = 1000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Apply LinearPattern2D: 3×4 grid along X and Y with 20mm spacing
        let pattern_h = kernel
            .execute(&GeometryOp::LinearPattern2D {
                target: box_h.id,
                direction1: [1.0, 0.0, 0.0],
                count1: 3,
                spacing1: Value::Real(20.0),
                direction2: [0.0, 1.0, 0.0],
                count2: 4,
                spacing2: Value::Real(20.0),
            })
            .unwrap();
        // Volume should be approximately 3*4 * 1000 = 12000
        let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 12000.0).abs() < 200.0,
                    "expected linear_pattern_2d volume ~12000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn arbitrary_pattern_3_transforms() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box (volume = 1000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Apply ArbitraryPattern with 3 non-overlapping translations
        let pattern_h = kernel
            .execute(&GeometryOp::ArbitraryPattern {
                target: box_h.id,
                transforms: vec![
                    [20.0, 0.0, 0.0],
                    [0.0, 20.0, 0.0],
                    [20.0, 20.0, 0.0],
                ],
            })
            .unwrap();
        // Volume should be approximately 4 * 1000 = 4000 (original + 3 copies)
        let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 4000.0).abs() < 200.0,
                    "expected arbitrary_pattern volume ~4000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn linear_pattern_2d_count1_zero_errors() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::LinearPattern2D {
            target: box_h.id,
            direction1: [1.0, 0.0, 0.0],
            count1: 0,
            spacing1: Value::Real(20.0),
            direction2: [0.0, 1.0, 0.0],
            count2: 3,
            spacing2: Value::Real(20.0),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("count1"),
                    "error should mention count1, got: {msg}"
                );
            }
            other => panic!("expected OperationFailed for count1==0, got {:?}", other),
        }
    }

    #[test]
    fn linear_pattern_2d_count2_zero_errors() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::LinearPattern2D {
            target: box_h.id,
            direction1: [1.0, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(20.0),
            direction2: [0.0, 1.0, 0.0],
            count2: 0,
            spacing2: Value::Real(20.0),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("count2"),
                    "error should mention count2, got: {msg}"
                );
            }
            other => panic!("expected OperationFailed for count2==0, got {:?}", other),
        }
    }

    #[test]
    fn linear_pattern_2d_nan_direction_errors() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // NaN in direction1 should fail
        let result = kernel.execute(&GeometryOp::LinearPattern2D {
            target: box_h.id,
            direction1: [f64::NAN, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(20.0),
            direction2: [0.0, 1.0, 0.0],
            count2: 3,
            spacing2: Value::Real(20.0),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("direction1"),
                    "error should mention direction1, got: {msg}"
                );
            }
            other => panic!("expected OperationFailed for NaN direction1, got {:?}", other),
        }
        // Inf in direction2 should fail
        let result = kernel.execute(&GeometryOp::LinearPattern2D {
            target: box_h.id,
            direction1: [1.0, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(20.0),
            direction2: [0.0, f64::INFINITY, 0.0],
            count2: 3,
            spacing2: Value::Real(20.0),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("direction2"),
                    "error should mention direction2, got: {msg}"
                );
            }
            other => panic!("expected OperationFailed for Inf direction2, got {:?}", other),
        }
    }

    #[test]
    fn arbitrary_pattern_zero_transforms_errors() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::ArbitraryPattern {
            target: box_h.id,
            transforms: vec![],
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("transform"),
                    "error should mention transform, got: {msg}"
                );
            }
            other => panic!(
                "expected OperationFailed for empty transforms, got {:?}",
                other
            ),
        }
    }

    // --- Loft tests ---

    #[test]
    fn loft_two_circles_creates_solid() {
        let mut kernel = OcctKernel::new();
        // Create two circle wire profiles at different heights using the FFI helper.
        // make_circle_wire creates a TopoDS_Wire circle profile.
        let wire1 = ffi::ffi::make_circle_wire(10.0, 0.0)
            .expect("make_circle_wire should work for profile 1");
        let id1 = kernel.store_raw(wire1);

        let wire2 = ffi::ffi::make_circle_wire(5.0, 30.0)
            .expect("make_circle_wire should work for profile 2");
        let id2 = kernel.store_raw(wire2);

        // Loft through both profiles
        let loft_h = kernel
            .execute(&GeometryOp::Loft {
                profiles: vec![id1, id2],
            })
            .unwrap();

        // Query volume - should be positive (a cone-like solid)
        let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    v > 100.0,
                    "loft volume should be positive and significant, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn loft_one_profile_returns_error() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let w1 = ffi::ffi::make_circle_wire(10.0, 0.0).expect("wire1");
        let id1 = kernel.store_raw(w1);

        let result = kernel.execute(&GeometryOp::Loft {
            profiles: vec![id1],
        });
        assert!(result.is_err(), "loft with 1 profile should fail");
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("at least 2"),
            "error should mention 'at least 2', got: {err_msg}"
        );
    }

    #[test]
    fn loft_two_different_circles_cone_like() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let w1 = ffi::ffi::make_circle_wire(10.0, 0.0).expect("wire1");
        let id1 = kernel.store_raw(w1);
        let w2 = ffi::ffi::make_circle_wire(2.0, 20.0).expect("wire2");
        let id2 = kernel.store_raw(w2);

        let loft_h = kernel
            .execute(&GeometryOp::Loft {
                profiles: vec![id1, id2],
            })
            .unwrap();

        let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                // Should be between small cylinder (pi*4*20 ~= 251) and
                // large cylinder (pi*100*20 ~= 6283): a cone-like frustum
                assert!(
                    v > 251.0 && v < 6283.0,
                    "cone-like frustum volume should be between 251 and 6283, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn loft_four_circles_creates_solid() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Create 4 circle wire profiles at different heights with decreasing radii
        let w1 = ffi::ffi::make_circle_wire(10.0, 0.0).expect("wire1");
        let id1 = kernel.store_raw(w1);
        let w2 = ffi::ffi::make_circle_wire(8.0, 10.0).expect("wire2");
        let id2 = kernel.store_raw(w2);
        let w3 = ffi::ffi::make_circle_wire(6.0, 20.0).expect("wire3");
        let id3 = kernel.store_raw(w3);
        let w4 = ffi::ffi::make_circle_wire(4.0, 30.0).expect("wire4");
        let id4 = kernel.store_raw(w4);

        // Loft through all 4 profiles
        let loft_h = kernel
            .execute(&GeometryOp::Loft {
                profiles: vec![id1, id2, id3, id4],
            })
            .unwrap();

        // Query volume — should fall between the smallest-enclosing cylinder
        // (π·r_min²·h_total = π·16·30 ≈ 1508) and the largest-enclosing
        // cylinder (π·r_max²·h_total = π·100·30 ≈ 9425). Expected OCCT result
        // for this four-circle frustum stack is ≈ 4900, comfortably inside.
        // Pattern mirrors loft_two_different_circles_cone_like (task-383 S4c).
        let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    v > 1508.0 && v < 9425.0,
                    "loft frustum volume should be between 1508 (smallest cylinder) \
                     and 9425 (largest cylinder), got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    // --- Thicken / Shell tests ---

    #[test]
    fn thicken_solid_increases_volume() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box (volume = 1000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Thicken by offset=2.0 — result should have volume > 1000
        let thickened_h = kernel
            .execute(&GeometryOp::Thicken {
                target: box_h.id,
                offset: Value::Real(2.0),
            })
            .unwrap();
        let vol = kernel
            .query(&GeometryQuery::Volume(thickened_h.id))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    v > 1000.0,
                    "thickened volume should exceed original 1000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn shell_box_hollow() {
        let mut kernel = OcctKernel::new();
        // Create a 20x20x20 box (volume = 8000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(20.0),
                height: Value::Real(20.0),
                depth: Value::Real(20.0),
            })
            .unwrap();
        // Shell with thickness=1.0, remove face 0 to create hollow box
        let shell_h = kernel
            .execute(&GeometryOp::Shell {
                target: box_h.id,
                thickness: Value::Real(1.0),
                faces_to_remove: vec![0],
            })
            .unwrap();
        let vol = kernel.query(&GeometryQuery::Volume(shell_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                // Should be less than 8000 (solid) but greater than 0
                assert!(
                    v > 0.0 && v < 8000.0,
                    "shell volume should be between 0 and 8000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    // --- Query tests ---

    #[test]
    fn distance_between_shapes() {
        let mut kernel = OcctKernel::new();
        // Create box_a at origin (10x10x10, centered)
        let box_a = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Create box_b and translate to (50,0,0)
        let box_b_raw = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let box_b = kernel
            .execute(&GeometryOp::Translate {
                target: box_b_raw.id,
                dx: 50.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        // Distance between them: box_a goes from -5 to 5 on X,
        // box_b goes from 45 to 55 on X, so gap = 40
        let dist = kernel
            .query(&GeometryQuery::Distance {
                from: box_a.id,
                to: box_b.id,
            })
            .unwrap();
        match dist {
            Value::Real(d) => {
                assert!((d - 40.0).abs() < 1.0, "expected distance ~40, got {d}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn moment_of_inertia_box() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Query MoI around Z axis
        let moi = kernel
            .query(&GeometryQuery::MomentOfInertia {
                handle: box_h.id,
                axis: [0.0, 0.0, 1.0],
            })
            .unwrap();
        match moi {
            Value::Real(v) => {
                // For a box of uniform density (mass=volume=1000),
                // MoI around Z through centroid = M/12 * (w^2 + d^2) = 1000/12 * (100 + 100)
                // = 1000/12 * 200 ≈ 16666.7
                assert!(v > 0.0, "MoI should be positive, got {v}");
                assert!(
                    (v - 16666.7).abs() < 100.0,
                    "expected MoI ~16666.7, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    // --- Integration tests ---

    #[test]
    fn new_ops_export_step() {
        let mut kernel = OcctKernel::new();
        // Mirror
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let translated = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 15.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        let mirrored = kernel
            .execute(&GeometryOp::Mirror {
                target: translated.id,
                plane_origin: [0.0, 0.0, 0.0],
                plane_normal: [1.0, 0.0, 0.0],
            })
            .unwrap();
        let mut buf = Vec::new();
        kernel
            .export(mirrored.id, ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(
            content.contains("ISO-10303-21"),
            "STEP should contain ISO header"
        );

        // LinearPattern
        let pat = kernel
            .execute(&GeometryOp::LinearPattern {
                target: box_h.id,
                direction: [1.0, 0.0, 0.0],
                count: 3,
                spacing: Value::Real(15.0),
            })
            .unwrap();
        let mut buf2 = Vec::new();
        kernel
            .export(pat.id, ExportFormat::Step, &mut buf2)
            .unwrap();
        let content2 = String::from_utf8(buf2).unwrap();
        assert!(
            content2.contains("ISO-10303-21"),
            "pattern STEP should contain ISO header"
        );
    }

    #[test]
    fn new_ops_tessellate() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let translated = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 15.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        let mirrored = kernel
            .execute(&GeometryOp::Mirror {
                target: translated.id,
                plane_origin: [0.0, 0.0, 0.0],
                plane_normal: [1.0, 0.0, 0.0],
            })
            .unwrap();
        let mesh = kernel.tessellate(mirrored.id, 0.1).unwrap();
        assert!(
            !mesh.vertices.is_empty(),
            "mirrored tessellation should have vertices"
        );
        assert!(
            mesh.indices.len().is_multiple_of(3),
            "indices should be divisible by 3"
        );

        // Loft
        let w1 = ffi::ffi::make_circle_wire(10.0, 0.0).unwrap();
        let id1 = kernel.store_raw(w1);
        let w2 = ffi::ffi::make_circle_wire(5.0, 20.0).unwrap();
        let id2 = kernel.store_raw(w2);
        let loft = kernel
            .execute(&GeometryOp::Loft {
                profiles: vec![id1, id2],
            })
            .unwrap();
        let loft_mesh = kernel.tessellate(loft.id, 0.1).unwrap();
        assert!(
            !loft_mesh.vertices.is_empty(),
            "loft tessellation should have vertices"
        );
        assert!(
            loft_mesh.indices.len().is_multiple_of(3),
            "loft indices divisible by 3"
        );
    }

    #[test]
    fn pattern_plus_boolean() {
        let mut kernel = OcctKernel::new();
        // Create plate: 100x60x10
        let plate = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(100.0),
                height: Value::Real(60.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let plate_vol = match kernel.query(&GeometryQuery::Volume(plate.id)).unwrap() {
            Value::Real(v) => v,
            _ => panic!("expected Real"),
        };

        // Create hole: small cylinder
        let hole = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(3.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        // Move hole down so it passes through the plate
        let hole_pos = kernel
            .execute(&GeometryOp::Translate {
                target: hole.id,
                dx: -30.0,
                dy: 0.0,
                dz: -10.0,
            })
            .unwrap();

        // Pattern: 5 holes along X
        let patterned = kernel
            .execute(&GeometryOp::LinearPattern {
                target: hole_pos.id,
                direction: [1.0, 0.0, 0.0],
                count: 5,
                spacing: Value::Real(15.0),
            })
            .unwrap();

        // Boolean difference: plate minus patterned holes
        let result = kernel
            .execute(&GeometryOp::Difference {
                left: plate.id,
                right: patterned.id,
            })
            .unwrap();

        let result_vol = match kernel.query(&GeometryQuery::Volume(result.id)).unwrap() {
            Value::Real(v) => v,
            _ => panic!("expected Real"),
        };
        assert!(
            result_vol < plate_vol,
            "plate with holes ({result_vol}) should have less volume than solid plate ({plate_vol})"
        );
        assert!(result_vol > 0.0, "result should have positive volume");
    }

    // --- Draft tests ---

    #[test]
    fn draft_angle_on_box() {
        let mut kernel = OcctKernel::new();
        // Create a 20x20x20 box
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(20.0),
                height: Value::Real(20.0),
                depth: Value::Real(20.0),
            })
            .unwrap();
        // Create a plane reference (small flat box at z=0)
        let plane_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(100.0),
                height: Value::Real(100.0),
                depth: Value::Real(0.1),
            })
            .unwrap();
        // Apply Draft with angle ~5.7 degrees (0.1 rad)
        let draft_h = kernel.execute(&GeometryOp::Draft {
            target: box_h.id,
            angle: Value::Real(0.1),
            plane: plane_h.id,
        });
        // Draft is complex and may fail for certain shapes - we just verify it
        // either succeeds with a positive volume or returns an expected error
        match draft_h {
            Ok(h) => {
                let vol = kernel.query(&GeometryQuery::Volume(h.id)).unwrap();
                match vol {
                    Value::Real(v) => {
                        assert!(v > 0.0, "drafted volume should be positive, got {v}");
                    }
                    other => panic!("expected Value::Real, got {:?}", other),
                }
            }
            Err(GeometryError::OperationFailed(_)) => {
                // Acceptable: Draft is finicky with some shapes
            }
            Err(other) => panic!("unexpected error: {:?}", other),
        }
    }

    // --- Mirror tests ---

    #[test]
    fn mirror_across_yz_plane() {
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box (volume = 1000)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Translate box to (5,0,0) so it's off-center
        let translated_h = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 10.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        // Mirror across YZ plane (origin=[0,0,0], normal=[1,0,0])
        let mirrored_h = kernel
            .execute(&GeometryOp::Mirror {
                target: translated_h.id,
                plane_origin: [0.0, 0.0, 0.0],
                plane_normal: [1.0, 0.0, 0.0],
            })
            .unwrap();
        // Fuse original and mirrored
        let fused_h = kernel
            .execute(&GeometryOp::Union {
                left: translated_h.id,
                right: mirrored_h.id,
            })
            .unwrap();
        // Volume should be approximately 2 * 1000 = 2000
        let vol = kernel.query(&GeometryQuery::Volume(fused_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 2000.0).abs() < 10.0,
                    "expected fused mirror volume ~2000, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn mirror_near_zero_normal_rejected() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // plane_normal=[1e-20, 0, 0] has mag_sq=1e-40, physically meaningless.
        // Should be rejected like near-degenerate axes in rotate/revolve.
        let result = kernel.execute(&GeometryOp::Mirror {
            target: box_h.id,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1e-20, 0.0, 0.0],
        });
        assert_operation_fails_with(result, "zero");
    }

    #[test]
    fn make_circle_wire_rejects_degenerate_radius() {
        // Radius of 0 should cause MakeEdge to fail; verify we get an error
        // rather than a silently invalid shape.
        let result = ffi::ffi::make_circle_wire(0.0, 0.0);
        assert!(
            result.is_err(),
            "make_circle_wire(0.0, 0.0) should return Err, got Ok"
        );
    }

    #[test]
    fn make_circle_wire_valid_produces_wire() {
        // A valid radius should produce a usable wire shape without error.
        let wire = ffi::ffi::make_circle_wire(10.0, 0.0);
        assert!(
            wire.is_ok(),
            "make_circle_wire(10.0, 0.0) should succeed, got {:?}",
            wire.err()
        );
    }

    // --- Scale tests (task-311 step-5) ---

    #[test]
    fn scale_doubles_volume() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box, volume = 1000
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Scale by 2: linear dimensions double, volume becomes 8x = 8000
        let scaled_h = kernel
            .execute(&GeometryOp::Scale {
                target: box_h.id,
                factor: 2.0,
            })
            .unwrap();
        let vol = kernel.query(&GeometryQuery::Volume(scaled_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 8000.0).abs() < 1.0,
                    "scale(2.0) should give volume ≈ 8000, got {v}"
                );
            }
            other => panic!("expected Value::Real for volume, got {:?}", other),
        }
    }

    #[test]
    fn scale_identity_preserves_volume() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let scaled_h = kernel
            .execute(&GeometryOp::Scale {
                target: box_h.id,
                factor: 1.0,
            })
            .unwrap();
        let vol = kernel.query(&GeometryQuery::Volume(scaled_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1000.0).abs() < 1.0,
                    "scale(1.0) should preserve volume ≈ 1000, got {v}"
                );
            }
            other => panic!("expected Value::Real for volume, got {:?}", other),
        }
    }

    #[test]
    fn scale_zero_factor_rejected() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Scale {
            target: box_h.id,
            factor: 0.0,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("non-zero") || msg.contains("finite"),
                    "error should mention non-zero/finite constraint, got: {msg}"
                );
            }
            other => panic!(
                "expected GeometryError::OperationFailed for zero scale factor, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn scale_nan_factor_rejected() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Scale {
            target: box_h.id,
            factor: f64::NAN,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("finite") || msg.contains("non-zero"),
                    "error should mention finite constraint, got: {msg}"
                );
            }
            other => panic!(
                "expected GeometryError::OperationFailed for NaN scale factor, got {:?}",
                other
            ),
        }
    }

    // --- RotateAround tests (task-311 step-7) ---

    #[test]
    fn rotate_around_non_origin_differs_from_rotate_at_origin() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Create a 10x10x10 box centered at origin.
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();

        // Rotate around Z through origin by PI/2 — centroid stays at (0,0,0).
        let rotated_origin = kernel
            .execute(&GeometryOp::Rotate {
                target: box_h.id,
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();

        // Rotate around Z through (50, 0, 0) by PI/2 — centroid moves.
        let rotated_around = kernel
            .execute(&GeometryOp::RotateAround {
                target: box_h.id,
                point: [50.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();

        // Both should succeed (volumes must be preserved).
        let vol_origin = kernel
            .query(&GeometryQuery::Volume(rotated_origin.id))
            .unwrap();
        let vol_around = kernel
            .query(&GeometryQuery::Volume(rotated_around.id))
            .unwrap();

        match (vol_origin, vol_around) {
            (Value::Real(v1), Value::Real(v2)) => {
                assert!(
                    (v1 - 1000.0).abs() < 1.0,
                    "rotate-at-origin should preserve volume ≈ 1000, got {v1}"
                );
                assert!(
                    (v2 - 1000.0).abs() < 1.0,
                    "rotate-around-point should preserve volume ≈ 1000, got {v2}"
                );
            }
            other => panic!("expected (Value::Real, Value::Real), got {:?}", other),
        }

        // The centroids should be different: rotate_around (point=(50,0,0), axis=Z, angle=PI/2)
        // moves centroid of origin-centered box from (0,0,0) to roughly (-50, 50, 0).
        let centroid_around = kernel
            .query(&GeometryQuery::Centroid(rotated_around.id))
            .unwrap();
        match centroid_around {
            Value::String(s) => {
                let x_start = s.find("\"x\":").unwrap() + 4;
                let x_end = s[x_start..].find([',', '}']).unwrap() + x_start;
                let x: f64 = s[x_start..x_end].parse().unwrap();
                // After rotating (0,0,0) 90° around Z through (50,0,0):
                // new position = (50,0,0) + Rz(PI/2) * (0-50, 0-0, 0-0)
                //              = (50,0,0) + Rz(PI/2) * (-50, 0, 0)
                //              = (50,0,0) + (0, -50, 0)  [Rz(PI/2)*(-1,0,0) = (0,-1,0)]
                //              = (50, -50, 0)
                // So x ≈ 50
                assert!(
                    (x - 50.0).abs() < 1.0,
                    "rotate_around centroid x should be ≈ 50, got {x}"
                );
            }
            other => panic!("expected String centroid, got {:?}", other),
        }
    }

    #[test]
    fn draft_uses_plane_shape_not_hardcoded_z() {
        // Verify that draft_shape extracts the neutral plane from plane_shape
        // rather than using a hardcoded Z-up direction. We test this by
        // drafting with a non-planar shape as plane — after the fix, this
        // should error with "does not contain a planar face" because the
        // code actually tries to extract the plane from the shape.
        // Currently FAILS because draft_shape ignores plane_shape, so a
        // sphere (non-planar) is accepted and draft proceeds with Z-up.
        let mut kernel = OcctKernel::new();

        // Target box
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(20.0),
                height: Value::Real(20.0),
                depth: Value::Real(20.0),
            })
            .unwrap();

        // Use a sphere as the "plane" — a sphere has no planar faces.
        // After fix: should get an explicit error about non-planar face.
        // Before fix: plane_shape is ignored, so draft either succeeds
        // with Z-up or fails for unrelated reasons.
        let sphere_h = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(10.0),
            })
            .unwrap();

        let result = kernel.execute(&GeometryOp::Draft {
            target: box_h.id,
            angle: Value::Real(0.05),
            plane: sphere_h.id,
        });

        // After the fix, we expect an OperationFailed error whose message
        // mentions "planar" — the code should detect that the sphere's
        // face is not planar and throw an explicit error.
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("planar"),
                    "expected error about non-planar face, got: {msg}"
                );
            }
            Ok(_) => {
                panic!(
                    "draft with sphere as plane should fail \
                     (sphere has no planar faces), but succeeded"
                );
            }
            Err(other) => {
                panic!(
                    "expected OperationFailed with 'planar' message, got: {:?}",
                    other
                );
            }
        }
    }

    // --- Extrude tests (task-308 step-7 + step-9) ---

    #[test]
    fn extrude_zero_distance_returns_error() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Create a box as a stand-in profile (provides a valid handle)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        // Extrude with zero distance should fail
        let result = kernel.execute(&GeometryOp::Extrude {
            profile: box_h.id,
            distance: Value::Real(0.0),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("zero"),
                    "expected error message containing 'zero', got: {msg}"
                );
            }
            Ok(_) => panic!("expected OperationFailed for zero distance, got Ok"),
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    #[test]
    fn extrude_nan_distance_returns_error() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Extrude {
            profile: box_h.id,
            distance: Value::Real(f64::NAN),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("finite"),
                    "expected error message containing 'finite', got: {msg}"
                );
            }
            Ok(_) => panic!("expected OperationFailed for NaN distance, got Ok"),
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    #[test]
    fn extrude_inf_distance_returns_error() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Extrude {
            profile: box_h.id,
            distance: Value::Real(f64::INFINITY),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("finite"),
                    "expected error message containing 'finite', got: {msg}"
                );
            }
            Ok(_) => panic!("expected OperationFailed for Inf distance, got Ok"),
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    #[test]
    fn extrude_circle_face_volume() {
        // Tests via direct FFI: make_circle_face → make_prism → query_volume.
        // Circle radius=5, extrude distance=10 → volume ≈ π * 5² * 10 = 785.4
        let face = ffi::ffi::make_circle_face(5.0, 0.0)
            .expect("make_circle_face should succeed for radius=5");
        let prism = ffi::ffi::make_prism(&face, 0.0, 0.0, 10.0)
            .expect("make_prism should succeed for circle face");
        let vol =
            ffi::ffi::query_volume(&prism).expect("query_volume should work for extruded circle");
        let expected = std::f64::consts::PI * 25.0 * 10.0; // π * r² * h
        let rel_err = (vol - expected).abs() / expected;
        assert!(
            rel_err < 0.01,
            "expected extrude circle volume ≈ {:.2}, got {:.2} (rel_err={:.4})",
            expected,
            vol,
            rel_err
        );
    }

    #[test]
    fn extrude_negative_distance_same_volume() {
        // Extruding in -Z should produce the same volume as +Z.
        let face = ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
        let prism_pos =
            ffi::ffi::make_prism(&face, 0.0, 0.0, 10.0).expect("make_prism +Z should succeed");
        let vol_pos = ffi::ffi::query_volume(&prism_pos).expect("query_volume +Z should work");

        let face2 = ffi::ffi::make_circle_face(5.0, 0.0)
            .expect("make_circle_face should succeed for negative test");
        let prism_neg =
            ffi::ffi::make_prism(&face2, 0.0, 0.0, -10.0).expect("make_prism -Z should succeed");
        let vol_neg = ffi::ffi::query_volume(&prism_neg).expect("query_volume -Z should work");

        let rel_diff = (vol_pos - vol_neg).abs() / vol_pos;
        assert!(
            rel_diff < 0.01,
            "positive and negative extrude should have same volume, got pos={:.2} neg={:.2}",
            vol_pos,
            vol_neg
        );
    }

    // --- Extrude kernel-path volume tests (task-308 step-14) ---
    // These exercise the full OcctKernel::execute() + query() path using
    // store_raw to inject FFI-created circle faces as kernel handles.

    #[test]
    fn extrude_circle_face_through_kernel() {
        // Store a circle face (radius=5) via store_raw, then extrude via execute().
        // Expected volume: π * r² * h = π * 25 * 10 = 785.4
        let mut kernel = OcctKernel::new();
        let face = ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
        let face_id = kernel.store_raw(face);

        let result = kernel
            .execute(&GeometryOp::Extrude {
                profile: face_id,
                distance: Value::Real(10.0),
            })
            .expect("Extrude through kernel should succeed");

        let vol = kernel
            .query(&GeometryQuery::Volume(result.id))
            .expect("Volume query should succeed");
        let vol_f64 = vol.as_f64().expect("Volume should be numeric");
        let expected = std::f64::consts::PI * 25.0 * 10.0;
        let rel_err = (vol_f64 - expected).abs() / expected;
        assert!(
            rel_err < 0.01,
            "expected extrude circle volume ≈ {:.2}, got {:.2} (rel_err={:.4})",
            expected,
            vol_f64,
            rel_err
        );
    }

    #[test]
    fn extrude_negative_distance_through_kernel() {
        // Negative distance should produce same volume as positive.
        let mut kernel = OcctKernel::new();

        let face_pos =
            ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
        let face_pos_id = kernel.store_raw(face_pos);
        let result_pos = kernel
            .execute(&GeometryOp::Extrude {
                profile: face_pos_id,
                distance: Value::Real(10.0),
            })
            .expect("Extrude +Z through kernel should succeed");
        let vol_pos = kernel
            .query(&GeometryQuery::Volume(result_pos.id))
            .expect("Volume query +Z should succeed")
            .as_f64()
            .expect("Volume should be numeric");

        let face_neg = ffi::ffi::make_circle_face(5.0, 0.0)
            .expect("make_circle_face should succeed for negative test");
        let face_neg_id = kernel.store_raw(face_neg);
        let result_neg = kernel
            .execute(&GeometryOp::Extrude {
                profile: face_neg_id,
                distance: Value::Real(-10.0),
            })
            .expect("Extrude -Z through kernel should succeed");
        let vol_neg = kernel
            .query(&GeometryQuery::Volume(result_neg.id))
            .expect("Volume query -Z should succeed")
            .as_f64()
            .expect("Volume should be numeric");

        let rel_diff = (vol_pos - vol_neg).abs() / vol_pos;
        assert!(
            rel_diff < 0.01,
            "positive and negative extrude should have same volume, got pos={:.2} neg={:.2}",
            vol_pos,
            vol_neg
        );
    }

    // --- Revolve FFI tests (task-309 step-1) ---

    #[test]
    fn make_rect_face_creates_valid_face() {
        // make_rect_face(width=10, height=5, cx=20, cy=0, cz=0) → area ≈ 50
        if !crate::OCCT_AVAILABLE {
            return;
        }
        let face = ffi::ffi::make_rect_face(10.0, 5.0, 20.0, 0.0, 0.0)
            .expect("make_rect_face should succeed");
        let area = ffi::ffi::query_area(&face).expect("query_area should work on rect face");
        let expected = 50.0;
        let rel_err = (area - expected).abs() / expected;
        assert!(
            rel_err < 0.01,
            "expected rect face area ≈ {:.2}, got {:.2} (rel_err={:.4})",
            expected,
            area,
            rel_err
        );
    }

    #[test]
    fn revolve_ffi_circle_face_full_rotation() {
        // Create a circle face in XY plane, rotate 90° around X to put it in XZ plane,
        // translate to offset 20 on X, then revolve around Z axis by 2π → torus.
        // Profile must be in a plane CONTAINING the revolution axis for a solid torus.
        if !crate::OCCT_AVAILABLE {
            return;
        }
        let face = ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
        // Rotate 90° around X axis: XY plane → XZ plane
        let rotated = ffi::ffi::rotate_shape(
            &face,
            1.0,
            0.0,
            0.0, // X axis
            std::f64::consts::FRAC_PI_2,
        )
        .expect("rotate_shape should succeed");
        // Translate 20 along X so centroid is offset from Z axis
        let translated = ffi::ffi::translate_shape(&rotated, 20.0, 0.0, 0.0)
            .expect("translate_shape should succeed");
        let revolved = ffi::ffi::make_revolve(
            &translated,
            0.0,
            0.0,
            0.0, // axis origin
            0.0,
            0.0,
            1.0, // axis direction (Z)
            std::f64::consts::TAU,
        )
        .expect("make_revolve should succeed for full rotation");
        let vol =
            ffi::ffi::query_volume(&revolved).expect("query_volume should work for revolved shape");
        assert!(
            vol > 0.0,
            "revolved circle face should have positive volume, got {}",
            vol
        );
    }

    // --- Revolve kernel error tests (task-309 step-5) ---

    // --- Revolve kernel volume tests (task-309 step-7) ---

    #[test]
    fn revolve_circle_face_full_volume() {
        // Pappus' theorem: V = 2πR × A where R = centroid-to-axis distance, A = profile area.
        // Circle face r=5 at offset R=20, revolve around Z axis by 2π → torus volume = 2π²Rr²
        let mut kernel = OcctKernel::new();
        let face_id = make_torus_profile(&mut kernel, 5.0, 20.0);

        let result = kernel
            .execute(&GeometryOp::Revolve {
                profile: face_id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::TAU,
            })
            .expect("Revolve full should succeed");
        let expected = 2.0 * std::f64::consts::PI.powi(2) * 20.0 * 25.0; // 2π²Rr²
        assert_volume_near(&mut kernel, result.id, expected, 0.02, "circle torus full");
    }

    #[test]
    fn revolve_half_angle_half_volume() {
        // Same setup as full volume but angle=π → half torus should be ~50% of full.
        let mut kernel = OcctKernel::new();
        let face_id = make_torus_profile(&mut kernel, 5.0, 20.0);

        let full = kernel
            .execute(&GeometryOp::Revolve {
                profile: face_id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::TAU,
            })
            .expect("Revolve full should succeed");
        let vol_full = kernel
            .query(&GeometryQuery::Volume(full.id))
            .expect("Volume query should succeed")
            .as_f64()
            .expect("Volume should be numeric");

        // Create another face for half revolution
        let face2_id = make_torus_profile(&mut kernel, 5.0, 20.0);

        let half = kernel
            .execute(&GeometryOp::Revolve {
                profile: face2_id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::PI,
            })
            .expect("Revolve half should succeed");
        let vol_half = kernel
            .query(&GeometryQuery::Volume(half.id))
            .expect("Volume query should succeed")
            .as_f64()
            .expect("Volume should be numeric");

        let ratio = vol_half / vol_full;
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "half-angle volume should be ~50% of full, got ratio {:.4} (full={:.2}, half={:.2})",
            ratio,
            vol_full,
            vol_half
        );
    }

    // --- Sweep tests ---

    #[test]
    fn sweep_solid_path_returns_error() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Create a box solid as (invalid) path
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        // Create a circle face as profile
        let face = ffi::ffi::make_circle_face(2.0, 0.0).expect("circle face");
        let profile_id = kernel.store_raw(face);

        let result = kernel.execute(&GeometryOp::Sweep {
            profile: profile_id,
            path: box_h.id,
        });
        assert!(result.is_err(), "sweep with a solid as path should fail");
    }

    #[test]
    fn revolve_rect_face_torus_volume() {
        // Rect face w=4, h=2, centered at (10, 0, 0) in XZ plane.
        // Pappus: V = 2π × R × A = 2π × 10 × (4×2) = 160π ≈ 502.65
        let mut kernel = OcctKernel::new();
        let face_id = make_rect_torus_profile(&mut kernel, 4.0, 2.0, 10.0);

        let result = kernel
            .execute(&GeometryOp::Revolve {
                profile: face_id,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::TAU,
            })
            .expect("Revolve rect should succeed");
        let expected = 2.0 * std::f64::consts::PI * 10.0 * (4.0 * 2.0); // 2πR×A = 160π
        assert_volume_near(&mut kernel, result.id, expected, 0.02, "rect torus full");
    }

    #[test]
    fn revolve_zero_angle_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: 0.0,
        });
        assert_operation_fails_with(result, "zero");
    }

    #[test]
    fn revolve_nan_params_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: f64::NAN,
        });
        assert_operation_fails_with(result, "finite");
    }

    #[test]
    fn revolve_zero_axis_dir_returns_error() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 0.0],
            angle_rad: std::f64::consts::TAU,
        });
        assert_operation_fails_with(result, "zero");
    }

    #[test]
    fn revolve_result_is_solid_not_shell() {
        // Regression test: make_revolve must always return a volumetric Solid,
        // never a Shell. A Shell shape returns 0 or near-0 for volume queries,
        // which would silently produce wrong results.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let face = ffi::ffi::make_rect_face(4.0, 2.0, 0.0, 0.0, 0.0)
            .expect("make_rect_face should succeed");
        // Rotate to XZ plane and translate to offset 10 from Z axis
        let rotated = ffi::ffi::rotate_shape(&face, 1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2)
            .expect("rotate_shape should succeed");
        let translated = ffi::ffi::translate_shape(&rotated, 10.0, 0.0, 0.0)
            .expect("translate_shape should succeed");
        // Revolve around Z axis by full rotation
        let revolved = ffi::ffi::make_revolve(
            &translated,
            0.0,
            0.0,
            0.0, // axis origin
            0.0,
            0.0,
            1.0, // axis direction (Z)
            std::f64::consts::TAU,
        )
        .expect("make_revolve should succeed");

        // Volume must be positive (a Shell would give 0 or near-0)
        let vol = ffi::ffi::query_volume(&revolved).expect("query_volume should succeed");
        assert!(
            vol > 1.0,
            "revolve result should be a Solid with positive volume, got {}",
            vol
        );

        // Verify geometric correctness via Pappus' theorem:
        // V = 2π × R × A = 2π × 10 × (4×2) = 160π ≈ 502.65
        let expected = 2.0 * std::f64::consts::PI * 10.0 * (4.0 * 2.0);
        let rel_err = (vol - expected).abs() / expected;
        assert!(
            rel_err < 0.02,
            "expected volume ≈ {:.2} (160π), got {:.2} (rel_err={:.4})",
            expected,
            vol,
            rel_err
        );
    }

    // --- Revolve error specificity tests (task-400) ---

    #[test]
    fn revolve_nan_axis_origin_error_mentions_origin() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [f64::NAN, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::TAU,
        });
        assert_operation_fails_with(result, "axis_origin");
    }

    #[test]
    fn revolve_nan_axis_dir_error_mentions_dir() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, f64::NAN, 0.0],
            angle_rad: std::f64::consts::TAU,
        });
        assert_operation_fails_with(result, "axis_dir");
    }

    #[test]
    fn revolve_inf_angle_error_mentions_angle() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: f64::INFINITY,
        });
        assert_operation_fails_with(result, "angle");
    }

    #[test]
    fn revolve_zero_angle_error_includes_value() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: 0.0,
        });
        // Error should include the actual value "0" for debuggability
        assert_operation_fails_with(result, "0");
    }

    #[test]
    fn revolve_zero_axis_error_includes_values() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 0.0],
            angle_rad: std::f64::consts::TAU,
        });
        // Error should include the axis values
        assert_operation_fails_with(result, "0, 0, 0");
    }

    #[test]
    fn revolve_near_zero_angle_rejected() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        // 1e-35 is below C++ threshold (1e-30) and below any reasonable physical angle.
        // Rust should catch this at its layer with a clear error.
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: 1e-35,
        });
        assert_operation_fails_with(result, "zero");
    }

    #[test]
    fn revolve_near_degenerate_axis_rejected() {
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        // axis=[1e-8, 0, 0] has mag_sq=1e-16, physically meaningless
        // but above old EPSILON^2 ≈ 4.9e-32; threshold 1e-12 should catch it
        let result = kernel.execute(&GeometryOp::Revolve {
            profile: box_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [1e-8, 0.0, 0.0],
            angle_rad: std::f64::consts::TAU,
        });
        assert_operation_fails_with(result, "zero");
    }

    // --- Revolve positive boundary tests (task-574) ---

    #[test]
    fn revolve_just_above_axis_threshold_accepted() {
        // axis_dir=[0, 0, 1e-3] has mag_sq=1e-6, well above AXIS_MAG_SQ_MIN=1e-12.
        // Uses Z-direction so the torus profile (offset 20 along X) doesn't
        // intersect the revolve axis.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let face_id = make_torus_profile(&mut kernel, 5.0, 20.0);

        let result = kernel.execute(&GeometryOp::Revolve {
            profile: face_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1e-3],
            angle_rad: std::f64::consts::TAU,
        });
        assert!(
            result.is_ok(),
            "axis mag_sq=1e-6 (above AXIS_MAG_SQ_MIN=1e-12) should be accepted, got {:?}",
            result.err()
        );
    }

    #[test]
    fn revolve_just_above_angle_threshold_accepted() {
        // angle_rad=1e-6 is far above ANGLE_ABS_MIN=1e-30 and also above
        // OCCT's internal Precision::Angular()≈1e-12, so both the Rust
        // validation and the OCCT operation succeed.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let face_id = make_torus_profile(&mut kernel, 5.0, 20.0);

        let result = kernel.execute(&GeometryOp::Revolve {
            profile: face_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: 1e-6,
        });
        assert!(
            result.is_ok(),
            "angle 1e-6 (above ANGLE_ABS_MIN=1e-30) should be accepted, got {:?}",
            result.err()
        );
    }

    #[test]
    fn sweep_circle_along_line_creates_pipe() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        // Circle face profile: r=2.0 at z=0
        let face = ffi::ffi::make_circle_face(2.0, 0.0).expect("circle face");
        let profile_id = kernel.store_raw(face);
        // Line wire path: (0,0,0) to (0,0,10)
        let wire = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 0.0, 0.0, 10.0).expect("line wire");
        let path_id = kernel.store_raw(wire);

        let pipe_h = kernel
            .execute(&GeometryOp::Sweep {
                profile: profile_id,
                path: path_id,
            })
            .unwrap();

        // Volume should be approximately pi*r^2*h = pi*4*10 ≈ 125.66
        let vol = kernel.query(&GeometryQuery::Volume(pipe_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                let expected = std::f64::consts::PI * 4.0 * 10.0;
                let rel_err = (v - expected).abs() / expected;
                assert!(
                    rel_err < 0.05,
                    "pipe volume should be ≈ {expected:.2}, got {v:.2} (rel_err={rel_err:.4})"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    // --- Tube tests (task-324) ---

    #[test]
    fn kernel_tube_volume_matches_pi_r2_minus_r2_h() {
        // Volume formula: π*(R² - r²)*h. With R=0.010, r=0.005, h=0.020 →
        // π*(1e-4 - 2.5e-5)*0.020 ≈ 4.712e-6 m³.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Tube {
                outer_r: Value::Real(0.010),
                inner_r: Value::Real(0.005),
                height: Value::Real(0.020),
            })
            .expect("Tube execute should succeed");

        let vol = kernel
            .query(&GeometryQuery::Volume(handle.id))
            .expect("Volume query should succeed");
        let v = vol.as_f64().expect("Volume should be numeric");
        let expected =
            std::f64::consts::PI * (0.010_f64.powi(2) - 0.005_f64.powi(2)) * 0.020;
        let rel_err = (v - expected).abs() / expected;
        assert!(
            rel_err < 0.01,
            "tube volume should be ≈ {expected:.3e} m³, got {v:.3e} (rel_err={rel_err:.4})"
        );
    }

    #[test]
    fn kernel_tube_inner_ge_outer_returns_error() {
        // inner_r >= outer_r must be rejected with a message mentioning "inner".
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Tube {
            outer_r: Value::Real(0.005),
            inner_r: Value::Real(0.010),
            height: Value::Real(0.020),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("inner"),
                    "expected error mentioning 'inner', got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for inner_r > outer_r"),
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    #[test]
    fn kernel_tube_non_positive_dimensions_return_error() {
        // Each of outer_r=0, inner_r=0, height=0, outer_r=-1 must be rejected.
        let cases: &[(f64, f64, f64, &str)] = &[
            (0.0, 0.001, 0.010, "outer_r=0"),
            (0.005, 0.0, 0.010, "inner_r=0"),
            (0.005, 0.001, 0.0, "height=0"),
            (-1.0, 0.001, 0.010, "outer_r=-1"),
        ];
        for (outer, inner, height, label) in cases {
            let mut kernel = OcctKernel::new();
            let result = kernel.execute(&GeometryOp::Tube {
                outer_r: Value::Real(*outer),
                inner_r: Value::Real(*inner),
                height: Value::Real(*height),
            });
            match result {
                Err(GeometryError::OperationFailed(_)) => {}
                Ok(_) => panic!("expected error for {label}, got Ok"),
                Err(other) => panic!("expected OperationFailed for {label}, got {:?}", other),
            }
        }
    }

    // --- Pipe tests (task-324) ---

    #[test]
    fn kernel_pipe_straight_path_volume_matches_pi_r2_l() {
        // Path: straight line from (0,0,0) to (0,0,0.020), radius=0.002 →
        // Volume ≈ π*r²*L = π*(0.002)²*0.020 ≈ 2.513e-7 m³.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let wire_handle = kernel
            .execute(&GeometryOp::LineSegment {
                x1: 0.0, y1: 0.0, z1: 0.0,
                x2: 0.0, y2: 0.0, z2: 0.020,
            })
            .expect("LineSegment execute should succeed");
        let pipe_handle = kernel
            .execute(&GeometryOp::Pipe {
                path: wire_handle.id,
                radius: Value::Real(0.002),
            })
            .expect("Pipe execute should succeed");

        let vol = kernel
            .query(&GeometryQuery::Volume(pipe_handle.id))
            .expect("Volume query should succeed");
        let v = vol.as_f64().expect("Volume should be numeric");
        let expected = std::f64::consts::PI * 0.002_f64.powi(2) * 0.020;
        let rel_err = (v - expected).abs() / expected;
        // Direct BRep volume queries are analytic (not tessellation-based),
        // so a straight circular pipe should match the formula to within
        // floating-point noise. The tight tolerance protects against silent
        // unit-conversion regressions (a 1-3% error would previously pass a
        // lax 5% bound).
        assert!(
            rel_err < 1e-6,
            "pipe volume should be ≈ {expected:.3e} m³, got {v:.3e} (rel_err={rel_err:.4e})"
        );
    }

    #[test]
    fn kernel_pipe_non_positive_radius_returns_error() {
        // radius=0 and radius=-1 must both be rejected with "pipe radius"
        // in the error message.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        for r in [0.0, -1.0] {
            let mut kernel = OcctKernel::new();
            let wire_handle = kernel
                .execute(&GeometryOp::LineSegment {
                    x1: 0.0, y1: 0.0, z1: 0.0,
                    x2: 0.0, y2: 0.0, z2: 0.020,
                })
                .expect("LineSegment execute should succeed");
            let result = kernel.execute(&GeometryOp::Pipe {
                path: wire_handle.id,
                radius: Value::Real(r),
            });
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("pipe radius"),
                        "expected error mentioning 'pipe radius' for r={r}, got: {msg}"
                    );
                }
                Ok(_) => panic!("expected error for radius={r}, got Ok"),
                Err(other) => panic!("expected OperationFailed for radius={r}, got {:?}", other),
            }
        }
    }

    #[test]
    fn kernel_pipe_non_z_start_tangent_returns_error() {
        // This test locks in the explicit-error contract for non-+Z paths
        // defined in the orientation-constraint section of GeometryOp::Pipe.
        // Prior to task-2095, these cases silently returned a degenerate
        // (zero-volume) solid; they now return
        // GeometryError::OperationFailed with "start-tangent" in the message.
        //
        // The four cases cover:
        //   - +X line segment (start-tangent = +X)
        //   - +Y line segment (start-tangent = +Y)
        //   - Arc in the XY plane, start_angle=0 (start-tangent = +Y)
        //   - -Z line segment (start-tangent = -Z)
        //
        // The -Z case guards against future refactors that might accidentally
        // compare t.z.abs() instead of t.z — such a change would still reject
        // +X and +Y but would incorrectly accept -Z.
        //
        // See `kernel_pipe_straight_path_volume_matches_pi_r2_l` for the
        // accepted +Z case.
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }

        let cases: &[(&str, GeometryOp)] = &[
            (
                "+X line segment",
                GeometryOp::LineSegment {
                    x1: 0.0, y1: 0.0, z1: 0.0,
                    x2: 0.020, y2: 0.0, z2: 0.0,
                },
            ),
            (
                "+Y line segment",
                GeometryOp::LineSegment {
                    x1: 0.0, y1: 0.0, z1: 0.0,
                    x2: 0.0, y2: 0.020, z2: 0.0,
                },
            ),
            (
                "arc in XY plane",
                GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.010,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                    axis: [0.0, 0.0, 1.0],
                },
            ),
            (
                "-Z line segment",
                GeometryOp::LineSegment {
                    x1: 0.0, y1: 0.0, z1: 0.0,
                    x2: 0.0, y2: 0.0, z2: -0.020,
                },
            ),
        ];

        for (label, path_op) in cases {
            let mut kernel = OcctKernel::new();
            let wire_handle = kernel
                .execute(path_op)
                .unwrap_or_else(|e| panic!("{label}: path execute should succeed, got {e:?}"));
            let result = kernel.execute(&GeometryOp::Pipe {
                path: wire_handle.id,
                radius: Value::Real(0.002),
            });
            assert_operation_fails_with(result, "start-tangent");
        }
    }

    // --- validate_pipe_start_tangent helper unit tests ---

    #[test]
    fn validate_pipe_start_tangent_rejects_non_finite_components() {
        // Calls the pure helper directly with NaN / Infinity inputs and
        // asserts each non-finite component produces a "non-finite" error.
        let non_finite_cases = [
            ffi::ffi::Point3 { x: f64::NAN,       y: 0.0,            z: 1.0 },
            ffi::ffi::Point3 { x: 0.0,            y: f64::INFINITY,  z: 1.0 },
            ffi::ffi::Point3 { x: 0.0,            y: 0.0,            z: f64::NEG_INFINITY },
        ];
        for t in non_finite_cases {
            let coords = (t.x, t.y, t.z);
            let result = super::validate_pipe_start_tangent(t);
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("non-finite"),
                        "expected error containing 'non-finite' for {coords:?}, got: {msg}"
                    );
                }
                Ok(()) => panic!(
                    "expected Err for non-finite tangent ({coords:?}), got Ok"
                ),
                Err(other) => panic!(
                    "expected OperationFailed for non-finite tangent ({coords:?}), got {:?}",
                    other
                ),
            }
        }
    }

    #[test]
    fn validate_pipe_start_tangent_rejects_negative_z() {
        // Exercises the pure helper with a -Z unit tangent directly.
        // Guards against a future refactor that compares t.z.abs() instead of
        // t.z — such a change would still reject +X and +Y but would
        // incorrectly accept -Z. Asserts both the "start-tangent" substring
        // (correct branch) and negative-z evidence in the reported coordinates
        // so that a wrong-branch rejection would surface immediately.
        let t = ffi::ffi::Point3 { x: 0.0, y: 0.0, z: -1.0 };
        match super::validate_pipe_start_tangent(t) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("start-tangent"),
                    "expected error containing 'start-tangent' for -Z tangent, got: {msg}"
                );
                assert!(
                    msg.contains("-1"),
                    "expected error to include negative-Z coordinate evidence ('-1'), got: {msg}"
                );
            }
            Ok(()) => panic!("expected Err for -Z tangent (z=-1.0), got Ok"),
            Err(other) => panic!("expected OperationFailed for -Z tangent, got {:?}", other),
        }
    }

    #[test]
    fn validate_pipe_start_tangent_rejects_nan_magnitude() {
        // Exercises the non-finite guard with NaN inputs: any NaN component is
        // caught by the is_finite() check and should produce a "non-finite" error.
        // Covers the NaN-in-y case ({0, NaN, 0}) which is absent from
        // validate_pipe_start_tangent_rejects_non_finite_components.
        let nan_cases = [
            ffi::ffi::Point3 { x: f64::NAN, y: 0.0,      z: 1.0 },
            ffi::ffi::Point3 { x: 0.0,      y: f64::NAN, z: 0.0 },
        ];
        for t in nan_cases {
            let coords = (t.x, t.y, t.z);
            let result = super::validate_pipe_start_tangent(t);
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("non-finite"),
                        "expected error containing 'non-finite' for NaN tangent {coords:?}, got: {msg}"
                    );
                }
                Ok(()) => panic!(
                    "expected Err for NaN tangent ({coords:?}), got Ok"
                ),
                Err(other) => panic!(
                    "expected OperationFailed for NaN tangent ({coords:?}), got {:?}",
                    other
                ),
            }
        }
    }

    #[test]
    fn validate_pipe_start_tangent_rejects_oversize_z() {
        // Guards the upper-bound: a finite t.z far above 1.0 (e.g. 1e100) is not
        // a unit vector. The two-sided comparator rejects t.z outside
        // [1 - PIPE_START_TANGENT_Z_EPSILON, 1 + PIPE_START_TANGENT_Z_EPSILON].
        let t = ffi::ffi::Point3 { x: 0.0, y: 0.0, z: 1e100 };
        match super::validate_pipe_start_tangent(t) {
            Err(GeometryError::OperationFailed(_)) => {}
            Ok(()) => panic!("expected Err for oversize-z tangent (z=1e100), got Ok"),
            Err(other) => panic!(
                "expected OperationFailed for oversize-z tangent, got {:?}",
                other
            ),
        }
    }

    // --- wire_start_tangent FFI tests ---

    #[test]
    fn ffi_wire_start_tangent_returns_unit_z_for_z_line() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let wire = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 0.0, 0.0, 0.020)
            .expect("make_line_wire should succeed");
        let t = ffi::ffi::wire_start_tangent(&wire)
            .expect("wire_start_tangent should succeed for Z line");
        assert!(
            t.x.abs() < 1e-6,
            "start-tangent x should be ≈ 0 for Z line, got {}",
            t.x
        );
        assert!(
            t.y.abs() < 1e-6,
            "start-tangent y should be ≈ 0 for Z line, got {}",
            t.y
        );
        assert!(
            (t.z - 1.0).abs() < 1e-6,
            "start-tangent z should be ≈ 1 for Z line, got {}",
            t.z
        );
    }

    #[test]
    fn ffi_wire_start_tangent_returns_unit_x_for_x_line() {
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let wire = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 0.020, 0.0, 0.0)
            .expect("make_line_wire should succeed");
        let t = ffi::ffi::wire_start_tangent(&wire)
            .expect("wire_start_tangent should succeed for X line");
        assert!(
            (t.x - 1.0).abs() < 1e-6,
            "start-tangent x should be ≈ 1 for X line, got {}",
            t.x
        );
        assert!(
            t.y.abs() < 1e-6,
            "start-tangent y should be ≈ 0 for X line, got {}",
            t.y
        );
        assert!(
            t.z.abs() < 1e-6,
            "start-tangent z should be ≈ 0 for X line, got {}",
            t.z
        );
    }

    /// These two tests specifically exercise `BRepAdaptor_CompCurve` on
    /// **multi-edge** wires (two line segments joined end-to-end via
    /// `make_polyline_wire`). If a future refactor replaced the composite-curve
    /// adaptor with a single-edge `BRepAdaptor_Curve`, it would break these
    /// tests while the single-edge tests above would still pass — making the
    /// regression immediately detectable.
    #[test]
    fn ffi_wire_start_tangent_composite_wire_non_z_direction() {
        // Two +X segments: (0,0,0)→(0.010,0,0)→(0.020,0,0).
        // start-tangent should be ≈ +X = (1, 0, 0).
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        #[rustfmt::skip]
        let coords: &[f64] = &[
            0.0, 0.0, 0.0,
            0.010, 0.0, 0.0,
            0.020, 0.0, 0.0,
        ];
        let wire = ffi::ffi::make_polyline_wire(coords, 3)
            .expect("make_polyline_wire should succeed for 3-point +X polyline");
        let t = ffi::ffi::wire_start_tangent(&wire)
            .expect("wire_start_tangent should succeed for composite +X wire");
        assert!(
            (t.x - 1.0).abs() < 1e-6,
            "composite +X wire: start-tangent x should be ≈ 1.0, got {}",
            t.x
        );
        assert!(
            t.y.abs() < 1e-6,
            "composite +X wire: start-tangent y should be ≈ 0, got {}",
            t.y
        );
        assert!(
            t.z.abs() < 1e-6,
            "composite +X wire: start-tangent z should be ≈ 0, got {}",
            t.z
        );
    }

    #[test]
    fn ffi_wire_start_tangent_composite_wire_z_direction() {
        // Two +Z segments: (0,0,0)→(0,0,0.010)→(0,0,0.020).
        // start-tangent should be ≈ +Z = (0, 0, 1).
        if !crate::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        #[rustfmt::skip]
        let coords: &[f64] = &[
            0.0, 0.0, 0.0,
            0.0, 0.0, 0.010,
            0.0, 0.0, 0.020,
        ];
        let wire = ffi::ffi::make_polyline_wire(coords, 3)
            .expect("make_polyline_wire should succeed for 3-point +Z polyline");
        let t = ffi::ffi::wire_start_tangent(&wire)
            .expect("wire_start_tangent should succeed for composite +Z wire");
        assert!(
            t.x.abs() < 1e-6,
            "composite +Z wire: start-tangent x should be ≈ 0, got {}",
            t.x
        );
        assert!(
            t.y.abs() < 1e-6,
            "composite +Z wire: start-tangent y should be ≈ 0, got {}",
            t.y
        );
        assert!(
            (t.z - 1.0).abs() < 1e-6,
            "composite +Z wire: start-tangent z should be ≈ 1.0, got {}",
            t.z
        );
    }

    // --- make_line_wire degeneracy threshold tests (task-383 S2) ---

    #[test]
    fn ffi_make_line_wire_rejects_sub_epsilon_length() {
        // length = 5e-6 m → dist_sq = 2.5e-11, which is above the old
        // CPP_AXIS_MAG_SQ_MIN (1e-30) but below the new CPP_LINE_WIRE_MIN_LENGTH_SQ
        // (1e-10). Confirms that make_line_wire now rejects degenerate lengths
        // < 1e-5 m (= √(1e-10) m ≈ 10 µm).
        let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 5e-6, 0.0, 0.0);
        // Use .err().expect(...) instead of unwrap_err() because UniquePtr<OcctShape>
        // doesn't implement Debug, so unwrap_err()'s panic message can't format the Ok arm.
        let err = result.err().expect("make_line_wire with 5µm segment should return Err, got Ok");
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("distinct"),
            "error message should mention 'distinct', got: {msg}"
        );
    }

    #[test]
    fn cpp_line_wire_floor_matches_rust_const() {
        // Behavioral anchor: verifies the C++ layer honours the Rust-defined
        // CPP_LINE_WIRE_MIN_LENGTH_SQ constant by bracketing the floor tightly.
        // Inputs are derived from the Rust const so this test auto-tracks any
        // future change to the canonical value in src/floor_constants.rs.
        // Multipliers 0.99× / 1.01× give squared distances 0.9801× / 1.0201× of
        // the floor — tight enough that drift >~2% in the C++ floor will fail.
        let floor_len = crate::CPP_LINE_WIRE_MIN_LENGTH_SQ.sqrt();
        // Just below the C++ floor — must be rejected.
        let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, floor_len * 0.99, 0.0, 0.0);
        result
            .err()
            .expect("make_line_wire just below CPP floor should return Err, got Ok");
        // Just above the C++ floor — must succeed.
        let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, floor_len * 1.01, 0.0, 0.0);
        result.expect("make_line_wire just above CPP floor should succeed");
    }

    #[test]
    fn rust_guard_above_floor_does_not_fire() {
        // Exercises the above-floor case via `OcctKernel::execute`:
        //   dist_sq = 1.1 × RUST_LINE_WIRE_MIN_LENGTH_SQ — the `[rust-guard]` marker must NOT
        //   appear in any error. `Ok` is not required: CPP_LINE_WIRE_MIN_LENGTH_SQ is 100×
        //   above the Rust floor, so the C++ layer still rejects this input; an
        //   `OperationFailed` without the marker is acceptable.
        // See `floor_constants::tests::below_floor_rejects_with_rust_guard_marker` and
        //   `tests/curve_constructors_integration.rs::line_segment_coincident_points_returns_error`
        //   for the below-floor complements.

        // Above-floor case: dist_sq = 1.1 × RUST_LINE_WIRE_MIN_LENGTH_SQ.
        // The Rust guard must NOT fire; any error here is from the C++ layer.
        let above_dx = (1.1 * crate::RUST_LINE_WIRE_MIN_LENGTH_SQ).sqrt();
        debug_assert!(
            above_dx * above_dx > crate::RUST_LINE_WIRE_MIN_LENGTH_SQ,
            "above_dx² must be strictly > RUST_LINE_WIRE_MIN_LENGTH_SQ after fp round-trip"
        );
        let mut kernel = OcctKernel::new();
        let above_result = kernel.execute(&GeometryOp::LineSegment {
            x1: 0.0, y1: 0.0, z1: 0.0,
            x2: above_dx, y2: 0.0, z2: 0.0,
        });
        match above_result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    !msg.contains(RUST_GUARD_MARKER),
                    "above-floor case must not fire the Rust guard; marker '[rust-guard]' must \
                     be absent in error (C++ rejection expected here), got: {msg:?}. This \
                     indicates the Rust guard was widened to fire above RUST_LINE_WIRE_MIN_LENGTH_SQ."
                );
            }
            Ok(_) => { /* stronger than needed but acceptable */ }
            Err(other) => panic!(
                "above-floor case should be Ok or OperationFailed, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn center_of_mass_with_density_returns_translated_centroid() {
        let mut kernel = OcctKernel::new();
        // Create a 10×10×10 box and translate it by dx=5 so centroid is at (5, 0, 0).
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        let translated = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 5.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();


        // Query with density 100.0.
        let result_100 = kernel
            .query(&GeometryQuery::CenterOfMass {
                handle: translated.id,
                density: 100.0,
            })
            .unwrap();
        let (x, y, z) = match &result_100 {
            Value::String(s) => parse_centroid_json(s),
            other => panic!("expected Value::String JSON centroid, got {:?}", other),
        };
        // After dx=5 translation on an origin-centred box, centroid must be at (5, 0, 0).
        let tol = 1e-3;
        assert!((x - 5.0).abs() < tol, "centroid x expected ≈5, got {x}");
        assert!(y.abs() < tol, "centroid y expected ≈0, got {y}");
        assert!(z.abs() < tol, "centroid z expected ≈0, got {z}");

        // Density must be irrelevant for uniform-density bodies.  Run the same
        // query with a completely different density and confirm results are identical
        // (bit-equal strings), locking in the documented invariant.
        let result_1 = kernel
            .query(&GeometryQuery::CenterOfMass {
                handle: translated.id,
                density: 1.0,
            })
            .unwrap();
        assert_eq!(
            result_100, result_1,
            "CenterOfMass result must be density-independent (got {result_100:?} vs {result_1:?})"
        );
    }

    /// Query CenterOfMass with a handle that was never inserted into the kernel.
    /// The kernel must return `Err(QueryError::InvalidHandle(id))` with the same
    /// `GeometryHandleId` that was queried — not `Ok(_)` or any other error variant.
    ///
    /// This pins the contract already implemented at lib.rs:957–962, where the
    /// `query` dispatch arm for CenterOfMass maps an unknown handle to
    /// `Err(QueryError::InvalidHandle(*handle))`.
    #[test]
    fn center_of_mass_invalid_handle_returns_invalid_handle_err() {
        let mut kernel = OcctKernel::new();
        let bad_id = GeometryHandleId(9999);
        let result = kernel.query(&GeometryQuery::CenterOfMass { handle: bad_id, density: 1.0 });
        match result {
            Err(QueryError::InvalidHandle(id)) => {
                assert_eq!(
                    id, bad_id,
                    "InvalidHandle must carry the same handle ID that was queried"
                );
            }
            Ok(v) => panic!(
                "expected Err(QueryError::InvalidHandle({:?})), got Ok({:?})",
                bad_id, v
            ),
            Err(other) => panic!(
                "expected Err(QueryError::InvalidHandle({:?})), got Err({:?})",
                bad_id, other
            ),
        }
    }

    /// Query InertiaTensor with a handle that was never inserted into the kernel.
    /// The kernel must return `Err(QueryError::InvalidHandle(id))` with the same
    /// `GeometryHandleId` that was queried — not `Ok(_)` or any other error variant.
    ///
    /// Kept separate from `center_of_mass_invalid_handle_returns_invalid_handle_err`
    /// so a regression that breaks only one query variant has unambiguous attribution.
    ///
    /// This pins the contract already implemented at lib.rs:968–973, where the
    /// `query` dispatch arm for InertiaTensor maps an unknown handle to
    /// `Err(QueryError::InvalidHandle(*handle))`.
    #[test]
    fn inertia_tensor_invalid_handle_returns_invalid_handle_err() {
        let mut kernel = OcctKernel::new();
        let bad_id = GeometryHandleId(9999);
        let result = kernel.query(&GeometryQuery::InertiaTensor { handle: bad_id, density: 1.0 });
        match result {
            Err(QueryError::InvalidHandle(id)) => {
                assert_eq!(
                    id, bad_id,
                    "InvalidHandle must carry the same handle ID that was queried"
                );
            }
            Ok(v) => panic!(
                "expected Err(QueryError::InvalidHandle({:?})), got Ok({:?})",
                bad_id, v
            ),
            Err(other) => panic!(
                "expected Err(QueryError::InvalidHandle({:?})), got Err({:?})",
                bad_id, other
            ),
        }
    }

    /// Query CenterOfMass on a non-solid wire (a line segment).
    ///
    /// OCCT's `BRepGProp::VolumeProperties` returns mass=0 for non-solid shapes
    /// (wires have no enclosed volume), and OCCT's `GProp_GProps::CentreOfMass()`
    /// defaults to the origin when mass=0.  The kernel therefore returns
    /// `Ok(Value::String("..."))` with (x, y, z) ≈ (0, 0, 0).
    ///
    /// **This pins current observable behavior.**  A future kernel change MAY tighten
    /// this to return a typed `Err(QueryError::…)` for non-solid input — update this
    /// test if that change lands.
    #[test]
    fn center_of_mass_on_non_solid_wire_returns_origin() {
        let mut kernel = OcctKernel::new();
        // A line segment of length 10 — well above any minimum-length floor.
        let wire_h = kernel
            .execute(&GeometryOp::LineSegment {
                x1: 0.0, y1: 0.0, z1: 0.0,
                x2: 10.0, y2: 0.0, z2: 0.0,
            })
            .expect("LineSegment must succeed");
        let result = kernel
            .query(&GeometryQuery::CenterOfMass { handle: wire_h.id, density: 1.0 });
        // (a) must not panic, must not return Err — the kernel is permissive for non-solids.
        let value = result.expect(
            "CenterOfMass on a non-solid wire must not return Err (kernel is permissive)"
        );
        // (b) result must be a JSON-encoded centroid string.
        let (x, y, z) = match &value {
            Value::String(s) => parse_centroid_json(s),
            other => panic!("expected Value::String JSON centroid, got {:?}", other),
        };
        // (c) OCCT returns mass=0 for non-solids → CentreOfMass defaults to origin.
        let tol = 1e-6;
        assert!(x.abs() < tol, "centroid x expected ≈0 for non-solid wire, got {x}");
        assert!(y.abs() < tol, "centroid y expected ≈0 for non-solid wire, got {y}");
        assert!(z.abs() < tol, "centroid z expected ≈0 for non-solid wire, got {z}");
    }

    /// Query InertiaTensor on a non-solid wire (a line segment).
    ///
    /// `BRepGProp::VolumeProperties` returns mass=0 for non-solid shapes, so
    /// `GProp_GProps::MatrixOfInertia()` is the zero matrix.  After multiplication
    /// by `density` all 9 entries remain zero.
    ///
    /// **This pins current observable behavior.**  A future kernel change MAY tighten
    /// this to return a typed `Err(QueryError::…)` for non-solid input — update this
    /// test if that change lands.
    #[test]
    fn inertia_tensor_on_non_solid_wire_returns_zero_tensor() {
        let mut kernel = OcctKernel::new();
        let wire_h = kernel
            .execute(&GeometryOp::LineSegment {
                x1: 0.0, y1: 0.0, z1: 0.0,
                x2: 10.0, y2: 0.0, z2: 0.0,
            })
            .expect("LineSegment must succeed");
        let result = kernel
            .query(&GeometryQuery::InertiaTensor { handle: wire_h.id, density: 1.0 });
        let value = result.expect(
            "InertiaTensor on a non-solid wire must not return Err (kernel is permissive)"
        );
        let entries = extract_3x3_tensor_entries(&value);
        let tol = 1e-9;
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    entries[i][j].abs() < tol,
                    "entry [{i}][{j}] expected ≈0 for non-solid wire (mass=0 → zero tensor), \
                     got {}",
                    entries[i][j]
                );
            }
        }
    }

    /// Verify that `CenterOfMass` is density-independent for a uniform-density solid.
    ///
    /// The kernel's `CenterOfMass` query ignores the `density` field (bound to `_`
    /// in the dispatch arm at lib.rs:957) because the centre of mass of a uniform-density
    /// body equals its geometric centroid regardless of density.  This test locks in that
    /// invariant for three edge values: `density=1.0` (baseline), `density=0.0` (zero),
    /// and `density=-2.0` (negative) — the two cases the task description called out.
    ///
    /// **This pins current observable behavior.**  A future kernel change MAY reject
    /// ρ ≤ 0 with a typed error — update this test if so.
    #[test]
    fn center_of_mass_density_zero_or_negative_unchanged() {
        let mut kernel = OcctKernel::new();
        // A 10×10×10 box centred at the origin.
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .expect("Box creation must succeed");
        // Query with four density values including the two edge cases ρ=0 and ρ<0.
        let densities = [1.0_f64, 0.0, -2.0, 100.0];
        let results: Vec<Value> = densities
            .iter()
            .map(|&d| {
                kernel
                    .query(&GeometryQuery::CenterOfMass { handle: box_h.id, density: d })
                    .unwrap_or_else(|e| {
                        panic!("CenterOfMass with density={d} must not return Err: {e:?}")
                    })
            })
            .collect();
        // All four results must be identical (bit-equal strings) — CenterOfMass is density-
        // independent for uniform-density bodies.
        for (i, r) in results.iter().enumerate().skip(1) {
            assert_eq!(
                &results[0], r,
                "CenterOfMass with density={} must equal result with density={} \
                 (density-independent for uniform bodies); got {:?} vs {:?}",
                densities[i], densities[0], r, &results[0]
            );
        }
    }

    /// Document the density edge-case behavior of `InertiaTensor`.
    ///
    /// The C++ `query_inertia_tensor` implementation multiplies every entry of OCCT's
    /// volume-weighted matrix by `density` (a pure scalar multiply in occt_wrapper.cpp).
    /// This yields two documented edge behaviors:
    ///
    /// * **ρ = 0.0** → all 9 tensor entries are exactly 0.0 (zero density → zero mass →
    ///   zero inertia tensor).
    /// * **ρ < 0** → each entry equals `−1 × (entry at the absolute value of ρ)` (scalar
    ///   negation, no guard at the kernel boundary).
    ///
    /// **This pins current observable behavior.**  A future kernel change MAY reject ρ ≤ 0
    /// with a typed error — update this test if so.
    #[test]
    fn inertia_tensor_density_edge_cases_documented() {
        let mut kernel = OcctKernel::new();
        // A 20×10×5 box (same fixture as `inertia_tensor_box_with_density_analytic`).
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(20.0),
                height: Value::Real(10.0),
                depth: Value::Real(5.0),
            })
            .expect("Box creation must succeed");

        // Baseline: ρ = 2.0.
        let result_baseline = kernel
            .query(&GeometryQuery::InertiaTensor { handle: box_h.id, density: 2.0 })
            .expect("InertiaTensor with density=2.0 must not return Err");
        let baseline = extract_3x3_tensor_entries(&result_baseline);

        // Edge case 1: ρ = 0.0 → zero tensor.
        let result_zero = kernel
            .query(&GeometryQuery::InertiaTensor { handle: box_h.id, density: 0.0 })
            .expect("InertiaTensor with density=0.0 must not return Err (kernel is permissive)");
        let zero_entries = extract_3x3_tensor_entries(&result_zero);
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    zero_entries[i][j].abs() < 1e-9,
                    "entry [{i}][{j}] must be ≈0 for density=0 (zero density → zero tensor), \
                     got {}",
                    zero_entries[i][j]
                );
            }
        }

        // Edge case 2: ρ = -2.0 → negated baseline tensor.
        let result_neg = kernel
            .query(&GeometryQuery::InertiaTensor { handle: box_h.id, density: -2.0 })
            .expect("InertiaTensor with density=-2.0 must not return Err (kernel is permissive)");
        let neg_entries = extract_3x3_tensor_entries(&result_neg);
        // Each entry must equal the negation of the baseline within a small absolute tolerance.
        // Baseline entries are O(1e4), so tol=100 is ~1% relative — comfortable margin.
        let tol = 100.0_f64;
        for i in 0..3 {
            for j in 0..3 {
                let expected = -baseline[i][j];
                assert!(
                    (neg_entries[i][j] - expected).abs() < tol,
                    "entry [{i}][{j}] with density=-2.0 expected ≈{expected:.4} \
                     (negation of baseline {:.4}), got {}, diff {}",
                    baseline[i][j], neg_entries[i][j],
                    (neg_entries[i][j] - expected).abs()
                );
            }
        }
    }

    #[test]
    fn inertia_tensor_box_with_density_analytic() {
        let mut kernel = OcctKernel::new();
        // Create a 20×10×5 axis-aligned box. Volume = 1000; mass = ρ·V = 2·1000 = 2000.
        // Analytic moments about centroid (box m/12·(h²+d²)):
        //   I_xx = 2000/12 · (10² + 5²) = 2000/12 · 125 ≈ 20833.33
        //   I_yy = 2000/12 · (20² + 5²) = 2000/12 · 425 ≈ 70833.33
        //   I_zz = 2000/12 · (20² + 10²) = 2000/12 · 500 ≈ 83333.33
        // Off-diagonals = 0 for axis-aligned box at origin.
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(20.0),
                height: Value::Real(10.0),
                depth: Value::Real(5.0),
            })
            .unwrap();
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: box_h.id,
                density: 2.0,
            })
            .unwrap();
        // (a) result must be a 3-row list
        let rows = match result {
            Value::List(rows) => {
                assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
                rows
            }
            other => panic!("expected Value::List(rows), got {:?}", other),
        };
        // (b) extract the nine Real entries
        let mut entries = [[0.0f64; 3]; 3];
        for (i, row) in rows.iter().enumerate() {
            let cols = match row {
                Value::List(cols) => {
                    assert_eq!(cols.len(), 3, "row {} expected 3 cols, got {}", i, cols.len());
                    cols
                }
                other => panic!("row {} expected Value::List, got {:?}", i, other),
            };
            for (j, col) in cols.iter().enumerate() {
                entries[i][j] = match col {
                    Value::Real(v) => *v,
                    other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
                };
            }
        }
        let tol = 100.0;
        // (c) diagonal entries
        assert!(
            (entries[0][0] - 20833.33).abs() < tol,
            "I_xx expected ~20833.33, got {}", entries[0][0]
        );
        assert!(
            (entries[1][1] - 70833.33).abs() < tol,
            "I_yy expected ~70833.33, got {}", entries[1][1]
        );
        assert!(
            (entries[2][2] - 83333.33).abs() < tol,
            "I_zz expected ~83333.33, got {}", entries[2][2]
        );
        // (d) off-diagonal entries all near zero
        let off_diag_pairs = [
            (0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1),
        ];
        for (i, j) in off_diag_pairs {
            assert!(
                entries[i][j].abs() < tol,
                "off-diagonal [{i}][{j}] expected ~0, got {}", entries[i][j]
            );
        }

        // ── Second case: translate the box far from the origin (dx=100) ──────────
        // OCCT's BRepGProp::VolumeProperties computes centroidal inertia, so the
        // diagonal moments must be identical to the origin-centred case above.  This
        // distinguishes an implementation that accidentally computes inertia about
        // the *world origin* (which would differ) from one that correctly computes
        // inertia about the *centroid*.
        let translated_h = kernel
            .execute(&GeometryOp::Translate {
                target: box_h.id,
                dx: 100.0,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        let result_t = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: translated_h.id,
                density: 2.0,
            })
            .unwrap();
        let rows_t = match result_t {
            Value::List(rows) => {
                assert_eq!(rows.len(), 3);
                rows
            }
            other => panic!("translated box: expected Value::List(rows), got {:?}", other),
        };
        let mut entries_t = [[0.0f64; 3]; 3];
        for (i, row) in rows_t.iter().enumerate() {
            let cols = match row {
                Value::List(cols) => {
                    assert_eq!(cols.len(), 3);
                    cols
                }
                other => panic!("translated row {i}: expected Value::List, got {:?}", other),
            };
            for (j, col) in cols.iter().enumerate() {
                entries_t[i][j] = match col {
                    Value::Real(v) => *v,
                    other => panic!("translated [{i}][{j}] expected Value::Real, got {:?}", other),
                };
            }
        }
        // Centroidal inertia is translation-invariant: diagonal entries must match.
        assert!(
            (entries_t[0][0] - entries[0][0]).abs() < tol,
            "translated I_xx should equal origin I_xx ({:.2}), got {:.2}",
            entries[0][0], entries_t[0][0]
        );
        assert!(
            (entries_t[1][1] - entries[1][1]).abs() < tol,
            "translated I_yy should equal origin I_yy ({:.2}), got {:.2}",
            entries[1][1], entries_t[1][1]
        );
        assert!(
            (entries_t[2][2] - entries[2][2]).abs() < tol,
            "translated I_zz should equal origin I_zz ({:.2}), got {:.2}",
            entries[2][2], entries_t[2][2]
        );
    }

    #[test]
    fn inertia_tensor_large_shape_returns_symmetric_tensor() {
        let mut kernel = OcctKernel::new();
        // 1000×1000×1000 cube, density 1.0.
        // Diagonal inertia: m·L²/6 = (1e9)·1e6/6 ≈ 1.67e14 — well above 1e6.
        // A cube is isotropic: off-diagonal products of inertia are analytically zero at
        // centroid.  This test therefore verifies (a) the function does not throw for a
        // large shape and (b) the symmetry *contract* holds (bit-exact symmetric pairs),
        // which is guaranteed by the averaging implementation in query_inertia_tensor.
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(1000.0),
                height: Value::Real(1000.0),
                depth: Value::Real(1000.0),
            })
            .unwrap();
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: box_h.id,
                density: 1.0,
            })
            .expect("query_inertia_tensor must not spuriously fail on large shapes (tensor entries ≫ 1e6)");
        // Extract the nine Real entries from the returned 3×3 list-of-lists.
        let rows = match result {
            Value::List(rows) => {
                assert_eq!(rows.len(), 3, "expected 3 rows, got {}", rows.len());
                rows
            }
            other => panic!("expected Value::List(rows), got {:?}", other),
        };
        let mut entries = [[0.0f64; 3]; 3];
        for (i, row) in rows.iter().enumerate() {
            let cols = match row {
                Value::List(cols) => {
                    assert_eq!(cols.len(), 3, "row {} expected 3 cols, got {}", i, cols.len());
                    cols
                }
                other => panic!("row {} expected Value::List, got {:?}", i, other),
            };
            for (j, col) in cols.iter().enumerate() {
                entries[i][j] = match col {
                    Value::Real(v) => *v,
                    other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
                };
            }
        }
        // Diagonal entries must be positive, finite, and in the large-shape regime.
        for i in 0..3 {
            assert!(
                entries[i][i].is_finite() && entries[i][i] > 0.0,
                "diagonal [{i}][{i}] must be positive and finite, got {}",
                entries[i][i]
            );
            assert!(
                entries[i][i] > 1e6,
                "diagonal [{i}][{i}] must be > 1e6 (large-shape sanity), got {}",
                entries[i][i]
            );
        }
        // Off-diagonal symmetric pairs must be bit-exactly equal after the averaging fix.
        assert_eq!(
            entries[0][1], entries[1][0],
            "m12 vs m21 must be bit-equal after averaging fix"
        );
        assert_eq!(
            entries[0][2], entries[2][0],
            "m13 vs m31 must be bit-equal after averaging fix"
        );
        assert_eq!(
            entries[1][2], entries[2][1],
            "m23 vs m32 must be bit-equal after averaging fix"
        );
    }

    /// Test that off-diagonal products of inertia are bit-exactly symmetric for a shape
    /// that has *large, non-zero* off-diagonals — the regime the averaging fix targets.
    ///
    /// An axis-aligned cube has analytically zero off-diagonals at the centroid, so it is
    /// not a useful regression shape for this property.  A non-cubic box (1000×2000×500)
    /// rotated 30° about the Z-axis has I_xy ≈ 1.08e14 by the rotation-of-axes formula:
    ///   I_xy_world = (I_xx_local - I_yy_local) · sin(30°) · cos(30°)
    /// This is orders of magnitude above any noise threshold and conclusively exercises the
    /// regime where the old 1e-6 absolute threshold was unsound.  The bit-equality
    /// assertions pin the averaging behaviour: they would fail if query_inertia_tensor
    /// returned m(i,j) and m(j,i) from two independent m.Value(…) reads (pre-fix) and
    /// OCCT's two reads happened to disagree at the ULP level.
    #[test]
    fn inertia_tensor_rotated_non_cubic_box_offdiagonals_symmetric() {
        let mut kernel = OcctKernel::new();
        // Non-cubic box: width=1000, height=2000, depth=500, density=1.
        // Volume = 1e9, mass m = 1e9.
        // Centroidal moments in local frame (box axes):
        //   I_xx_local = m/12·(h²+d²) = 1e9/12·(4e6+2.5e5) ≈ 3.54e14
        //   I_yy_local = m/12·(w²+d²) = 1e9/12·(1e6+2.5e5) ≈ 1.04e14
        // After 30° rotation around Z:
        //   I_xy_world = (I_xx_local - I_yy_local)·sin30°·cos30° ≈ 1.08e14
        // (I_xz and I_yz remain zero because Z-rotation doesn't mix Z cross-products.)
        let box_h = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(1000.0),
                height: Value::Real(2000.0),
                depth: Value::Real(500.0),
            })
            .expect("Box creation must succeed");
        // Rotate 30° around Z-axis (PI/6 radians).
        let rotated_h = kernel
            .execute(&GeometryOp::Rotate {
                target: box_h.id,
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::PI / 6.0,
            })
            .expect("Rotate must succeed");
        let result = kernel
            .query(&GeometryQuery::InertiaTensor {
                handle: rotated_h.id,
                density: 1.0,
            })
            .expect("query_inertia_tensor must not fail on a rotated non-cubic box");
        // Extract entries.
        let rows = match result {
            Value::List(rows) => {
                assert_eq!(rows.len(), 3, "expected 3 rows");
                rows
            }
            other => panic!("expected Value::List(rows), got {:?}", other),
        };
        let mut entries = [[0.0f64; 3]; 3];
        for (i, row) in rows.iter().enumerate() {
            let cols = match row {
                Value::List(cols) => {
                    assert_eq!(cols.len(), 3);
                    cols
                }
                other => panic!("row {i} expected Value::List, got {:?}", other),
            };
            for (j, col) in cols.iter().enumerate() {
                entries[i][j] = match col {
                    Value::Real(v) => *v,
                    other => panic!("entry [{i}][{j}] expected Value::Real, got {:?}", other),
                };
            }
        }
        // Diagonal entries must be positive and finite.
        for i in 0..3 {
            assert!(
                entries[i][i].is_finite() && entries[i][i] > 0.0,
                "diagonal [{i}][{i}] must be positive and finite, got {}",
                entries[i][i]
            );
        }
        // I_xy_world must be non-trivially non-zero — this is the key property that
        // distinguishes this shape from a cube and places it in the regime the fix targets.
        // Expected |I_xy| ≈ 1.08e14; require > 1e12 as a conservative lower bound.
        assert!(
            entries[0][1].abs() > 1e12,
            "I_xy must be large (> 1e12) for rotated non-cubic box, got {}",
            entries[0][1]
        );
        // I_xz and I_yz must remain near zero (Z-rotation does not mix Z products).
        assert!(
            entries[0][2].abs() < 1e6,
            "I_xz must be near zero for Z-rotation, got {}",
            entries[0][2]
        );
        assert!(
            entries[1][2].abs() < 1e6,
            "I_yz must be near zero for Z-rotation, got {}",
            entries[1][2]
        );
        // Bit-exact symmetry: guaranteed by the averaging implementation.
        // These assertions would fail with the old code if OCCT's two independent reads
        // of m.Value(i,j) and m.Value(j,i) differed at the ULP level (possible when
        // entries are ~1e14 and FP noise is ~ULP(1e14) ≈ 10).
        assert_eq!(
            entries[0][1], entries[1][0],
            "m12 vs m21 must be bit-equal after averaging fix"
        );
        assert_eq!(
            entries[0][2], entries[2][0],
            "m13 vs m31 must be bit-equal after averaging fix"
        );
        assert_eq!(
            entries[1][2], entries[2][1],
            "m23 vs m32 must be bit-equal after averaging fix"
        );
    }

}

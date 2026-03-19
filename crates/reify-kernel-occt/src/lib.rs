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

#[allow(dead_code)]
mod ffi;
mod handle;
pub use handle::OcctKernelHandle;

use std::collections::HashMap;

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp,
    GeometryQuery, Mesh, OpaqueState, QueryError, ReprKind, TessError, Value, WarmStartable,
};

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

/// Extract an f64 from a Value (Int, Real, or Scalar → SI value).
fn extract_f64(v: &Value) -> Result<f64, GeometryError> {
    v.as_f64()
        .ok_or_else(|| GeometryError::OperationFailed("expected numeric value".into()))
}

/// OpenCASCADE geometry kernel (raw, `!Send + !Sync`).
///
/// Contains `cxx::UniquePtr<OcctShape>` handles which are `!Send`, so the
/// kernel cannot cross thread boundaries. For cross-thread usage, use
/// [`OcctKernelHandle`] which runs the kernel on a dedicated OS thread.
pub struct OcctKernel {
    shapes: HashMap<u64, cxx::UniquePtr<ffi::ffi::OcctShape>>,
    next_id: u64,
}

// Note: OcctKernel is !Send + !Sync because cxx::UniquePtr<OcctShape> is !Send.
// Use OcctKernelHandle for cross-thread usage — it communicates with a dedicated
// OS thread that owns the kernel.

impl OcctKernel {
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store a shape and return the next handle.
    fn store(&mut self, shape: cxx::UniquePtr<ffi::ffi::OcctShape>) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, shape);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: ReprKind::Solid,
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
}

impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

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
                // Chamfer not yet implemented via OCCT wrapper
                let _ = self.get_shape(*target)?;
                let _ = extract_f64(distance)?;
                return Err(GeometryError::OperationFailed(
                    "Chamfer not yet implemented".into(),
                ));
            }
            GeometryOp::Translate {
                target,
                dx,
                dy,
                dz,
            } => {
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
                if mag_sq == 0.0 {
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
            GeometryOp::CircularPattern { .. } => {
                return Err(GeometryError::OperationFailed(
                    "CircularPattern not yet implemented".into(),
                ));
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
                if mag_sq == 0.0
                    || !plane_normal[0].is_finite()
                    || !plane_normal[1].is_finite()
                    || !plane_normal[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "mirror plane normal must be a finite non-zero vector".into(),
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
            GeometryOp::Loft { .. } => {
                return Err(GeometryError::OperationFailed(
                    "Loft not yet implemented".into(),
                ));
            }
            GeometryOp::Draft { .. } => {
                return Err(GeometryError::OperationFailed(
                    "Draft not yet implemented".into(),
                ));
            }
            GeometryOp::Thicken { .. } => {
                return Err(GeometryError::OperationFailed(
                    "Thicken not yet implemented".into(),
                ));
            }
            GeometryOp::Shell { .. } => {
                return Err(GeometryError::OperationFailed(
                    "Shell not yet implemented".into(),
                ));
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
            GeometryQuery::Distance { .. } => {
                Err(QueryError::QueryFailed("Distance not yet implemented".into()))
            }
            GeometryQuery::MomentOfInertia { .. } => {
                Err(QueryError::QueryFailed("MomentOfInertia not yet implemented".into()))
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

    pub fn tessellate(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<Mesh, TessError> {
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
        let mut staged = HashMap::new();
        for (id, brep) in warm.shapes {
            cxx::let_cxx_string!(brep_cxx = brep.as_str());
            match ffi::ffi::deserialize_brep(&brep_cxx) {
                Ok(shape) => {
                    staged.insert(id, shape);
                }
                Err(_) => {
                    // Skip entries that fail to deserialize (best-effort)
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

#[cfg(test)]
impl OcctKernel {
    /// Inject a null `UniquePtr<OcctShape>` into the shapes map for testing.
    /// This simulates a corrupted shape handle (present in map but wrapping a
    /// null C++ pointer).
    fn insert_null_shape(&mut self, id: u64) {
        self.shapes
            .insert(id, cxx::UniquePtr::null());
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_state_returns_none_on_fresh_kernel() {
        let kernel = OcctKernel::new();
        assert!(kernel.warm_state().is_none(), "fresh kernel should have no warm state");
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
                assert!(
                    (v - 6000.0).abs() < 1.0,
                    "expected volume ~6000, got {v}"
                );
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
        assert_eq!(sphere_h.id, GeometryHandleId(4), "next_id should be restored");
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
        let vol = kernel
            .query(&GeometryQuery::Volume(gh.id))
            .unwrap();
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
        let vol_after = kernel.query(&GeometryQuery::Volume(GeometryHandleId(1))).unwrap();
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
        assert!(!brep.is_empty(), "BRep serialization should produce non-empty output");

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
        let vol = kernel
            .query(&GeometryQuery::Volume(pattern_h.id))
            .unwrap();
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
        let vol = kernel
            .query(&GeometryQuery::Volume(fused_h.id))
            .unwrap();
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
}

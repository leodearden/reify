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
        self.shapes
            .get(&id.0)
            .map(|ptr| ptr.as_ref().unwrap())
            .ok_or(GeometryError::InvalidReference(id))
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
                ffi::ffi::make_box(w, h, d)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Cylinder { radius, height } => {
                let r = extract_f64(radius)?;
                let h = extract_f64(height)?;
                ffi::ffi::make_cylinder(r, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Sphere { radius } => {
                let r = extract_f64(radius)?;
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
                ffi::ffi::translate_shape(shape, *dx, *dy, *dz)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
            }
            GeometryOp::Rotate {
                target,
                axis,
                angle_rad,
            } => {
                let shape = self.get_shape(*target)?;
                ffi::ffi::rotate_shape(shape, axis[0], axis[1], axis[2], *angle_rad)
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
            match ffi::ffi::serialize_brep(shape.as_ref().unwrap()) {
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
}

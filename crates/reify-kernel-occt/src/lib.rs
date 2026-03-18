//! OpenCASCADE geometry kernel implementation for Reify.
//!
//! Implements the `GeometryKernel` trait from `reify-types` using OCCT via cxx FFI.

#[allow(dead_code)]
mod ffi;
mod handle;
pub use handle::OcctKernelHandle;

use std::collections::HashMap;

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp,
    GeometryQuery, Mesh, QueryError, ReprKind, TessError, Value,
};

/// Extract an f64 from a Value (Int, Real, or Scalar → SI value).
fn extract_f64(v: &Value) -> Result<f64, GeometryError> {
    v.as_f64()
        .ok_or_else(|| GeometryError::OperationFailed("expected numeric value".into()))
}

/// OpenCASCADE geometry kernel.
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

use std::fmt;

use crate::hash::ContentHash;
use crate::value::Value;

/// Unique identifier for a geometry handle within a kernel session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeometryHandleId(pub u64);

impl GeometryHandleId {
    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [11] followed by the id as le_bytes.
    /// This serves as a proxy hash since OCCT shapes can't be hashed directly.
    pub fn content_hash(&self) -> ContentHash {
        let mut buf = [0u8; 9];
        buf[0] = 11;
        buf[1..].copy_from_slice(&self.0.to_le_bytes());
        ContentHash::of(&buf)
    }
}

/// An opaque handle to a geometry object managed by a kernel.
#[derive(Debug, Clone)]
pub struct GeometryHandle {
    pub id: GeometryHandleId,
    pub repr: ReprKind,
}

/// What kind of geometric representation this handle holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReprKind {
    /// B-rep solid.
    Solid,
    /// Shell (open or closed).
    Shell,
    /// Wire.
    Wire,
    /// Compound of multiple shapes.
    Compound,
}

/// Operations that can be sent to a geometry kernel.
#[derive(Debug, Clone)]
pub enum GeometryOp {
    /// Create a box primitive centered at origin.
    Box {
        width: Value,
        height: Value,
        depth: Value,
    },
    /// Create a cylinder primitive along Z axis.
    Cylinder { radius: Value, height: Value },
    /// Create a sphere primitive.
    Sphere { radius: Value },
    /// Boolean union.
    Union {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Boolean difference (left - right).
    Difference {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Boolean intersection.
    Intersection {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Fillet (round) edges by radius.
    Fillet {
        target: GeometryHandleId,
        radius: Value,
    },
    /// Chamfer edges by distance.
    Chamfer {
        target: GeometryHandleId,
        distance: Value,
    },
    /// Translate by vector (dx, dy, dz in meters).
    Translate {
        target: GeometryHandleId,
        dx: f64,
        dy: f64,
        dz: f64,
    },
    /// Rotate around axis by angle.
    Rotate {
        target: GeometryHandleId,
        axis: [f64; 3],
        angle_rad: f64,
    },
}

/// Queries against geometry handles.
#[derive(Debug, Clone)]
pub enum GeometryQuery {
    /// Compute volume in m³.
    Volume(GeometryHandleId),
    /// Compute surface area in m².
    SurfaceArea(GeometryHandleId),
    /// Compute centroid position.
    Centroid(GeometryHandleId),
    /// Compute bounding box.
    BoundingBox(GeometryHandleId),
}

/// Export formats for geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    Step,
    Stl,
    Obj,
}

/// Tessellated mesh for visualization.
#[derive(Debug, Clone)]
pub struct Mesh {
    /// Vertex positions, flat [x0, y0, z0, x1, y1, z1, ...].
    pub vertices: Vec<f32>,
    /// Triangle indices, flat [i0, i1, i2, i3, i4, i5, ...].
    pub indices: Vec<u32>,
    /// Optional vertex normals, flat like vertices.
    pub normals: Option<Vec<f32>>,
}

/// Errors from geometry operations.
#[derive(Debug, Clone)]
pub enum GeometryError {
    /// Reference to a handle that doesn't exist.
    InvalidReference(GeometryHandleId),
    /// Operation failed (e.g., zero-dimension primitive).
    OperationFailed(String),
    /// Kernel initialization error.
    InitFailed(String),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeometryError::InvalidReference(id) => {
                write!(f, "invalid geometry handle: {:?}", id)
            }
            GeometryError::OperationFailed(msg) => {
                write!(f, "geometry operation failed: {}", msg)
            }
            GeometryError::InitFailed(msg) => {
                write!(f, "geometry kernel init failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for GeometryError {}

/// Errors from export operations.
#[derive(Debug, Clone)]
pub enum ExportError {
    InvalidHandle(GeometryHandleId),
    IoError(String),
    FormatError(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::InvalidHandle(id) => write!(f, "invalid handle for export: {:?}", id),
            ExportError::IoError(msg) => write!(f, "export I/O error: {}", msg),
            ExportError::FormatError(msg) => write!(f, "export format error: {}", msg),
        }
    }
}

impl std::error::Error for ExportError {}

/// Errors from tessellation.
#[derive(Debug, Clone)]
pub enum TessError {
    InvalidHandle(GeometryHandleId),
    TessellationFailed(String),
}

impl fmt::Display for TessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TessError::InvalidHandle(id) => write!(f, "invalid handle for tessellation: {:?}", id),
            TessError::TessellationFailed(msg) => write!(f, "tessellation failed: {}", msg),
        }
    }
}

impl std::error::Error for TessError {}

/// Errors from geometry queries.
#[derive(Debug, Clone)]
pub enum QueryError {
    InvalidHandle(GeometryHandleId),
    QueryFailed(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::InvalidHandle(id) => write!(f, "invalid handle for query: {:?}", id),
            QueryError::QueryFailed(msg) => write!(f, "geometry query failed: {}", msg),
        }
    }
}

impl std::error::Error for QueryError {}

/// Trait for geometry kernels. Lives in reify-types for dependency inversion —
/// implemented in reify-kernel-occt, consumed by reify-eval via reify-geometry.
pub trait GeometryKernel: Send + Sync {
    /// Execute a geometry operation, returning a handle to the result.
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError>;

    /// Run a query against a handle.
    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError>;

    /// Export a handle to the given format, writing to the provided writer.
    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError>;

    /// Tessellate a handle into a mesh.
    fn tessellate(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<Mesh, TessError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_handle_id_content_hash_deterministic() {
        let h1 = GeometryHandleId(42).content_hash();
        let h2 = GeometryHandleId(42).content_hash();
        assert_eq!(h1, h2);

        let h3 = GeometryHandleId(0).content_hash();
        let h4 = GeometryHandleId(0).content_hash();
        assert_eq!(h3, h4);
    }

    #[test]
    fn geometry_handle_id_content_hash_distinct() {
        let h1 = GeometryHandleId(0).content_hash();
        let h2 = GeometryHandleId(1).content_hash();
        let h3 = GeometryHandleId(u64::MAX).content_hash();

        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }
}

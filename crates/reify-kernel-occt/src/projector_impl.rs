//! `reify_mesh_morph::Projector` implementation over [`OcctKernel`].
//!
//! Wires the three Projector trait methods to the corresponding `OcctKernel`
//! primitives (`closest_point_on_shape` for face/edge, `vertex_point` for
//! vertex) and maps `QueryError` failures to `ProjectorPayload` per PRD
//! `mesh-morphing-phase-2.md` §3.4 / §7.3.

use reify_mesh_morph::{Projector, ProjectorPayload};
use reify_ir::{GeometryHandleId, QueryError};

use crate::OcctKernel;

/// `Projector` impl backed by an [`OcctKernel`] borrow.
///
/// `&'k OcctKernel` lifetime: the projector is short-lived per morph call;
/// the engine holds the kernel for the realization's lifetime which strictly
/// exceeds the morph (PRD §9 Q-9-3). No interior mutability or `Send + Sync`
/// is needed at the projector layer — engine wiring decides thread bridging
/// separately.
pub struct OcctProjector<'k> {
    kernel: &'k OcctKernel,
}

impl<'k> OcctProjector<'k> {
    /// Construct an `OcctProjector` borrowing `kernel` for `'k`.
    pub fn new(kernel: &'k OcctKernel) -> Self {
        Self { kernel }
    }
}

/// Translate a kernel `QueryError` into the `ProjectorPayload` envelope per
/// PRD §7.3 ("Stub-kernel handle returns Err(ProjectorPayload { message:
/// 'kernel returned error: ...' }) with OCCT error text preserved"). The
/// `QueryError` Display impl provides the OCCT error text suffix.
fn wrap_kernel_error(e: QueryError) -> ProjectorPayload {
    ProjectorPayload::new(format!("kernel returned error: {e}"))
}

impl<'k> Projector for OcctProjector<'k> {
    fn project_onto_face(
        &self,
        face: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], ProjectorPayload> {
        self.kernel
            .closest_point_on_shape(face, point[0], point[1], point[2])
            .map_err(wrap_kernel_error)
    }

    fn project_onto_edge(
        &self,
        edge: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], ProjectorPayload> {
        self.kernel
            .closest_point_on_shape(edge, point[0], point[1], point[2])
            .map_err(wrap_kernel_error)
    }

    fn vertex_position(
        &self,
        vertex: GeometryHandleId,
    ) -> Result<[f64; 3], ProjectorPayload> {
        // PRD §3.4: BRep_Tool::Pnt direct; no closest-point. Short-circuits
        // the BRepExtrema path entirely (`vertex_point` is a direct wrapper
        // around `BRep_Tool::Pnt(TopoDS::Vertex(shape))`).
        self.kernel.vertex_point(vertex).map_err(wrap_kernel_error)
    }
}

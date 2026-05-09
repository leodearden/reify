//! Real `GmshKernel` — backed by hand-rolled extern "C" FFI to libgmsh 4.15.2.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.
//!
//! The full `mesh_to_volume(...)` pipeline lands in task 3092 plan step 6;
//! this file exists at pre-3 with a minimal stub-method shell so the
//! cfg-conditional `pub use kernel_real::GmshKernel` in `lib.rs` resolves
//! and the `gmsh_factory()` in `register.rs` can construct it.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

/// Real Gmsh kernel — populated incrementally by task 3092 plan steps.
///
/// At pre-3, this is a placeholder whose trait methods all error. The
/// `mesh_to_volume(...)` non-trait method lands in step 6 with the full
/// pipeline.
pub struct GmshKernel {
    _private: (),
}

impl GmshKernel {
    /// Construct a new `GmshKernel`. The real FFI initialisation runs lazily
    /// on the first `mesh_to_volume` call (via `init::ensure_initialized`).
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for GmshKernel {
    fn default() -> Self {
        Self::new()
    }
}

const PENDING_MSG: &str = "Gmsh trait dispatch through GeometryKernel::execute is not yet \
    routed for Mesh→VolumeMesh; call `GmshKernel::mesh_to_volume` directly. \
    (mesh_to_volume itself is populated in task 3092 plan step 6.)";

impl GeometryKernel for GmshKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(PENDING_MSG.into()))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(PENDING_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(PENDING_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(PENDING_MSG.into()))
    }
}

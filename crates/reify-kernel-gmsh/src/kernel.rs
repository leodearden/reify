//! Stub `GmshKernel` — only compiled when `cfg(not(has_gmsh))` (libgmsh
//! was not detected by `build.rs`). All trait operations return a
//! descriptive error pointing callers at the real entry points.
//!
//! When `cfg(has_gmsh)` is set this module is omitted entirely and
//! `crate::GmshKernel` resolves to [`kernel_real::GmshKernel`](crate::kernel_real::GmshKernel)
//! via the cfg-conditional `pub use` in `lib.rs`.
//!
//! # Design templates
//!
//! `crates/reify-kernel-openvdb/src/kernel.rs` — closest template
//! (cfg(not(has_*))-gated stub, `_private: ()` field, `new()` constructor,
//! all-error trait impl, `assert_stub_kernel_errors!` invocation).

use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

const STUB_MSG: &str = "Gmsh trait dispatch through GeometryKernel::execute is not yet \
    routed for Mesh→VolumeMesh; call `GmshKernel::mesh_to_volume` directly. \
    (libgmsh not detected at build time — building stub-only.)";

/// Stub Gmsh kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external struct-literal construction;
/// callers must go through [`Self::new`] or [`Self::default`]. Matches the
/// OpenVDB/OCCT stub pattern.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct GmshKernel {
    _private: (),
}

impl GmshKernel {
    /// Construct a new stub `GmshKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for GmshKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for GmshKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(STUB_MSG.into()))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(STUB_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(STUB_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(STUB_MSG.into()))
    }
    // extract_edges, extract_faces, execute_with_history, query_many all use
    // the trait defaults — they error in the standard "not supported" fashion.
}

#[cfg(test)]
mod tests {
    use super::*;

    reify_test_support::assert_stub_kernel_errors!(GmshKernel::new, "Gmsh");
}

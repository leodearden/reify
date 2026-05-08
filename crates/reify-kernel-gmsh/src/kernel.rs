//! Stub `GmshKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-openvdb/src/kernel.rs` — closest template
//! (stub-only kernel, `_private: ()` field, `new()` constructor, all-error
//! trait impl, `assert_stub_kernel_errors!` invocation).
//!
//! # v0.3 scope
//!
//! Real Gmsh FFI is deferred to follow-up task #3092. This stub exists so
//! the `inventory::submit!` in `register.rs` has a factory that compiles.
//! When the follow-up task lands, the factory can switch to the real impl
//! behind `cfg(has_gmsh)` without changing the registration shape — see
//! the OCCT precedent (`crates/reify-kernel-occt/build.rs` +
//! `crates/reify-kernel-occt/src/`) for the cfg-gated pattern that lands
//! alongside the FFI.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

const STUB_MSG: &str = "Gmsh volume-mesh kernel not yet implemented; \
    reify-kernel-gmsh is a registration-only scaffold for v0.3 task 2925. \
    Real Gmsh FFI is follow-up task 3092.";

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

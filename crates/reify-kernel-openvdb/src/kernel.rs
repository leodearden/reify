//! Stub `OpenVdbKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real OpenVDB FFI is deferred to a follow-up task. This stub exists so the
//! `inventory::submit!` in `register.rs` has a factory that compiles. When
//! the follow-up task lands, the factory can switch to the real impl behind
//! `cfg(has_openvdb)` without changing the registration shape.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

const STUB_MSG: &str = "OpenVDB voxel kernel not yet implemented; \
    reify-kernel-openvdb is a registration-only scaffold for v0.2 task 2645. \
    Real OpenVDB FFI is a follow-up.";

/// Stub OpenVDB kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external struct-literal construction;
/// callers must go through [`Self::new`] or [`Self::default`].
/// Matches the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct OpenVdbKernel {
    _private: (),
}

impl OpenVdbKernel {
    /// Construct a new stub `OpenVdbKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for OpenVdbKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for OpenVdbKernel {
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

    reify_test_support::assert_stub_kernel_errors!(OpenVdbKernel::new, "OpenVDB");
}

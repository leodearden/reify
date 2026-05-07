//! Stub `FidgetKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real Fidget Rust JIT FFI is deferred to a follow-up task. This stub exists
//! so the `inventory::submit!` in `register.rs` has a factory that compiles.
//! When the follow-up task lands, the factory can switch to the real impl
//! behind `cfg(has_fidget)` without changing the registration shape.

use reify_types::{
    BRepKind, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

const STUB_MSG: &str = "Fidget SDF kernel not yet implemented; \
    reify-kernel-fidget is a registration-only scaffold for the v0.2 multi-kernel system \
    (see docs/prds/v0_2/multi-kernel.md). Real Fidget Rust JIT FFI is a follow-up.";

/// Stub Fidget kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external struct-literal construction;
/// callers must go through [`Self::new`] or [`Self::default`].
/// Matches the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct FidgetKernel {
    _private: (),
}

impl FidgetKernel {
    /// Construct a new stub `FidgetKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for FidgetKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for FidgetKernel {
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

    /// Trait-conformance pin: `FidgetKernel` must be `Send + Sync` and
    /// upcastable to `Box<dyn GeometryKernel>` (the dyn-safe trait surface
    /// `KernelRegistration::factory` returns).
    ///
    /// Replaces the `assert_stub_kernel_errors!(FidgetKernel::new, "Fidget")`
    /// macro invocation: that macro asserted every op returns `Err`, which
    /// is exactly what the wired-in implementation contradicts (Sphere/Box
    /// and the SDF Booleans now succeed).
    #[test]
    fn fidget_kernel_is_send_sync_and_object_safe() {
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        let kernel = FidgetKernel::new();
        assert_send_sync(&kernel);
        let _boxed: Box<dyn GeometryKernel> = Box::new(FidgetKernel::new());
    }

    /// Pins the contract that `execute(GeometryOp::Sphere { radius })`
    /// returns a fresh handle with `BRepKind::Solid` (the closest
    /// fine-grained classifier for "implicit-surface-defined solid"; see
    /// design decision in plan).
    #[test]
    fn fidget_kernel_execute_sphere_returns_handle_with_solid_repr() {
        let mut kernel = FidgetKernel::new();
        let result = kernel.execute(&GeometryOp::Sphere {
            radius: Value::Real(1.0),
        });
        let handle = result.expect("Sphere execution must succeed on FidgetKernel");
        assert_eq!(handle.repr, BRepKind::Solid);
        assert_ne!(
            handle.id,
            GeometryHandleId::INVALID,
            "FidgetKernel must allocate a real handle id, not the INVALID sentinel",
        );
    }
}

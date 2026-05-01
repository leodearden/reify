//! Stub `FidgetKernel` — scaffold for v0.2 task 2644.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real Fidget Rust JIT FFI is deferred to a follow-up task. The `GeometryKernel`
//! impl (all-error stub) arrives in step-4; this pre-1 scaffold provides only
//! the struct and constructors so the crate compiles.

use reify_types::{
    ExportError, ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery,
    Mesh, TessError,
};

/// Stub Fidget kernel — scaffold for v0.2 multi-kernel registration.
///
/// The `_private: ()` field prevents external construction without [`Self::new`],
/// matching the OCCT stub pattern in
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{GeometryError, GeometryHandleId};

    /// Compile-time supertraits pin: `fn assert_send_sync` forces a monomorphism
    /// bound check for `T: Send + Sync`. Calling it with `FidgetKernel` will
    /// fail to compile if a non-Send or non-Sync field is ever added to the
    /// struct — the `Box<dyn GeometryKernel>` check below only catches this
    /// transitively, but this helper makes the constraint explicit.
    fn assert_send_sync<T: Send + Sync>() {}

    /// Structural pin: `Box<dyn GeometryKernel>` from `FidgetKernel` must
    /// compile. This fails at compile time if `FidgetKernel` lacks the
    /// `Send + Sync` supertraits required by the `GeometryKernel` trait object.
    ///
    /// `assert_send_sync::<FidgetKernel>()` makes the `Send + Sync` constraint
    /// explicit and unambiguous — it cannot be satisfied transitively by accident.
    #[test]
    fn fidget_kernel_implements_geometry_kernel_trait() {
        assert_send_sync::<FidgetKernel>();
        let _boxed: Box<dyn GeometryKernel> = Box::new(FidgetKernel::new());
    }

    /// `execute` must return `Err(GeometryError::OperationFailed(msg))` where
    /// `msg` contains "Fidget" for ALL three declared Boolean variants
    /// (`Union`, `Difference`, `Intersection`). Looping over all three prevents
    /// a regression that special-cases only `Union` from slipping through.
    #[test]
    fn fidget_kernel_returns_descriptive_error_for_sdf_boolean() {
        let mut kernel = FidgetKernel::new();
        let ops = [
            GeometryOp::Union {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            },
            GeometryOp::Difference {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            },
            GeometryOp::Intersection {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            },
        ];
        for op in &ops {
            let result = kernel.execute(op);
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("Fidget"),
                        "error message must mention 'Fidget' for op {op:?}, got: {msg:?}",
                    );
                }
                other => panic!(
                    "expected Err(GeometryError::OperationFailed(_)) for op {op:?}, got {other:?}"
                ),
            }
        }
    }

    /// `query`, `export`, and `tessellate` must all return `Err(...)` for any
    /// input, locking the all-error stub contract.
    #[test]
    fn fidget_kernel_query_export_tessellate_all_error() {
        let kernel = FidgetKernel::new();

        let query_result = kernel.query(&GeometryQuery::Volume(GeometryHandleId(1)));
        assert!(query_result.is_err(), "query must return Err(...)");

        let export_result = kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut vec![]);
        assert!(export_result.is_err(), "export must return Err(...)");

        let tess_result = kernel.tessellate(GeometryHandleId(1), 0.1);
        assert!(tess_result.is_err(), "tessellate must return Err(...)");
    }
}

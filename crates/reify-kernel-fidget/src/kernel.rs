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
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

const STUB_MSG: &str = "Fidget SDF kernel not yet implemented; \
    reify-kernel-fidget is a registration-only scaffold for v0.2 task 2644. \
    Real Fidget Rust JIT FFI is a follow-up.";

/// Stub Fidget kernel — all operations return descriptive errors.
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
    use reify_types::{ExportError, GeometryError, GeometryHandleId, QueryError, TessError};

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

    /// STUB_MSG must point to the stable doc path, not a bare task ID.
    ///
    /// Asserts via the public `execute` trait surface (matching the existing
    /// test style) that the error message:
    ///   1. contains `"docs/prds/v0_2/multi-kernel.md"` (stable pointer), and
    ///   2. does NOT contain `"task 2644"` (volatile tracker reference).
    ///
    /// A single `Union` op is sufficient because STUB_MSG is shared across all
    /// variants — see design decision in plan.json.
    #[test]
    fn fidget_stub_msg_points_to_stable_doc_not_bare_task_id() {
        let mut kernel = FidgetKernel::new();
        let op = GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        };
        match kernel.execute(&op) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("docs/prds/v0_2/multi-kernel.md"),
                    "error message must contain stable doc path 'docs/prds/v0_2/multi-kernel.md', got: {msg:?}",
                );
                assert!(
                    !msg.contains("task 2644"),
                    "error message must NOT contain bare task ID 'task 2644', got: {msg:?}",
                );
            }
            other => panic!(
                "expected Err(GeometryError::OperationFailed(_)), got {other:?}"
            ),
        }
    }

    /// `query`, `export`, and `tessellate` must all return `Err(...)` with the
    /// specific error variant and a message mentioning "Fidget", locking the
    /// all-error stub contract symmetrically with `fidget_kernel_returns_descriptive_error_for_sdf_boolean`.
    #[test]
    fn fidget_kernel_query_export_tessellate_all_error() {
        let kernel = FidgetKernel::new();

        match kernel.query(&GeometryQuery::Volume(GeometryHandleId(1))) {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(
                    msg.contains("Fidget"),
                    "query error message must mention 'Fidget', got: {msg:?}",
                );
            }
            other => panic!("expected Err(QueryError::QueryFailed(_)), got {other:?}"),
        }

        match kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut vec![]) {
            Err(ExportError::FormatError(msg)) => {
                assert!(
                    msg.contains("Fidget"),
                    "export error message must mention 'Fidget', got: {msg:?}",
                );
            }
            other => panic!("expected Err(ExportError::FormatError(_)), got {other:?}"),
        }

        match kernel.tessellate(GeometryHandleId(1), 0.1) {
            Err(TessError::TessellationFailed(msg)) => {
                assert!(
                    msg.contains("Fidget"),
                    "tessellate error message must mention 'Fidget', got: {msg:?}",
                );
            }
            other => panic!("expected Err(TessError::TessellationFailed(_)), got {other:?}"),
        }
    }
}

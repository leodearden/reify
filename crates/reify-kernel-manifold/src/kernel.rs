//! Stub `ManifoldKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real Manifold C++ FFI is deferred to a follow-up task. This stub exists
//! so the `inventory::submit!` in `register.rs` has a factory that compiles.
//! When the follow-up task lands, the factory can switch to the real impl
//! behind `cfg(has_manifold)` without changing the registration shape.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

const STUB_MSG: &str = "Manifold mesh booleans not yet implemented; \
    reify-kernel-manifold is a registration-only scaffold for v0.2 task 2643. \
    Real Manifold C++ FFI is a follow-up.";

/// Stub Manifold kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external construction without [`Self::new`],
/// matching the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct ManifoldKernel {
    _private: (),
}

impl ManifoldKernel {
    /// Construct a new stub `ManifoldKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ManifoldKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for ManifoldKernel {
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

    /// Compile-time supertraits pin: `fn assert_send_sync` forces a monomorphism
    /// bound check for `T: Send + Sync`. Calling it with `ManifoldKernel` will
    /// fail to compile if a non-Send or non-Sync field is ever added to the
    /// struct — the `Box<dyn GeometryKernel>` check below only catches this
    /// transitively, but this helper makes the constraint explicit.
    fn assert_send_sync<T: Send + Sync>() {}

    /// Assert that a descriptive-error message contains "Manifold".
    ///
    /// Used by `manifold_kernel_query_export_tessellate_all_error` to pin the
    /// substring contract for `query`, `export`, and `tessellate` without
    /// repeating the `assert!` + format string three times.
    fn assert_manifold_msg(label: &str, msg: &str) {
        assert!(
            msg.contains("Manifold"),
            "{label} error must mention 'Manifold', got: {msg:?}",
        );
    }

    /// Structural pin: `Box<dyn GeometryKernel>` from `ManifoldKernel` must
    /// compile. This fails at compile time if `ManifoldKernel` lacks the
    /// `Send + Sync` supertraits required by the `GeometryKernel` trait object.
    ///
    /// `assert_send_sync::<ManifoldKernel>()` makes the `Send + Sync` constraint
    /// explicit and unambiguous — it cannot be satisfied transitively by accident.
    #[test]
    fn manifold_kernel_implements_geometry_kernel_trait() {
        assert_send_sync::<ManifoldKernel>();
        let _boxed: Box<dyn GeometryKernel> = Box::new(ManifoldKernel::new());
    }

    /// `execute` must return `Err(GeometryError::OperationFailed(msg))` where
    /// `msg` contains "Manifold" for ALL three declared Boolean variants
    /// (`Union`, `Difference`, `Intersection`). Looping over all three prevents
    /// a regression that special-cases only `Union` from slipping through.
    #[test]
    fn manifold_kernel_returns_descriptive_error_for_mesh_boolean() {
        let mut kernel = ManifoldKernel::new();
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
                        msg.contains("Manifold"),
                        "error message must mention 'Manifold' for op {op:?}, got: {msg:?}",
                    );
                }
                other => panic!(
                    "expected Err(GeometryError::OperationFailed(_)) for op {op:?}, got {other:?}"
                ),
            }
        }
    }

    /// `query`, `export`, and `tessellate` must all return descriptive errors
    /// whose message contains "Manifold" (matching the loop in
    /// `manifold_kernel_returns_descriptive_error_for_mesh_boolean`), locking
    /// the all-error stub contract for query/export/tessellate.
    #[test]
    fn manifold_kernel_query_export_tessellate_all_error() {
        let kernel = ManifoldKernel::new();

        let query_result = kernel.query(&GeometryQuery::Volume(GeometryHandleId(1)));
        match query_result {
            Err(QueryError::QueryFailed(msg)) => assert_manifold_msg("query", &msg),
            other => panic!(
                "expected Err(QueryError::QueryFailed(_)) from query, got {other:?}"
            ),
        }

        let export_result = kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut vec![]);
        match export_result {
            Err(ExportError::FormatError(msg)) => assert_manifold_msg("export", &msg),
            other => panic!(
                "expected Err(ExportError::FormatError(_)) from export, got {other:?}"
            ),
        }

        let tess_result = kernel.tessellate(GeometryHandleId(1), 0.1);
        match tess_result {
            Err(TessError::TessellationFailed(msg)) => assert_manifold_msg("tessellate", &msg),
            other => panic!(
                "expected Err(TessError::TessellationFailed(_)) from tessellate, got {other:?}"
            ),
        }
    }
}

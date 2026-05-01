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

/// Stub OpenVDB kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external construction without [`Self::new`],
/// matching the OCCT stub pattern in
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{
        ExportError, ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp,
        GeometryQuery, QueryError, TessError,
    };

    /// Compile-time supertraits pin: `fn assert_send_sync` forces a monomorphism
    /// bound check for `T: Send + Sync`. Calling it with `OpenVdbKernel` will
    /// fail to compile if a non-Send or non-Sync field is ever added to the
    /// struct — the `Box<dyn GeometryKernel>` check below only catches this
    /// transitively, but this helper makes the constraint explicit.
    fn assert_send_sync<T: Send + Sync>() {}

    /// Structural pin: `Box<dyn GeometryKernel>` from `OpenVdbKernel` must
    /// compile. This fails at compile time if `OpenVdbKernel` lacks the
    /// `Send + Sync` supertraits required by the `GeometryKernel` trait object.
    ///
    /// `assert_send_sync::<OpenVdbKernel>()` makes the `Send + Sync` constraint
    /// explicit and unambiguous — it cannot be satisfied transitively by accident.
    #[test]
    fn openvdb_kernel_implements_geometry_kernel_trait() {
        assert_send_sync::<OpenVdbKernel>();
        let _boxed: Box<dyn GeometryKernel> = Box::new(OpenVdbKernel::new());
    }

    /// `execute` must return `Err(GeometryError::OperationFailed(msg))` where
    /// `msg` contains "OpenVDB" for ALL three declared Boolean variants
    /// (`Union`, `Difference`, `Intersection`). Looping over all three prevents
    /// a regression that special-cases only `Union` from slipping through.
    #[test]
    fn openvdb_kernel_returns_descriptive_error_for_voxel_boolean() {
        let mut kernel = OpenVdbKernel::new();
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
                        msg.contains("OpenVDB"),
                        "error message must mention 'OpenVDB' for op {op:?}, got: {msg:?}",
                    );
                }
                other => panic!(
                    "expected Err(GeometryError::OperationFailed(_)) for op {op:?}, got {other:?}"
                ),
            }
        }
    }

    /// `query`, `export`, and `tessellate` must all return `Err(...)` with the
    /// specific error variant and a message mentioning "OpenVDB", locking the
    /// all-error stub contract symmetrically with
    /// `openvdb_kernel_returns_descriptive_error_for_voxel_boolean`.
    #[test]
    fn openvdb_kernel_query_export_tessellate_all_error() {
        let kernel = OpenVdbKernel::new();

        match kernel.query(&GeometryQuery::Volume(GeometryHandleId(1))) {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(
                    msg.contains("OpenVDB"),
                    "query error message must mention 'OpenVDB', got: {msg:?}",
                );
            }
            other => panic!("expected Err(QueryError::QueryFailed(_)), got {other:?}"),
        }

        match kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut vec![]) {
            Err(ExportError::FormatError(msg)) => {
                assert!(
                    msg.contains("OpenVDB"),
                    "export error message must mention 'OpenVDB', got: {msg:?}",
                );
            }
            other => panic!("expected Err(ExportError::FormatError(_)), got {other:?}"),
        }

        match kernel.tessellate(GeometryHandleId(1), 0.1) {
            Err(TessError::TessellationFailed(msg)) => {
                assert!(
                    msg.contains("OpenVDB"),
                    "tessellate error message must mention 'OpenVDB', got: {msg:?}",
                );
            }
            other => panic!("expected Err(TessError::TessellationFailed(_)), got {other:?}"),
        }
    }
}

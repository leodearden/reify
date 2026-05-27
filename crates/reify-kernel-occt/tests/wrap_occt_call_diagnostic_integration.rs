//! Regression test pinning the Standard_Failure catch-arm diagnostic format.
//!
//! This test verifies that a C++ `Standard_Failure` exception propagates through
//! the OCCT wrapper with the expected prefix scheme:
//!
//!   `"OCCT <wrapper_name>: <message>"`   (Standard_Failure arm)
//!
//! and does NOT include the `"unexpected:"` marker, which is reserved for the
//! `std::exception` catch arm.
//!
//! The trigger: `query_moment_of_inertia(handle, [0, 0, 0])`.  Rust does NOT
//! pre-validate the axis on the `MomentOfInertia` query path (lib.rs:947-953),
//! so the zero vector reaches the C++ wrapper where `gp_Dir(0,0,0)` raises
//! `gp_VectorWithNullMagnitude` (a `Standard_Failure` subclass).
//!
//! This test must pass against both the pre-helper code AND all migration batches
//! of `wrap_occt_call`; it locks in the observable error-format contract.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helper
// ---------------------------------------------------------------------------

/// Build a kernel containing one 10 mm × 10 mm × 10 mm box; return the kernel
/// and the handle id of the box.
fn box_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.010),
            height: Value::Real(0.010),
            depth: Value::Real(0.010),
        })
        .expect("Box creation should succeed");
    (kernel, box_h.id)
}

// ---------------------------------------------------------------------------
// Standard_Failure arm regression test
// ---------------------------------------------------------------------------

/// Passing a zero-magnitude axis `[0, 0, 0]` to `query_moment_of_inertia`
/// triggers `gp_Dir(0,0,0)` → `gp_VectorWithNullMagnitude` (a `Standard_Failure`
/// subclass) inside the C++ wrapper.
///
/// Expected outcome:
/// - Error variant: `QueryError::QueryFailed(_)`
/// - Message contains `"OCCT query_moment_of_inertia:"` (standard prefix)
/// - Message does NOT contain `"unexpected:"` (that marker is for `std::exception`)
#[test]
fn zero_axis_moment_of_inertia_produces_standard_failure_prefix() {
    let (kernel, handle) = box_kernel();

    let result = kernel.query(&GeometryQuery::MomentOfInertia {
        handle,
        axis: [0.0, 0.0, 0.0],
    });

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("OCCT query_moment_of_inertia:"),
                "expected Standard_Failure prefix 'OCCT query_moment_of_inertia:' in error message, got: {msg:?}"
            );
            assert!(
                !msg.contains("unexpected:"),
                "Standard_Failure arm must NOT include 'unexpected:' marker, got: {msg:?}"
            );
        }
        Ok(v) => panic!("expected Err(QueryFailed) for zero-magnitude axis, got Ok({v:?})"),
        Err(other) => {
            panic!("expected Err(QueryFailed) for zero-magnitude axis, got Err({other:?})")
        }
    }
}

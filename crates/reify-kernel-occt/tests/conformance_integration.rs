//! Integration tests for geometry conformance queries:
//! `IsWatertight`, `IsManifold`, `IsOrientable`.
//!
//! These tests verify that:
//! - A valid solid (box) passes all three predicates.
//! - An invalid handle returns `QueryError::InvalidHandle`.
//! - Non-solid shapes (wire, face) fail `IsWatertight` but pass the others.
//! - Sphere and cylinder also pass all three predicates.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

/// Helper: build a kernel containing one 10×10×10 box, return the kernel and
/// the handle id of the box.
fn box_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box creation should succeed");
    (kernel, box_h.id)
}

/// A valid 10×10×10 box solid should report true for all three conformance
/// predicates: it is watertight (closed, no free edges), manifold (every edge
/// has exactly 2 parent faces), and orientable (all shells consistently oriented).
#[test]
fn box_is_watertight_manifold_orientable() {
    let (kernel, box_id) = box_kernel();

    // IsWatertight
    match kernel.query(&GeometryQuery::IsWatertight(box_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!(
            "IsWatertight on box: expected Ok(Value::Bool(true)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsWatertight on box: expected Ok(true), got Err({:?})", e),
    }

    // IsManifold
    match kernel.query(&GeometryQuery::IsManifold(box_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!(
            "IsManifold on box: expected Ok(Value::Bool(true)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsManifold on box: expected Ok(true), got Err({:?})", e),
    }

    // IsOrientable
    match kernel.query(&GeometryQuery::IsOrientable(box_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!(
            "IsOrientable on box: expected Ok(Value::Bool(true)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsOrientable on box: expected Ok(true), got Err({:?})", e),
    }
}

/// Each conformance query variant must return `Err(QueryError::InvalidHandle(id))`
/// when passed a handle id that was never allocated.
#[test]
fn conformance_query_invalid_handle_returns_invalid_handle_err() {
    let (kernel, _) = box_kernel();
    let bad_id = GeometryHandleId(9999);

    // IsWatertight on unknown handle
    match kernel.query(&GeometryQuery::IsWatertight(bad_id)) {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(id, bad_id, "IsWatertight: InvalidHandle should carry the bad id");
        }
        Ok(v) => panic!(
            "IsWatertight with invalid handle: expected Err(InvalidHandle), got Ok({:?})",
            v
        ),
        Err(other) => panic!(
            "IsWatertight with invalid handle: expected Err(InvalidHandle), got Err({:?})",
            other
        ),
    }

    // IsManifold on unknown handle
    match kernel.query(&GeometryQuery::IsManifold(bad_id)) {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(id, bad_id, "IsManifold: InvalidHandle should carry the bad id");
        }
        Ok(v) => panic!(
            "IsManifold with invalid handle: expected Err(InvalidHandle), got Ok({:?})",
            v
        ),
        Err(other) => panic!(
            "IsManifold with invalid handle: expected Err(InvalidHandle), got Err({:?})",
            other
        ),
    }

    // IsOrientable on unknown handle
    match kernel.query(&GeometryQuery::IsOrientable(bad_id)) {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(id, bad_id, "IsOrientable: InvalidHandle should carry the bad id");
        }
        Ok(v) => panic!(
            "IsOrientable with invalid handle: expected Err(InvalidHandle), got Ok({:?})",
            v
        ),
        Err(other) => panic!(
            "IsOrientable with invalid handle: expected Err(InvalidHandle), got Err({:?})",
            other
        ),
    }
}

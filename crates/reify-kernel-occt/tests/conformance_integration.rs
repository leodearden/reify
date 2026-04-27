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

/// TAU = 2π for a full-circle arc.
const TAU: f64 = std::f64::consts::TAU;

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

/// A full-circle arc wire (360 degrees, radius 5 mm) is NOT watertight because
/// it is a `TopAbs_WIRE` — the shape-type guard in `is_watertight` must return
/// `false` for wire shapes.  It IS manifold (no edges have 3+ parent faces;
/// in fact this wire has zero face parents at all) and IS orientable (no shells
/// loaded → `ShapeAnalysis_Shell::NbLoaded() == 0` → trivially `true`).
#[test]
fn circle_wire_is_not_watertight_but_is_manifold_and_orientable() {
    let mut kernel = OcctKernel::new();
    let wire_h = kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: 0.005,
            start_angle: 0.0,
            end_angle: TAU,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("full-circle Arc (start=0, end=TAU) should succeed");
    let wire_id = wire_h.id;

    // shape-type guard must fire → false
    match kernel.query(&GeometryQuery::IsWatertight(wire_id)) {
        Ok(Value::Bool(false)) => {}
        Ok(other) => panic!(
            "IsWatertight on wire: expected Ok(Value::Bool(false)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsWatertight on wire: expected Ok(false), got Err({:?})", e),
    }

    // no edges with 3+ face parents → manifold
    match kernel.query(&GeometryQuery::IsManifold(wire_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!(
            "IsManifold on wire: expected Ok(Value::Bool(true)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsManifold on wire: expected Ok(true), got Err({:?})", e),
    }

    // NbLoaded() == 0 → trivially orientable
    match kernel.query(&GeometryQuery::IsOrientable(wire_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!(
            "IsOrientable on wire: expected Ok(Value::Bool(true)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsOrientable on wire: expected Ok(true), got Err({:?})", e),
    }
}

/// A single circle face (`TopAbs_FACE`) is NOT watertight — the shape-type guard
/// in `is_watertight` must return `false` for face shapes.
///
/// Uses `OcctKernel::store_circle_face_for_test`, a test-only helper that
/// wraps `ffi::ffi::make_circle_face` and stores the result in the kernel.
#[test]
fn single_face_is_not_watertight() {
    let mut kernel = OcctKernel::new();
    // `store_circle_face_for_test` is a `#[cfg(all(test, has_occt))] pub` method
    // added to OcctKernel so integration tests can inject a circle face fixture
    // without needing direct access to the private `ffi` module.
    let face_id = kernel.store_circle_face_for_test(0.005, 0.0);

    match kernel.query(&GeometryQuery::IsWatertight(face_id)) {
        Ok(Value::Bool(false)) => {}
        Ok(other) => panic!(
            "IsWatertight on circle face: expected Ok(Value::Bool(false)), got Ok({:?})",
            other
        ),
        Err(e) => panic!("IsWatertight on circle face: expected Ok(false), got Err({:?})", e),
    }
}

/// A sphere (radius 5 mm) and a cylinder (radius 3 mm, height 10 mm) are both
/// closed, manifold, and consistently-oriented solids.  All three conformance
/// predicates must return `true` for each, confirming positive coverage beyond
/// the 10×10×10 box tested in `box_is_watertight_manifold_orientable`.
#[test]
fn sphere_and_cylinder_pass_all_three_conformance_queries() {
    let mut kernel = OcctKernel::new();

    // --- sphere ---
    let sphere_h = kernel
        .execute(&GeometryOp::Sphere { radius: Value::Real(0.005) })
        .expect("Sphere creation should succeed");
    let sphere_id = sphere_h.id;

    match kernel.query(&GeometryQuery::IsWatertight(sphere_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsWatertight on sphere: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsWatertight on sphere: expected Ok(true), got Err({:?})", e),
    }
    match kernel.query(&GeometryQuery::IsManifold(sphere_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsManifold on sphere: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsManifold on sphere: expected Ok(true), got Err({:?})", e),
    }
    match kernel.query(&GeometryQuery::IsOrientable(sphere_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsOrientable on sphere: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsOrientable on sphere: expected Ok(true), got Err({:?})", e),
    }

    // --- cylinder ---
    let cyl_h = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.003),
            height: Value::Real(0.010),
        })
        .expect("Cylinder creation should succeed");
    let cyl_id = cyl_h.id;

    match kernel.query(&GeometryQuery::IsWatertight(cyl_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsWatertight on cylinder: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsWatertight on cylinder: expected Ok(true), got Err({:?})", e),
    }
    match kernel.query(&GeometryQuery::IsManifold(cyl_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsManifold on cylinder: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsManifold on cylinder: expected Ok(true), got Err({:?})", e),
    }
    match kernel.query(&GeometryQuery::IsOrientable(cyl_id)) {
        Ok(Value::Bool(true)) => {}
        Ok(other) => panic!("IsOrientable on cylinder: expected Ok(Bool(true)), got Ok({:?})", other),
        Err(e) => panic!("IsOrientable on cylinder: expected Ok(true), got Err({:?})", e),
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

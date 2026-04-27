//! Integration tests for geometry conformance queries:
//! `IsWatertight`, `IsManifold`, `IsOrientable`.
//!
//! These tests verify that:
//! - A valid solid (box) passes all three predicates.
//! - An invalid handle returns `QueryError::InvalidHandle`.
//! - Non-solid shapes (wire, face) fail `IsWatertight` but pass the others.
//! - Sphere and cylinder also pass all three predicates.
//! - Negative-path cases: non-manifold compound, malformed solid, non-orientable shell.
//!
//! All fixtures use SI-meter units throughout (e.g. 0.010 m = 10 mm). The choice
//! of unit does not affect conformance predicates, but consistency helps readers
//! understand that any numeric scale would produce the same result.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

/// TAU = 2π for a full-circle arc.
const TAU: f64 = std::f64::consts::TAU;

// ---------------------------------------------------------------------------
// Shared assertion helper
// ---------------------------------------------------------------------------

/// Assert that `kernel.query(&q)` returns `Ok(Value::Bool(expected))`.
///
/// On mismatch this panics with a message that includes `label` so failures
/// are easy to locate in test output.
fn assert_bool_query(kernel: &OcctKernel, q: GeometryQuery, expected: bool, label: &str) {
    match kernel.query(&q) {
        Ok(Value::Bool(got)) if got == expected => {}
        Ok(Value::Bool(got)) => panic!(
            "{label}: expected Ok(Bool({expected})), got Ok(Bool({got}))"
        ),
        Ok(other) => panic!(
            "{label}: expected Ok(Bool({expected})), got Ok({other:?})"
        ),
        Err(e) => panic!(
            "{label}: expected Ok(Bool({expected})), got Err({e:?})"
        ),
    }
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Helper: build a kernel containing one 10 mm × 10 mm × 10 mm box, return the
/// kernel and the handle id of the box.
///
/// All fixtures in this file use SI-meter units (same scale as sphere/cylinder
/// fixtures below) so that unit choice is invisible to conformance predicates.
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
// Positive-path tests
// ---------------------------------------------------------------------------

/// A valid 10 mm × 10 mm × 10 mm box solid should report true for all three
/// conformance predicates: it is watertight (closed, no free edges), manifold
/// (every edge has exactly 2 parent faces), and orientable (all shells
/// consistently oriented).
#[test]
fn box_is_watertight_manifold_orientable() {
    let (kernel, box_id) = box_kernel();

    assert_bool_query(&kernel, GeometryQuery::IsWatertight(box_id), true,  "IsWatertight on box");
    assert_bool_query(&kernel, GeometryQuery::IsManifold(box_id),   true,  "IsManifold on box");
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(box_id), true,  "IsOrientable on box");
}

/// A stand-alone closed shell extracted from a 10×10×10 mm box (via
/// `TopExp_Explorer(box, TopAbs_SHELL)`) must pass all three conformance
/// predicates.
///
/// This exercises the SHELL guard arm in `is_watertight`: previously the only
/// code path reaching `BRepCheck_Analyzer` for a shell came from the
/// `malformed-solid` fixture, which is queried as a `TopAbs_SOLID` (the shell
/// is wrapped inside the solid). This test queries the shape directly as
/// `TopAbs_SHELL`, reaching the `type == TopAbs_SHELL` branch in the guard.
#[test]
fn closed_shell_passes_all_three_conformance_queries() {
    let mut kernel = OcctKernel::new();
    let shell_id = kernel.store_closed_shell_for_test();

    assert_bool_query(&kernel, GeometryQuery::IsWatertight(shell_id), true,  "IsWatertight on closed shell");
    assert_bool_query(&kernel, GeometryQuery::IsManifold(shell_id),   true,  "IsManifold on closed shell");
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(shell_id), true,  "IsOrientable on closed shell");
}

/// A sphere (radius 5 mm) and a cylinder (radius 3 mm, height 10 mm) are both
/// closed, manifold, and consistently-oriented solids.  All three conformance
/// predicates must return `true` for each, confirming positive coverage beyond
/// the 10×10×10 mm box tested in `box_is_watertight_manifold_orientable`.
#[test]
fn sphere_and_cylinder_pass_all_three_conformance_queries() {
    let mut kernel = OcctKernel::new();

    // --- sphere (radius 5 mm) ---
    let sphere_h = kernel
        .execute(&GeometryOp::Sphere { radius: Value::Real(0.005) })
        .expect("Sphere creation should succeed");
    let sphere_id = sphere_h.id;

    assert_bool_query(&kernel, GeometryQuery::IsWatertight(sphere_id), true, "IsWatertight on sphere");
    assert_bool_query(&kernel, GeometryQuery::IsManifold(sphere_id),   true, "IsManifold on sphere");
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(sphere_id), true, "IsOrientable on sphere");

    // --- cylinder (radius 3 mm, height 10 mm) ---
    let cyl_h = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.003),
            height: Value::Real(0.010),
        })
        .expect("Cylinder creation should succeed");
    let cyl_id = cyl_h.id;

    assert_bool_query(&kernel, GeometryQuery::IsWatertight(cyl_id), true, "IsWatertight on cylinder");
    assert_bool_query(&kernel, GeometryQuery::IsManifold(cyl_id),   true, "IsManifold on cylinder");
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(cyl_id), true, "IsOrientable on cylinder");
}

// ---------------------------------------------------------------------------
// Shape-type-guard negative tests
// ---------------------------------------------------------------------------

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
    assert_bool_query(&kernel, GeometryQuery::IsWatertight(wire_id), false, "IsWatertight on wire");
    // no edges with 3+ face parents → manifold
    assert_bool_query(&kernel, GeometryQuery::IsManifold(wire_id),   true,  "IsManifold on wire");
    // NbLoaded() == 0 → trivially orientable
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(wire_id), true,  "IsOrientable on wire");
}

/// A single edge (`TopAbs_EDGE`) hits the shape-type guard in `is_watertight`
/// and returns `false`.
///
/// It IS manifold (no edges with 3+ face parents; an isolated edge has no face
/// parents at all) and IS orientable (`ShapeAnalysis_Shell::NbLoaded() == 0`
/// because an edge has no shells → trivially `true`).
///
/// This exercises the `TopAbs_EDGE` short-circuit path in `is_watertight`.
#[test]
fn edge_is_not_watertight_but_is_manifold_and_orientable() {
    let mut kernel = OcctKernel::new();
    let edge_id = kernel.store_edge_for_test();

    // shape-type guard fires → false
    assert_bool_query(&kernel, GeometryQuery::IsWatertight(edge_id), false, "IsWatertight on edge");
    // isolated edge: no edge→face incidence → trivially manifold
    assert_bool_query(&kernel, GeometryQuery::IsManifold(edge_id),   true,  "IsManifold on edge");
    // no shells loaded → NbLoaded() == 0 → trivially orientable
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(edge_id), true,  "IsOrientable on edge");
}

/// A single vertex (`TopAbs_VERTEX`) hits the shape-type guard in `is_watertight`
/// and returns `false`.
///
/// It IS manifold (a vertex has no edge→face incidence at all) and IS orientable
/// (`ShapeAnalysis_Shell::NbLoaded() == 0` — a vertex has no shells → trivially
/// `true`).
///
/// This exercises the `TopAbs_VERTEX` short-circuit path in `is_watertight`.
#[test]
fn vertex_is_not_watertight_but_is_manifold_and_orientable() {
    let mut kernel = OcctKernel::new();
    let vertex_id = kernel.store_vertex_for_test();

    // shape-type guard fires → false
    assert_bool_query(&kernel, GeometryQuery::IsWatertight(vertex_id), false, "IsWatertight on vertex");
    // no edge→face incidence → trivially manifold
    assert_bool_query(&kernel, GeometryQuery::IsManifold(vertex_id),   true,  "IsManifold on vertex");
    // no shells loaded → NbLoaded() == 0 → trivially orientable
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(vertex_id), true,  "IsOrientable on vertex");
}

/// A single circle face (`TopAbs_FACE`) is NOT watertight — the shape-type guard
/// in `is_watertight` must return `false` for face shapes.
///
/// Uses `OcctKernel::store_circle_face_for_test`, a test-only helper that
/// wraps `ffi::ffi::make_circle_face` and stores the result in the kernel.
/// The method is gated by `#[cfg(has_occt)]` (not `test`) because integration
/// tests link the library in normal build mode, where #[cfg(test)] items are
/// invisible.
#[test]
fn single_face_is_not_watertight() {
    let mut kernel = OcctKernel::new();
    let face_id = kernel.store_circle_face_for_test(0.005, 0.0);

    assert_bool_query(&kernel, GeometryQuery::IsWatertight(face_id), false, "IsWatertight on circle face");
}

// ---------------------------------------------------------------------------
// Analyzer negative tests (shape-type guard does NOT fire)
// ---------------------------------------------------------------------------

/// A compound of 3 faces sewn around a common edge is non-manifold: the shared
/// edge has 3 parent faces, violating the ≤ 2 condition.
///
/// `is_manifold` walks the cached `edge_face_map` and must return `false` here.
/// Note: `is_watertight` returns `false` because COMPOUND is excluded from the
/// shape-type guard (a compound of open faces is not watertight by definition).
#[test]
fn nonmanifold_compound_fails_is_manifold() {
    let mut kernel = OcctKernel::new();
    let shape_id = kernel.store_nonmanifold_compound_for_test();

    // COMPOUND hits the shape-type guard → always false for watertight
    assert_bool_query(&kernel, GeometryQuery::IsWatertight(shape_id), false, "IsWatertight on nonmanifold compound");
    // The shared edge has 3 parent faces → manifold check fails
    assert_bool_query(&kernel, GeometryQuery::IsManifold(shape_id), false, "IsManifold on nonmanifold compound");
}

/// A malformed solid built from an open shell (5 faces of a box) causes
/// `BRepCheck_Analyzer::IsValid()` to return false — the solid's shell is not
/// closed, so the analyzer's solid-level check fails. This exercises the
/// analyzer branch of `is_watertight` rather than the shape-type guard.
#[test]
fn malformed_solid_fails_is_watertight() {
    let mut kernel = OcctKernel::new();
    let shape_id = kernel.store_malformed_solid_for_test();

    // SOLID passes the guard but BRepCheck_Analyzer reports it as invalid
    assert_bool_query(&kernel, GeometryQuery::IsWatertight(shape_id), false, "IsWatertight on malformed solid");
}

/// A shell whose two adjacent faces have the same (rather than opposite) edge
/// orientation causes `ShapeAnalysis_Shell::CheckOrientedShells` to flag the
/// shared edge as "bad", so `is_orientable` returns `false`.
#[test]
fn nonorientable_shell_fails_is_orientable() {
    let mut kernel = OcctKernel::new();
    let shape_id = kernel.store_nonorientable_shell_for_test();

    // SHELL passes the shape-type guard; the shell has shells loaded → not trivially true
    assert_bool_query(&kernel, GeometryQuery::IsOrientable(shape_id), false, "IsOrientable on non-orientable shell");
}

// ---------------------------------------------------------------------------
// Invalid-handle error tests
// ---------------------------------------------------------------------------

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

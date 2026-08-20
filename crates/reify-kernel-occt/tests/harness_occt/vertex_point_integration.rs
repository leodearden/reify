//! Integration tests for `OcctKernel::vertex_point` — direct
//! `BRep_Tool::Pnt(TopoDS::Vertex(...))` accessor for stored `TopAbs_VERTEX`
//! shapes (task 3535, PRD §3.4 `vertex_position`).
//!
//! Distinct from `closest_point_on_shape`: this method short-circuits the
//! `BRepExtrema_DistShapeShape` machinery and reads the exact `gp_Pnt` from
//! the underlying `TopoDS_Vertex`. The PRD specifies "BRep_Tool::Pnt direct;
//! no closest-point" — the test fixture `store_vertex_at_for_test(x, y, z)`
//! lets us pin a non-origin location so the happy-path assertion catches
//! "always-zero" regressions a buggy impl might hide.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, QueryError};

/// `vertex_point` on a stored vertex returns the exact coordinates the
/// fixture placed it at, within `1e-9` (FP round-trip through the C++ `gp_Pnt`
/// constructor and Rust `f64` is bit-exact for finite values; we leave a
/// margin in case of future cxx-bridge struct-packing changes).
#[test]
fn vertex_point_returns_exact_coordinates_of_stored_vertex() {
    let mut kernel = OcctKernel::new();
    let vertex_id = kernel.store_vertex_at_for_test(1.5, -2.5, 3.5);

    match kernel.vertex_point(vertex_id) {
        Ok([x, y, z]) => {
            assert!((x - 1.5).abs() < 1e-9, "expected x≈1.5, got {x}");
            assert!((y - (-2.5)).abs() < 1e-9, "expected y≈-2.5, got {y}");
            assert!((z - 3.5).abs() < 1e-9, "expected z≈3.5, got {z}");
        }
        Err(e) => panic!("expected Ok([1.5, -2.5, 3.5]), got Err({e:?})"),
    }
}

/// `vertex_point` on an unknown handle returns `QueryError::InvalidHandle`
/// (not `QueryFailed`). Pins the `InvalidHandle` error-mapping branch in
/// `OcctKernel::vertex_point` that the happy-path test in
/// `vertex_point_returns_exact_coordinates_of_stored_vertex` doesn't cover.
#[test]
fn vertex_point_unknown_handle_returns_invalid_handle() {
    let kernel = OcctKernel::new();
    let unknown = GeometryHandleId(999);
    match kernel.vertex_point(unknown) {
        Err(QueryError::InvalidHandle(h)) => {
            assert_eq!(h, unknown, "InvalidHandle should carry the queried id");
        }
        Ok(p) => panic!("expected Err(InvalidHandle(999)), got Ok({p:?})"),
        Err(e) => panic!("expected Err(InvalidHandle(999)), got Err({e:?})"),
    }
}

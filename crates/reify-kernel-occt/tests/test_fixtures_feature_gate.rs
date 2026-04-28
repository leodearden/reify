//! Smoke test that pins the public surface exposed by the `test-fixtures` cargo
//! feature.
//!
//! If this file ever stops compiling, either the feature was removed from
//! `Cargo.toml`, or one of the gated helpers was renamed/removed. All 8
//! `store_*_for_test` methods must be reachable with their expected signatures.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use reify_kernel_occt::OcctKernel;
use reify_types::GeometryHandleId;

/// Compile-time surface pins for all 8 `store_*_for_test` fixture helpers.
///
/// Each `let _: fn(...) -> GeometryHandleId = OcctKernel::method;` line is a
/// function-pointer coercion checked entirely at compile time — no OCCT FFI is
/// invoked. If any helper is removed, renamed, or its signature changes, this
/// file fails to compile.
///
/// Runtime behaviour (that helpers actually store shapes and return distinct
/// handles) is covered by `conformance_integration.rs`, which exercises every
/// helper against a live `OcctKernel`.
#[test]
fn all_eight_test_fixture_signatures_visible_under_feature() {
    let _: fn(&mut OcctKernel, f64, f64) -> GeometryHandleId =
        OcctKernel::store_circle_face_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId =
        OcctKernel::store_nonmanifold_compound_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId =
        OcctKernel::store_malformed_solid_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId =
        OcctKernel::store_nonorientable_shell_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId = OcctKernel::store_closed_shell_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId = OcctKernel::store_edge_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId = OcctKernel::store_vertex_for_test;
    let _: fn(&mut OcctKernel) -> GeometryHandleId = OcctKernel::store_compsolid_for_test;
}

//! OCCT kernel integration tests for the `half_space` primitive (task #3465, step-5).
//!
//! These tests call the `make_half_space_for_test` fixture on `OcctKernel`
//! directly — they do NOT depend on `GeometryOp::HalfSpace` (which is added in
//! step-8). This isolates the C++ / FFI correctness from the Rust enum cascade.
//!
//! # RED until step-6
//!
//! `OcctKernel::make_half_space_for_test` does not exist until step-6 adds:
//!   - `ffi::ffi::make_half_space` to the cxx bridge (`ffi.rs`)
//!   - `fn make_half_space_for_test` to the `test-fixtures` impl of `OcctKernel` (`lib.rs`)
//!   - `make_half_space` in `cpp/occt_wrapper.cpp`
//!
//! Tests here:
//!   (a) A valid unit-+Z half_space builds without error and is a non-null shape.
//!   (b) Bisection identity: `intersection(half_space(p_center, nz), box(2a,2a,2a))`
//!       has volume ≈ ½·V_box (tolerance 1e-6·V_box). Orientation-independent: the
//!       bisection is exact by symmetry, regardless of which side MakeHalfSpace keeps.
//!   (c) A zero-length normal (0,0,0) returns `Err` (gp_Dir cannot normalise it).

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};
use reify_ir::{GeometryOp, GeometryQuery, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Assert that the given geometry handle has volume approximately equal to
/// `expected`, within `tolerance` (absolute, in cubic metres).
#[track_caller]
fn assert_volume_approx(
    kernel: &mut OcctKernel,
    id: reify_ir::GeometryHandleId,
    expected: f64,
    tolerance: f64,
    label: &str,
) {
    let vol = kernel
        .query(&GeometryQuery::Volume(id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be a Real value");
    assert!(
        (vol - expected).abs() <= tolerance,
        "{label}: expected volume ≈ {expected:.6e}, got {vol:.6e} (diff={:.2e})",
        (vol - expected).abs()
    );
}

// ---------------------------------------------------------------------------
// (a) Valid half_space construction
// ---------------------------------------------------------------------------

/// A half_space with a unit +Z normal and plane at the origin builds
/// successfully and is a valid (non-null) shape.
///
/// RED until step-6 adds `make_half_space_for_test` + FFI.
#[test]
fn half_space_unit_z_normal_builds_ok() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping half_space_unit_z_normal_builds_ok: OCCT unavailable");
        return;
    }

    let mut kernel = OcctKernel::new();
    // Plane at origin, retained side = +Z  (all z > 0)
    let result = kernel.make_half_space_for_test(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    assert!(
        result.is_ok(),
        "half_space with unit +Z normal must build successfully, got: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// (b) Bisection identity
// ---------------------------------------------------------------------------

/// `intersection(half_space(center_of_box, +Z), box_at_origin(2a×2a×2a))` has
/// volume exactly ½·(2a)³.
///
/// Setup:
///   - Box: 2a×2a×2a, origin at (0,0,0) → occupies [0,2a]×[0,2a]×[0,2a].
///   - half_space boundary plane: passes through the box center (a,a,a),
///     outward normal +Z → keeps the top half [0,2a]×[0,2a]×[a,2a].
///   - By reflection symmetry the intersection volume is exactly ½·V_box,
///     regardless of which side OCCT's `BRepPrimAPI_MakeHalfSpace` keeps
///     (the test is orientation-independent: the bottom half [0,a] is also
///     exactly ½·V_box).
///
/// Tolerance: 1e-6·V_box. GProp::VolumeProperties is analytically exact for
/// planar-faced solids (relative error ~ 1e-12), so 1e-6 is comfortably loose.
///
/// RED until step-6.
#[test]
fn half_space_intersection_with_box_has_half_volume() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping half_space_intersection_with_box_has_half_volume: OCCT unavailable");
        return;
    }

    let a = 10.0e-3_f64; // 10 mm
    let v_box = (2.0 * a).powi(3);
    let half_v = v_box / 2.0;
    let tol = 1.0e-6 * v_box;

    let mut kernel = OcctKernel::new();

    // Build the box: 2a × 2a × 2a (OCCT primitive is origin-cornered)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(2.0 * a),
            height: Value::Real(2.0 * a),
            depth: Value::Real(2.0 * a),
        })
        .expect("box should build");

    // Build the half_space with boundary plane through the box center (a, a, a),
    // normal +Z (retained side = top half).
    let hs_id = kernel
        .make_half_space_for_test(a, a, a, 0.0, 0.0, 1.0)
        .expect("half_space should build");

    // Intersect: keeps the portion of the box on the retained side of the plane.
    let inter = kernel
        .execute(&GeometryOp::Intersection {
            left: box_h.id,
            right: hs_id,
        })
        .expect("intersection should succeed");

    assert_volume_approx(&mut kernel, inter.id, half_v, tol, "intersection volume");
}

// ---------------------------------------------------------------------------
// (c) Zero-length normal → Err
// ---------------------------------------------------------------------------

/// A zero-length normal (0,0,0) must return `Err` because `gp_Dir` cannot
/// normalise a zero vector (throws a C++ exception caught by `wrap_occt_call`).
///
/// RED until step-6.
#[test]
fn half_space_zero_normal_returns_err() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping half_space_zero_normal_returns_err: OCCT unavailable");
        return;
    }

    let mut kernel = OcctKernel::new();
    let result = kernel.make_half_space_for_test(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    assert!(
        result.is_err(),
        "half_space with zero-length normal must return Err, got: {:?}",
        result
    );
}

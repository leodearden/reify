//! Integration tests for `GeometryOp::NurbsSurface` via the public OcctKernel API.
//!
//! RED step-3 (task #4191): verifies the kernel execute arm produces a
//! `BRepKind::Face` on valid input and rejects malformed grids/knot vectors
//! with `RUST_GUARD_MARKER`-tagged `GeometryError::OperationFailed`.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_kernel_occt::RUST_GUARD_MARKER;
use reify_ir::{BRepKind, GeometryError, GeometryOp};

// --- nurbs_surface ---

/// A valid bilinear (degree-1×1, clamped) patch over four corners in mm:
/// (0,0,0), (0,10,0), (10,0,0), (10,10,5)
/// must produce a BRepKind::Face handle without error.
#[test]
fn nurbs_surface_valid_bilinear_patch_creates_face() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::NurbsSurface {
        control_points: vec![
            vec![[0.0, 0.0, 0.0], [0.0, 0.01, 0.0]],
            vec![[0.01, 0.0, 0.0], [0.01, 0.01, 0.005]],
        ],
        weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_degree: 1,
        v_degree: 1,
    });
    let handle = result.expect("valid bilinear nurbs_surface should succeed");
    assert_eq!(
        handle.repr,
        Some(BRepKind::Face),
        "nurbs_surface must produce BRepKind::Face"
    );
}

/// Weights grid with a wrong-length row (row 1 has 1 element instead of 2)
/// must return a RUST_GUARD_MARKER-tagged OperationFailed, not UB.
#[test]
fn nurbs_surface_shape_mismatch_weights_returns_guard_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::NurbsSurface {
        control_points: vec![
            vec![[0.0, 0.0, 0.0], [0.0, 0.01, 0.0]],
            vec![[0.01, 0.0, 0.0], [0.01, 0.01, 0.005]],
        ],
        // weights[1] has only 1 element instead of 2 → grid not rectangular
        weights: vec![vec![1.0, 1.0], vec![1.0]],
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_degree: 1,
        v_degree: 1,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains(RUST_GUARD_MARKER),
                "expected RUST_GUARD_MARKER in error message, got: {msg}"
            );
        }
        Ok(_) => panic!("expected error for weight shape mismatch, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// u_knots vector of wrong length (5 instead of n_u+u_degree+1 = 2+1+1 = 4)
/// must return a RUST_GUARD_MARKER-tagged OperationFailed.
#[test]
fn nurbs_surface_wrong_knot_length_returns_guard_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::NurbsSurface {
        control_points: vec![
            vec![[0.0, 0.0, 0.0], [0.0, 0.01, 0.0]],
            vec![[0.01, 0.0, 0.0], [0.01, 0.01, 0.005]],
        ],
        weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
        // u_knots should be length 4 (n_u=2, u_degree=1 → 2+1+1=4); 5 is wrong
        u_knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_degree: 1,
        v_degree: 1,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains(RUST_GUARD_MARKER),
                "expected RUST_GUARD_MARKER in error message for wrong knot length, got: {msg}"
            );
        }
        Ok(_) => panic!("expected error for wrong u_knots length, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

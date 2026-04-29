//! Integration test: shell_shape returns an error for out-of-bounds face indices.
//!
//! Verifies that the kernel rejects a Shell op whose `faces_to_remove` contains
//! an index that exceeds the actual face count of the target shape, rather than
//! silently discarding the bad index and producing a "successful" result.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryError, GeometryHandleId, GeometryOp, Value};

/// Build a kernel that contains a 10×10×10 box (6 faces) and return its handle.
fn kernel_with_box() -> (OcctKernel, GeometryHandleId) {
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

#[test]
fn shell_shape_out_of_bounds_face_index_returns_error() {
    let (mut kernel, box_id) = kernel_with_box();

    // Face index 99 is out of range for a 6-face box; index 0 is valid.
    let result = kernel.execute(&GeometryOp::Shell {
        target: box_id,
        thickness: Value::Real(1.0),
        faces_to_remove: vec![0, 99],
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("shell_shape"),
                "expected error message containing 'shell_shape', got: {msg}"
            );
            assert!(
                msg.contains("99"),
                "expected error message containing the offending index '99', got: {msg}"
            );
            assert!(
                msg.contains("out of range"),
                "expected error message containing 'out of range', got: {msg}"
            );
            assert!(
                msg.contains("has 6 faces"),
                "expected error message containing 'has 6 faces', got: {msg}"
            );
        }
        Ok(_) => panic!(
            "expected OperationFailed for out-of-bounds face index 99, got Ok"
        ),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// Verify the boundary is exclusive: index == face_count (6) must also be rejected.
///
/// The C++ guard uses `idx >= all_faces.size()`, so index 6 on a 6-face box
/// (valid indices: 0–5) is out of range. This test locks in that boundary
/// semantics and would catch an off-by-one regression that changes `>=` to `>`.
#[test]
fn shell_shape_boundary_face_index_returns_error() {
    let (mut kernel, box_id) = kernel_with_box();

    // Index 6 is exactly == face_count (6), so it is out of range.
    let result = kernel.execute(&GeometryOp::Shell {
        target: box_id,
        thickness: Value::Real(1.0),
        faces_to_remove: vec![6],
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("shell_shape"),
                "expected error message containing 'shell_shape', got: {msg}"
            );
            assert!(
                msg.contains("out of range"),
                "expected error message containing 'out of range', got: {msg}"
            );
            assert!(
                msg.contains("has 6 faces"),
                "expected error message containing 'has 6 faces', got: {msg}"
            );
        }
        Ok(_) => panic!(
            "expected OperationFailed for out-of-bounds face index 6, got Ok"
        ),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

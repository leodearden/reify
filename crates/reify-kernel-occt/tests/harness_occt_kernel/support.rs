//! Helpers shared by two or more `harness_occt_kernel` members. A leaf: it imports no
//! member.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandle};

/// Assert that a `Result` is an `Err(GeometryError::OperationFailed(msg))` where
/// `msg` contains the given `expected_substring`. Panics on `Ok` or wrong error variant.
pub(crate) fn assert_operation_fails_with(
    result: Result<GeometryHandle, GeometryError>,
    expected_substring: &str,
) {
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.to_lowercase()
                    .contains(&expected_substring.to_lowercase()),
                "expected error containing '{expected_substring}', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed containing '{expected_substring}', got Ok"),
        Err(other) => panic!(
            "expected OperationFailed containing '{expected_substring}', got {:?}",
            other
        ),
    }
}

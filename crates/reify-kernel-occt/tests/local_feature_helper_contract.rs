//! Contract test for the shared `assert_local_feature_history_well_formed`
//! helper defined in `tests/common/mod.rs`.
//!
//! Lives in its own dedicated integration-test binary so the
//! `#[should_panic]` verifying the (g) silent_drop_count assertion runs
//! exactly once — not once per binary that includes `mod common;`.
//! See review feedback on task #2853 (test_architecture blocking issue):
//! placing `#[test]` items inside the shared `common/mod.rs` causes them
//! to be compiled into every integration-test binary that pulls the
//! module in, multiplying the test run count.

#![cfg(has_occt)]

mod common;

use reify_kernel_occt::{LocalFeatureOpHistoryRecords, OcctKernelHandle};
use reify_types::GeometryHandleId;

/// Verify the helper panics with a message containing "silently drop" when
/// `silent_drop_count` is non-zero. Substring match is robust to wording
/// tweaks while still pinning the specific (g) assertion.
///
/// `GeometryHandleId(0)` is a deliberately bogus id. It is fine because the
/// (g) silent_drop_count assertion fires at the TOP of the helper, before
/// `extract_edges` is called — so the kernel is never actually queried.
#[test]
#[should_panic(expected = "silently drop")]
fn helper_panics_when_silent_drop_count_nonzero() {
    let kernel = OcctKernelHandle::spawn();
    let history = LocalFeatureOpHistoryRecords {
        silent_drop_count: 1,
        ..Default::default()
    };
    common::assert_local_feature_history_well_formed(
        &kernel,
        GeometryHandleId(0),
        &history,
        "test_op",
    );
}

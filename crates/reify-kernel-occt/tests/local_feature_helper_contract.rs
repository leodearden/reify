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

use reify_kernel_occt::{HistoryRecord, LocalFeatureOpHistoryRecords, OcctKernelHandle};
use reify_types::GeometryHandleId;

/// Verify the helper panics with a message containing "face_generated
/// parent_subshape_index" when a `face_generated` record has
/// `parent_subshape_index >= 12`.
///
/// The fixture constructs 12 records with distinct `parent_subshape_index`
/// values `{0,1,2,3,4,5,6,7,8,9,10,99}`.  The HashSet cardinality is 12 so
/// the existing cardinality check (g2) passes.  The new per-record bound < 12
/// must fire on the 99-value record.
///
/// `GeometryHandleId(0)` is a deliberately bogus id.  It is fine because the
/// new per-record loop fires before block (h)'s `extract_edges` call — same
/// trick as `helper_panics_when_silent_drop_count_nonzero`.
#[test]
#[should_panic(expected = "face_generated parent_subshape_index")]
fn helper_panics_when_face_generated_parent_subshape_index_out_of_range() {
    let kernel = OcctKernelHandle::spawn();
    let face_generated = (0u32..11)
        .chain(std::iter::once(99u32))
        .map(|psi| HistoryRecord {
            parent_index: 0,
            parent_subshape_index: psi,
            result_subshape_index: 0,
        })
        .collect();
    let history = LocalFeatureOpHistoryRecords {
        face_generated,
        ..Default::default()
    };
    common::assert_local_feature_history_well_formed(
        &kernel,
        GeometryHandleId(0),
        &history,
        "test_op",
    );
}

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

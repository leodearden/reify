//! CLI integration tests for the `AllParamsDetermined` / `AllGeometryDetermined`
//! compiler-sugar intrinsics (task-4197 α, BT4 CLI leaf).
//!
//! Uses `examples/determinacy_intrinsics.ri` which declares a local
//! `design_review` purpose and two structures:
//!   - `DeterminedBracket` — all params have concrete defaults → Satisfied
//!   - `DraftBracket`      — one param has no default            → Violated
//!
//! These tests are RED before `examples/determinacy_intrinsics.ri` exists
//! (step-11), GREEN once it is created.

mod common;

/// BT4 CLI leaf (Satisfied branch): activating `design_review` against a fully-
/// determined structure must exit 0 and report the purpose constraint as satisfied.
///
/// RED: `examples/determinacy_intrinsics.ri` does not exist yet (step-10 RED test).
#[test]
fn check_design_review_satisfied_for_determined_bracket() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "design_review=DeterminedBracket",
        &common::example_path("determinacy_intrinsics.ri"),
    ]);

    assert!(
        status.success(),
        "reify check --purpose design_review=DeterminedBracket should exit 0 \
         (all params determined).\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Purpose-injected constraint must appear in the report.
    assert!(
        stdout.contains("purpose:design_review@"),
        "stdout should contain the purpose-injected constraint id prefix \
         'purpose:design_review@', got: {stdout}"
    );
    // Summary must be the all-satisfied message.
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.', got: {stdout}"
    );
}

/// BT4 CLI leaf (Violated branch): activating `design_review` against a structure
/// with an undetermined param must exit non-zero and report the constraint as violated.
///
/// RED: `examples/determinacy_intrinsics.ri` does not exist yet (step-10 RED test).
#[test]
fn check_design_review_violated_for_draft_bracket() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "design_review=DraftBracket",
        &common::example_path("determinacy_intrinsics.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --purpose design_review=DraftBracket should exit non-zero \
         (undetermined param).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
}

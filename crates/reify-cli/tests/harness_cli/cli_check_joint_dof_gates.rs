//! End-to-end `reify check` guards for the joint DOF self-check's gradualism
//! gates (task #8740, compiler-declaration-passes B12).
//!
//! The verdict (`E_JOINT_DOF_MISMATCH`) is gated off by a kind-less DOF (an
//! unresolved type draws nothing more; a non-kind type draws one targeted
//! error) and by a body relation with a known DOF count but no rot/trans split.
//! The control fixture is the unresolved-type fixture with each `Blorp` read as
//! `Angle`; its verdicts prove the silent fixtures are not vacuous.
//!
//! The CLI surfaces diagnostic MESSAGE text, so these assertions match message
//! substrings; the exact codes are pinned in reify-compiler's
//! `harness_mechanics/joint_dof_self_check_tests.rs`.

use crate::common;

/// The phrase every joint DOF mismatch message carries.
const MISMATCH_MARKER: &str = "but the relation leaves";
const NON_KIND_MARKER: &str = "not a valid joint DOF kind";

fn lines_containing<'a>(stderr: &'a str, needle: &str) -> Vec<&'a str> {
    stderr
        .lines()
        .filter(|line| line.contains(needle))
        .collect()
}

#[test]
fn check_joint_dof_mismatch_control_draws_the_verdict() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("joint_dof_gate_mismatch_control.ri"),
    );

    assert!(
        !status.success(),
        "the control joint must fail its DOF self-check.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let mismatches = lines_containing(&stderr, MISMATCH_MARKER);
    for joint in ["`j`", "`k`"] {
        let joint_mismatches: Vec<&str> = mismatches
            .iter()
            .copied()
            .filter(|line| line.contains(joint))
            .collect();
        assert_eq!(
            joint_mismatches.len(),
            1,
            "exactly one mismatch line naming joint {joint} expected, got: {stderr}"
        );
        assert!(
            joint_mismatches[0].contains("1 rot + 1 trans"),
            "the mismatch must name the residual, got: {}",
            joint_mismatches[0]
        );
    }
    assert_eq!(
        mismatches.len(),
        2,
        "exactly one mismatch line per joint expected, got: {stderr}"
    );
}

/// Paired with the control fixture, which differs only in its resolvable types.
/// No exit-code assertion: the unresolved `Blorp` is itself silent today, which
/// is task #8148's to change.
#[test]
fn check_joint_dof_error_type_draws_no_verdict_and_no_kind_error() {
    let (_status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("joint_dof_gate_error_type.ri"),
    );

    assert!(
        lines_containing(&stderr, MISMATCH_MARKER).is_empty(),
        "an unresolved DOF type must gate the verdict off.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        lines_containing(&stderr, NON_KIND_MARKER).is_empty(),
        "an unresolved DOF type must not draw a second diagnostic.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_joint_dof_undecidable_split_draws_no_verdict() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("joint_dof_gate_tangent.ri"));

    assert!(
        status.success(),
        "a body with `tangent` must gate the verdict off and check clean.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        lines_containing(&stderr, "error:").is_empty(),
        "no error expected, got: {stderr}"
    );
    assert!(
        lines_containing(&stderr, MISMATCH_MARKER).is_empty(),
        "no mismatch expected, got: {stderr}"
    );
}

#[test]
fn check_joint_dof_non_kind_draws_one_kind_error_and_no_verdict() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("joint_dof_gate_non_kind.ri"));

    assert!(
        !status.success(),
        "a non-kind DOF must fail the check.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let errors = lines_containing(&stderr, "error:");
    assert_eq!(
        errors.len(),
        1,
        "exactly one error line expected, got: {stderr}"
    );
    assert!(
        errors[0].contains(NON_KIND_MARKER) && errors[0].contains("`foo`"),
        "the error must name the non-kind field `foo`, got: {}",
        errors[0]
    );
    assert!(
        lines_containing(&stderr, MISMATCH_MARKER).is_empty(),
        "the kind error must replace the verdict, got: {stderr}"
    );
}

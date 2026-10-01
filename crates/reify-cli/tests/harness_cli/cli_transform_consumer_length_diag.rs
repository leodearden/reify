//! End-to-end CLI tests for the Transform-consumer translation diagnostic
//! (RULING #6089).
//!
//! A `Transform`'s translation is a displacement and carries LENGTH. Its two
//! consumers that read it as SI metres — `affine_from_transform` and
//! `transform_inverse` — reject any other dimension, and the rejection must be
//! EXPLAINED: a stderr `Error` naming the builtin and `t.translation`, and a
//! non-zero `reify eval` exit. Before the ruling both fixtures below exited 0, the
//! first building a map that read each bare unit as one metre.

use crate::common;

// The needles are shared by the positive tests AND the absence checks in the
// LENGTH controls, so a reword breaks them together instead of leaving a negative
// assertion vacuously true. Each carries the `<builtin>: ` opening so it cannot
// match a bare mention of the builtin elsewhere on stderr.

/// The `affine_from_transform` translation rejection.
const AFT_NEEDLE: &str = "affine_from_transform: t.translation argument expects Length";
/// The `transform_inverse` translation rejection.
const INV_NEEDLE: &str = "transform_inverse: t.translation argument expects Length";

/// Run `reify eval` on a dimensionless-translation fixture and assert it exits
/// non-zero with `needle` and the offending `got` shape on stderr.
fn assert_dimensionless_fixture_rejected(fixture: &str, needle: &str) {
    let path = common::fixture_path(fixture);
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a non-LENGTH translation is an Error, so reify eval must exit non-zero;\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(needle),
        "stderr should carry the RULING #6089 rejection naming the builtin and \
         `t.translation`; got: {stderr}"
    );
    assert!(
        stderr.contains("got Real"),
        "stderr should name the offending shape; got: {stderr}"
    );
    // Guards the fixture's `module` decl: without it, W_MODULE_DECL_MISSING
    // ("expected `module <stem>`") re-supplies the builtin-name substring for free
    // (the trap task 6155 documented for affine_scale_dim.ri).
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "{fixture} declares its module, so no module-decl warning should appear; \
         got: {stderr}"
    );
}

/// Run `reify eval` on a LENGTH-translation control fixture and assert it exits 0,
/// prints `stdout_needle`, and carries neither rejection on stderr.
fn assert_length_fixture_accepted(fixture: &str, stdout_needle: &str) {
    let path = common::fixture_path(fixture);
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a LENGTH translation must still evaluate;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(stdout_needle),
        "stdout should print the translation in SI metres ({stdout_needle:?}); \
         got: {stdout}"
    );
    for needle in [AFT_NEEDLE, INV_NEEDLE] {
        assert!(
            !stderr.contains(needle),
            "a LENGTH translation must not be rejected; got: {stderr}"
        );
    }
}

#[test]
fn eval_affine_from_transform_dimensionless_exits_1_naming_t_translation() {
    assert_dimensionless_fixture_rejected("affine_from_transform_dimensionless.ri", AFT_NEEDLE);
}

#[test]
fn eval_transform_inverse_dimensionless_exits_1_naming_t_translation() {
    assert_dimensionless_fixture_rejected("transform_inverse_dimensionless.ri", INV_NEEDLE);
}

/// The INSEPARABLE control for the `affine_from_transform` rejection. The printed
/// form was measured against the real `target/debug/reify` binary.
#[test]
fn eval_affine_from_transform_length_exits_0_and_prints_si_metres() {
    assert_length_fixture_accepted(
        "affine_from_transform_length.ri",
        "translation=[0.005, 0, 0]",
    );
}

/// The INSEPARABLE control for the `transform_inverse` rejection: the inverse of a
/// 5 mm translation under the identity rotation is -5 mm. The printed form was
/// measured against the real `target/debug/reify` binary.
#[test]
fn eval_transform_inverse_length_exits_0_and_prints_si_metres() {
    assert_length_fixture_accepted("transform_inverse_length.ri", "vec(-0.005 m,");
}

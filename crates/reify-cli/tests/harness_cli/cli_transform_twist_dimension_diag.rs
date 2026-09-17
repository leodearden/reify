//! End-to-end CLI tests for the twist linear-dimension diagnostic (RULING #6126).
//!
//! The ruling narrows BOTH ends of the `transform_log` ↔ `transform_exp` seam to
//! `Vector3<Length>`, and requires that the rejection be *explained* rather than
//! degrading to a silent `Undef`. Before the ruling these expressions evaluated
//! successfully; after the eval gates narrow but before the `geometry::diagnose`
//! arms land, they print only the generic
//! `note: … op contract failed (OpContractViolation)`, which never names the
//! offending dimension. These tests pin the user-observable end of that contract:
//! a stderr `Error` naming both the required dimension (`Length`) and the offending
//! one, and a NON-ZERO `reify eval` exit.
//!
//! The severity is an `Error` per Leo's severity amendment (2026-08-19, via
//! esc-6080-6): a wrong dimension is a design-correctness fault, not a degradation
//! to tolerate, so it must fail the build rather than scroll past on stderr.

use crate::common;

// ── The needles, shared by the positive tests AND the over-narrowing guard ────────
//
// The guard below asserts the dimension diagnostic is ABSENT. A negative assertion
// coupled to prose that nothing else pins is the classic silently-vacuous test: reword
// the message and the positive tests (asserting on other tokens) still pass, while the
// guard becomes vacuously true and stops guarding, with nothing going red. Routing all
// three tests through these consts means a reword breaks them TOGETHER.
//
// `RULING_TAG` is the stable half — the prose may be rewritten freely, but the ruling
// citation is the message's load-bearing identifier. The two prefixes carry the
// trailing `:` so they match the diagnostic's own `"<builtin>: …"` opening and not a
// bare mention of the builtin name elsewhere on stderr.

/// The ruling citation every RULING #6126 dimension diagnostic carries.
const RULING_TAG: &str = "RULING #6126";
/// The opening of the `transform_log` dimension diagnostic.
const LOG_DIAG_PREFIX: &str = "transform_log:";
/// The opening of the `transform_exp` dimension diagnostic.
const EXP_DIAG_PREFIX: &str = "transform_exp:";

/// `reify eval` on a dimensionless Transform translation emits the
/// Vector3<Length>-requirement `Error` on stderr via the post-Undef geometry
/// diagnose hook, and exits NON-ZERO.
#[test]
fn eval_transform_log_dimensionless_errors_length_required() {
    let path = common::fixture_path("transform_log_dimensionless.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a dimension rejection is an Error, so reify eval must exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(LOG_DIAG_PREFIX),
        "stderr should name the builtin that rejected; got: {stderr}"
    );
    assert!(
        stderr.contains(RULING_TAG),
        "stderr should cite the ruling — the token the over-narrowing guard keys its \
         ABSENCE off; got: {stderr}"
    );
    assert!(
        stderr.contains("Length"),
        "stderr should name the REQUIRED dimension; got: {stderr}"
    );
    assert!(
        stderr.contains("dimensionless"),
        "stderr should name the OFFENDING dimension; got: {stderr}"
    );
}

/// `reify eval` on a dimensionless twist `linear` half emits the
/// Vector3<Length>-requirement `Error` on stderr and exits NON-ZERO — the mirror
/// of the `transform_log` case, so both ends of the seam explain their rejection
/// AND fail with the same exit code.
#[test]
fn eval_transform_exp_dimensionless_errors_length_required() {
    let path = common::fixture_path("transform_exp_dimensionless.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a dimension rejection is an Error, so reify eval must exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(EXP_DIAG_PREFIX),
        "stderr should name the builtin that rejected; got: {stderr}"
    );
    assert!(
        stderr.contains(RULING_TAG),
        "stderr should cite the ruling — the token the over-narrowing guard keys its \
         ABSENCE off; got: {stderr}"
    );
    assert!(
        stderr.contains("Length"),
        "stderr should name the REQUIRED dimension; got: {stderr}"
    );
    assert!(
        stderr.contains("dimensionless"),
        "stderr should name the OFFENDING dimension; got: {stderr}"
    );
}

/// The over-narrowing guard: a Length transform round-trips through
/// `transform_log` → `transform_exp` and a pure-rotation (identity) transform is
/// still accepted, with NO dimension Warning on stderr.
///
/// `transform3_identity` builds `Value::length(0.0)` translations, so identity and
/// pure-rotation transforms carry LENGTH zeros and survive the narrowing.
#[test]
fn eval_transform_twist_length_round_trip_emits_no_dimension_warning() {
    let path = common::fixture_path("transform_twist_length_round_trip.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "the Length round-trip must stay green;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // 1mm → 0.001 m: the linear half survives the seam carrying its metre value.
    assert!(
        stdout.contains("0.001 m"),
        "stdout should show the metre-valued linear component;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Keyed off the SAME needles the two positive tests require, so a reword cannot
    // make this guard vacuously true while they keep passing.
    assert!(
        !stderr.contains(RULING_TAG),
        "a Length twist must NOT provoke the dimension warning (over-narrowing guard); got: {stderr}"
    );
    for prefix in [LOG_DIAG_PREFIX, EXP_DIAG_PREFIX] {
        assert!(
            !stderr.contains(prefix),
            "neither end of the seam may warn on a Length twist (expected no {prefix:?}); got: {stderr}"
        );
    }
}

//! End-to-end CLI tests for the twist linear-dimension diagnostic (RULING #6126).
//!
//! The ruling narrows BOTH ends of the `transform_log` ↔ `transform_exp` seam to
//! `Vector3<Length>`, and requires that the rejection be *explained* rather than
//! degrading to a silent `Undef`. Before the ruling these expressions evaluated
//! successfully; after the eval gates narrow but before the `geometry::diagnose`
//! arms land, they print only the generic
//! `note: … op contract failed (OpContractViolation)`, which never names the
//! offending dimension. These tests pin the user-observable end of that contract:
//! a stderr Warning naming both the required dimension (`Length`) and the offending
//! one, with `reify eval` still exiting 0.

use crate::common;

/// `reify eval` on a dimensionless Transform translation emits the
/// Vector3<Length>-requirement Warning on stderr via the post-Undef geometry
/// diagnose hook, and still exits 0.
#[test]
fn eval_transform_log_dimensionless_warns_length_required() {
    let path = common::fixture_path("transform_log_dimensionless.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a dimension rejection is a Warning (not an Error), so reify eval should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("transform_log"),
        "stderr should name the builtin that rejected; got: {stderr}"
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
/// Vector3<Length>-requirement Warning on stderr, and still exits 0 — the mirror
/// of the `transform_log` case, so both ends of the seam explain their rejection.
#[test]
fn eval_transform_exp_dimensionless_warns_length_required() {
    let path = common::fixture_path("transform_exp_dimensionless.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a dimension rejection is a Warning (not an Error), so reify eval should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("transform_exp"),
        "stderr should name the builtin that rejected; got: {stderr}"
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
    assert!(
        !stderr.contains("must be Vector3<Length>"),
        "a Length twist must NOT provoke the dimension warning (over-narrowing guard); got: {stderr}"
    );
}

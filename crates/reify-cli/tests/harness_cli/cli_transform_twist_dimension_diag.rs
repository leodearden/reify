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

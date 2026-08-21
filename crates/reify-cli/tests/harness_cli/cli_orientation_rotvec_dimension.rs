//! End-to-end CLI tests for the #6080 rotation-vector ANGLE ruling.
//!
//! Rotation vectors are `axis * angle`, so they carry ANGLE (slot 7, `rad`) —
//! not `Dimensionless`. These tests pin the *user-observable* half of that
//! ruling through the `reify eval` / `reify check` channel, which is the only
//! place a unit tag actually reaches a user.

use crate::common;

/// `orient_log` prints ANGLE-dimensioned components (acceptance clause 1).
///
/// RED before #6080's emission change: `vec(0, 0, 1.5707963267948963)` —
/// untagged. GREEN: every component carries a `rad` tag.
///
/// The digits are one ULP away from the `vec3(0deg,0deg,90deg)` literal path
/// (`...948966`); this fixture goes through `orient_axis_angle` → `orient_log`
/// and yields `...948963`.
#[test]
fn eval_orient_log_prints_angle_dimensioned_rotation_vector() {
    let path = common::fixture_path("orient_log_angle_probe.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval orient_log_angle_probe.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("RotVecProbe.lv = vec(0 rad, 0 rad, 1.5707963267948963 rad)"),
        "stdout should show an ANGLE-dimensioned rotation vector; got: {stdout}"
    );
}

/// The probe fixture stays `reify check`-clean, before AND after #6080.
///
/// This task rules on the EVAL-time dimension only; the static signature of
/// `orient_log` is the unchanged arg0-clone fallback, so no check-time
/// diagnostic may appear.
#[test]
fn check_orient_log_angle_probe_stays_clean() {
    let path = common::fixture_path("orient_log_angle_probe.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check orient_log_angle_probe.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
}

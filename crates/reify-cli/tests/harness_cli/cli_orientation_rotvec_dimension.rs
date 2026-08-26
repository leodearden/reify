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

/// `orient_exp(orient_log(q))` still recovers `q` (acceptance clause 2).
///
/// The coupled-change guard: this identity holds only if orient_log's emission
/// and orient_exp's gate agree on ANGLE. It is RED in between — orient_log
/// hands out an ANGLE vector that the old DIMENSIONLESS gate rejects, so `rt`
/// evaluates to `undef`.
#[test]
fn eval_orient_exp_of_orient_log_round_trips() {
    let path = common::fixture_path("orient_rotvec_round_trip.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval orient_rotvec_round_trip.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("RotVecRoundTrip.rt = [0.7071067811865476, 0, 0, 0.7071067811865475]q"),
        "stdout should show the recovered 90°z quaternion; got: {stdout}"
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

// ── Wrong-dimension diagnostics (acceptance clause 3) ────────────────────────
//
// Narrowing `orient_exp` / `Twist.angular` to ANGLE is a BREAKING change to a
// published stdlib signature, so the rejected spellings must stop failing
// silently. These tests prove the classifier is actually WIRED into
// `emit_undef_builtin_diagnostics` — a unit test on `diagnose()` alone would
// pass with the hook unreferenced.
//
// The diagnostic is unspanned: the hook signature is
// `fn(&str, &[Value]) -> Option<Diagnostic>` and `CompiledExpr` carries no
// span, so attribution lives in the message text (builtin name + offending
// dimension). See the plan's design decision for the full rationale.

/// A DIMENSIONLESS rotation vector — accepted before #6080 — now errors.
///
/// This is the migration case: the spelling that silently changed meaning.
/// RED before the hook: `q = undef`, a `note:` line, and exit 0.
#[test]
fn eval_orient_exp_dimensionless_errors_and_exits_nonzero() {
    let path = common::fixture_path("orient_exp_dimensionless.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "reify eval orient_exp_dimensionless.ri should exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_RotationVectorDimension"),
        "stderr should contain 'E_RotationVectorDimension'; got: {stderr}"
    );
    assert!(
        stderr.contains("dimensionless"),
        "stderr should name the offending dimension ('dimensionless'); got: {stderr}"
    );
}

/// A LENGTH rotation vector was never accepted, but used to fail silently.
#[test]
fn eval_orient_exp_length_errors_and_exits_nonzero() {
    let path = common::fixture_path("orient_exp_length.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "reify eval orient_exp_length.ri should exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_RotationVectorDimension"),
        "stderr should contain 'E_RotationVectorDimension'; got: {stderr}"
    );
    // Anchored on the full `got <dim>.` clause, not a bare `" m"` substring:
    // the latter matches any space-then-m anywhere on stderr (another
    // diagnostic line, or a dimension that merely STARTS with `m`, e.g. `m^2`
    // or `m·s^-1`), so it would not actually discriminate LENGTH. This is the
    // same message shape the sibling `dimensionless` assertions rely on.
    assert!(
        stderr.contains("got m."),
        "stderr should name the offending dimension as LENGTH ('got m.'); got: {stderr}"
    );
}

/// The `Twist.angular` half of the same ruling, through the same channel.
#[test]
fn eval_transform_exp_dimensionless_angular_errors_and_exits_nonzero() {
    let path = common::fixture_path("transform_exp_dimensionless_angular.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "reify eval transform_exp_dimensionless_angular.ri should exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_RotationVectorDimension"),
        "stderr should contain 'E_RotationVectorDimension'; got: {stderr}"
    );
    assert!(
        stderr.contains("angular"),
        "stderr should name the offending Twist field ('angular'); got: {stderr}"
    );
    assert!(
        stderr.contains("dimensionless"),
        "stderr should name the offending dimension ('dimensionless'); got: {stderr}"
    );
}

// ── Migration-advice guards (#6080 amendment) ────────────────────────────────
//
// The `E_RotationVectorDimension` message tells the user how to fix the call.
// These two tests pin BOTH halves of that advice being honest: the spelling it
// recommends works, and the spelling it must NOT recommend (a lone dimensioned
// component, leaving a half-migrated mixed-dimension vec3) still fails the old
// silent way — which is exactly why the advice names a whole vector.

/// The fix the diagnostic recommends is executable, not merely plausible.
///
/// The fixture is the diagnostic's own advice verbatim —
/// `vec3(0rad, 0rad, 1.5708rad)` and `vec3(0deg, 0deg, 90deg)`. A user who
/// follows the printed message must land on a clean, exit-0 evaluation with two
/// real quaternions, not another `undef`.
#[test]
fn eval_recommended_migration_spelling_succeeds() {
    let path = common::fixture_path("orient_exp_migrated_spelling.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "the spelling the diagnostic recommends must exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_RotationVectorDimension"),
        "the recommended spelling must not trip the dimension diagnostic; got: {stderr}"
    );
    assert!(
        !stdout.contains("undef"),
        "the recommended spelling must not evaluate to undef; got: {stdout}"
    );
    // The `90deg` half is exact, so pin the recovered quaternion outright —
    // same rendering the round-trip guard above relies on.
    assert!(
        stdout.contains(
            "OrientExpMigratedSpelling.p = [0.7071067811865476, 0, 0, 0.7071067811865475]q"
        ),
        "the `90deg` spelling should recover the 90°z quaternion; got: {stdout}"
    );
}

/// KNOWN GAP: a HALF-migrated rotation vector is the one case the loud
/// migration mechanism does not cover.
///
/// `vec3(0, 0, 1.5708rad)` mixes DIMENSIONLESS and ANGLE components, so
/// `construct::eval_vec` collapses it to `undef` at its own construction site,
/// before `orient_exp` runs. The classifier inspects `orient_exp`'s argument,
/// so it never sees the offending vector: no `E_RotationVectorDimension`, and
/// exit 0.
///
/// Pinned so the gap is visible and measured rather than assumed. Diagnosing a
/// mixed-dimension container belongs to `vec3`'s construction site, not to the
/// orientation family, and is out of #6080's scope — WHEN that lands, update
/// this test to assert the new loud behaviour rather than deleting it.
#[test]
fn eval_partially_migrated_rotation_vector_stays_silently_undef() {
    let path = common::fixture_path("orient_exp_partial_migration.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a mixed-dimension vec3 collapses upstream, so eval still exits 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OrientExpPartialMigration.q = undef"),
        "the half-migrated call should evaluate to undef; got: {stdout}"
    );
    assert!(
        !stderr.contains("E_RotationVectorDimension"),
        "the classifier cannot fire on an already-collapsed argument; got: {stderr}"
    );
}

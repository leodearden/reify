//! CLI signal for direction-typing ζ-a #9640 (PRD
//! `docs/prds/v0_6/direction-typing.md` C3 / B6): a joint or flexure axis is a
//! `Direction`.
//!
//! - `reify check` rejects a `Vector3` or `String` axis at a joint builtin, a
//!   PRB flexure ctor and a `std.kinematic` joint struct ctor, naming
//!   Direction; every `Vector3` form ends with the `dir(x, y, z)` fix hint.
//! - `reify eval` builds `prismatic(dir(0, 1, 0), …)`, and `joint_axis` prints
//!   the stored `dir(0, 1, 0)`.
//! - The migrated CoreXY fixture's non-basis axes `dir(1, ±1, 0)` keep the
//!   carriage where the `vec3` axes put it, within 1e-12: the Direction is
//!   normalised once, by the same IEEE expression the old at-use normaliser
//!   used, so at worst a few ulps move.
//!
//! The CLI prints no diagnostic code, so these tests pin the message text; the
//! `ArgTypeMismatch` code itself is pinned by reify-compiler's
//! `harness_type_checking/joint_flexure_direction_slot_tests.rs`.

use crate::common;

const COREXY_FIXTURE: &str = "tests/prd-gate/fixtures/s6_corexy_tie_orientation.ri";
const COREXY_CARRIAGE_A: [f64; 2] = [0.07071067811865475, 0.21213203435595915];
const COREXY_TOLERANCE: f64 = 1e-12;

fn error_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .collect()
}

#[test]
fn check_rejects_a_non_direction_joint_or_flexure_axis() {
    let path = common::fixture_path("joint_direction_axis_rejections.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "a non-Direction axis must fail check\nstdout: {stdout}\nstderr: {stderr}"
    );
    let expected = [
        "error: prismatic: axis argument expects Direction, got Vector3<Int>; write dir(x, y, z)",
        "error: revolute: axis argument expects Direction, got String",
        "error: prb_cantilever_beam: axis argument expects Direction, got Vector3<Int>; \
         write dir(x, y, z)",
        "error: argument 'axis' has type 'Vector3<Int>' but param 'axis' requires type \
         'Direction'; write dir(x, y, z)",
        "error: argument 'axis' has type 'String' but param 'axis' requires type 'Direction'",
    ];
    let errors = error_lines(&stderr);
    for line in expected {
        assert!(
            errors.contains(&line),
            "check must report the exact line {line:?}\nstderr: {stderr}"
        );
    }
    assert_eq!(
        errors.len(),
        expected.len(),
        "each of the fixture's structures draws exactly one error\nstderr: {stderr}"
    );
}

#[test]
fn eval_builds_a_direction_axis_joint_and_joint_axis_prints_it() {
    let path = common::fixture_path("joint_direction_axis_eval.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a Direction axis must evaluate cleanly\nstdout: {stdout}\nstderr: {stderr}"
    );
    let expected = "JointDirectionAxis.a = dir(0, 1, 0)";
    assert!(
        stdout.lines().any(|line| line == expected),
        "stdout must carry the exact line {expected:?}\nstdout: {stdout}"
    );
}

fn carriage_a_values(stdout: &str) -> Vec<f64> {
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("CoreXY.carriage_a = "))
        .unwrap_or_else(|| panic!("stdout must carry CoreXY.carriage_a\nstdout: {stdout}"));
    line.trim_matches(['[', ']'])
        .split(',')
        .map(|n| {
            n.trim()
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("carriage_a component {n:?} must parse: {e}"))
        })
        .collect()
}

#[test]
fn corexy_with_direction_axes_keeps_the_carriage_within_1e_12() {
    let path = common::workspace_root().join(COREXY_FIXTURE);
    let (status, stdout, stderr) = common::run_subcommand("eval", &path.to_string_lossy());

    assert!(
        status.success(),
        "reify eval should exit 0 for {COREXY_FIXTURE}\nstdout: {stdout}\nstderr: {stderr}"
    );
    let values = carriage_a_values(&stdout);
    assert_eq!(
        values.len(),
        COREXY_CARRIAGE_A.len(),
        "carriage_a: {values:?}"
    );
    for (got, want) in values.iter().zip(COREXY_CARRIAGE_A) {
        assert!(
            (got - want).abs() <= COREXY_TOLERANCE,
            "carriage_a component {got} must match {want} within {COREXY_TOLERANCE}"
        );
    }
}

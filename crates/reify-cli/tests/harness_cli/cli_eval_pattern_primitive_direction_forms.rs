//! CLI signal for direction-typing ι-c #9648 (PRD
//! `docs/prds/v0_6/direction-typing.md` B12″ / B13): `half_space`, `arc` and
//! the linear and circular patterns take typed operands.
//!
//! - `reify eval` builds the Direction-form twins of the retired scalar forms
//!   and prints the values the scalar forms printed, measured on base
//!   23763e4b7e before the scalar forms were retired.
//! - `reify check` rejects each retired scalar call with an arity error that
//!   names the new form.
//!
//! The emitted IR is pinned by reify-eval's
//! `harness_geometry/direction_pattern_primitive_forms_e2e.rs`.

use crate::common;

/// Each line is the one the retired scalar form printed:
/// - `half_space(0mm, 0mm, 0mm, 0, 0, 1)` clipped by `box(20mm, 20mm, 20mm)`;
/// - the `length` of `arc(0mm, 0mm, 0mm, 5mm, 0deg, 90deg, 0, 0, 1)`;
/// - the `volume` of `linear_pattern(box(2mm, 2mm, 2mm), 1, 0, 0, 3, 5mm)`;
/// - the `volume` of
///   `linear_pattern_2d(box(2mm, 2mm, 2mm), 1, 0, 0, 2, 5mm, 0, 1, 0, 2, 5mm)`.
#[test]
fn eval_prints_the_values_the_retired_scalar_forms_printed() {
    let path = common::fixture_path("pattern_primitive_direction_forms_eval.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "the Direction forms must evaluate cleanly\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "HalfSpaceClip.v = 0.000004 m^3",
        "QuarterArc.len = 0.007853981633974483 m",
        "LinearRow.v = 0.000000023999999999999993 m^3",
        "LinearGrid.v = 0.00000003199999999999999 m^3",
    ] {
        assert!(
            stdout.lines().any(|line| line == expected),
            "stdout must carry the exact line {expected:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn check_rejects_the_retired_scalar_arities_naming_the_direction_form() {
    let path = common::fixture_path("pattern_primitive_direction_forms_rejections.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert_eq!(
        status.code(),
        Some(1),
        "the retired scalar arities must fail check\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "half_space() expects 2 arguments: half_space(point3(x, y, z), dir(x, y, z)), got 6",
        "arc() expects 5 arguments: \
         arc(point3(x, y, z), radius, start_angle, end_angle, dir(x, y, z)), got 9",
        "linear_pattern() expects 4 arguments: \
         linear_pattern(geo, dir(x, y, z), count, spacing), got 6",
        "linear_pattern_2d() expects 7 arguments: \
         linear_pattern_2d(geo, dir(x, y, z), count1, spacing1, \
         dir(x, y, z), count2, spacing2), got 11",
    ] {
        assert!(
            stderr
                .lines()
                .any(|line| line.starts_with("error:") && line.contains(expected)),
            "check must print an error: line containing {expected:?}\nstderr: {stderr}"
        );
    }
}

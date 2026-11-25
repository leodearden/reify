//! CLI signal for direction-typing ι-b #9647 (PRD
//! `docs/prds/v0_6/direction-typing.md` B12′ / B13): the sweeps take a
//! `Direction`.
//!
//! - `reify check` rejects each retired scalar arity with an arity error that
//!   names the Direction form, and rejects a bare revolve origin at build,
//!   naming the `ox` component.
//! - `reify eval` builds the Direction-form twins of the retired scalar forms
//!   and prints the volumes the scalar forms printed, measured on the tree
//!   before the scalar forms were retired.
//!
//! The arity errors' codes and the lowered op layouts are pinned by
//! reify-compiler's `harness_type_checking/sweep_direction_slot_tests.rs`; the
//! emitted IR by reify-eval's `harness_geometry/sweep_direction_forms_e2e.rs`.

use crate::common;

fn error_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .collect()
}

#[test]
fn check_rejects_the_retired_scalar_sweep_arities_naming_the_direction_form() {
    let path = common::fixture_path("sweep_direction_forms_rejections.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "the retired scalar arities must fail check\nstdout: {stdout}\nstderr: {stderr}"
    );
    let errors = error_lines(&stderr);
    for (prefix, got) in [
        ("error: revolve() expects", 8),
        ("error: revolve_full() expects", 7),
        ("error: extrude_infinite() expects", 5),
    ] {
        let line = errors
            .iter()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("check must report {prefix:?}\nstderr: {stderr}"));
        assert!(
            line.ends_with(&format!(", got {got}")) && line.contains("dir(x, y, z)"),
            "the arity error must name the Direction form: {line:?}"
        );
        if prefix.contains("extrude_infinite") {
            assert!(
                line.contains("extrude_infinite_symmetric"),
                "the retired \"both\" sense maps to extrude_infinite_symmetric: {line:?}"
            );
        }
    }
}

#[test]
fn check_rejects_a_bare_revolve_origin_naming_the_component() {
    let path = common::fixture_path("sweep_direction_forms_bare_origin.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "a bare revolve origin must fail check\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        error_lines(&stderr).iter().any(|line| line.contains("ox")
            && line.contains("Length")
            && line.contains("pass a dimensioned length such as `5mm`")),
        "check must print the `ox` Length rejection\nstderr: {stderr}"
    );
}

/// Each volume is the string the retired scalar form printed, measured on base
/// 3876a555ba before the scalar forms were retired:
/// `revolve(rectangle(20mm, 10mm), -10mm, 0mm, 0mm, 0.0, 1.0, 0.0, 1.5707963267948966rad)`,
/// `revolve_full(rectangle(20mm, 10mm), -10mm, 0mm, 0mm, 0.0, 1.0, 0.0)`, and
/// `extrude_infinite(circle(5mm), 0, 0, 1, "positive" | "both")` clipped by
/// `box(20mm, 20mm, 10mm)`.
#[test]
fn eval_prints_the_volumes_the_retired_scalar_forms_printed() {
    let path = common::fixture_path("sweep_direction_forms_eval.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "the Direction forms must evaluate cleanly\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "QuarterRevolve.v = 0.0000031415926535897933 m^3",
        "QuarterRevolveAboutAxis.v = 0.0000031415926535897933 m^3",
        "FullRevolve.v = 0.000012566370614359172 m^3",
        "PositiveClip.v = 0.0000003926990816987241 m^3",
        "SymmetricClip.v = 0.0000007853981633974481 m^3",
    ] {
        assert!(
            stdout.lines().any(|line| line == expected),
            "stdout must carry the exact line {expected:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

/// The examples that spelled a retired scalar sweep check clean once migrated
/// to the Direction forms.
#[test]
fn check_accepts_the_migrated_sweep_examples() {
    for example in [
        "extrude_infinite.ri",
        "sweep_degenerate.ri",
        "geometric_relations/feature_datum_axis.ri",
    ] {
        let path = common::example_path(example);
        let (status, stdout, stderr) = common::run_subcommand("check", &path);
        assert!(
            status.success() && error_lines(&stderr).is_empty(),
            "examples/{example} must check clean\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

/// The B8 example still recovers the revolution axis from the Direction-form
/// revolve.
#[test]
fn eval_of_the_feature_datum_axis_example_recovers_the_revolution_axis() {
    let path = common::example_path("geometric_relations/feature_datum_axis.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "feature_datum_axis.ri must evaluate cleanly\nstdout: {stdout}\nstderr: {stderr}"
    );
    let expected = "Cyl.axis_dir = dir(0, 1, 0)";
    assert!(
        stdout.lines().any(|line| line == expected),
        "stdout must carry the exact line {expected:?}\nstdout: {stdout}"
    );
}

/// ε′ #10154's realized-world fixtures no longer spell the retired 8-argument
/// revolve. Only that is asserted: their own resolution behaviour is not this
/// leaf's to pin.
#[test]
fn check_of_the_realized_world_fixtures_reports_no_revolve_arity_error() {
    for fixture in [
        "realized_world_datum_axis_inline.ri",
        "realized_world_datum_axis_let.ri",
        "realized_world_datum_user_fn.ri",
        "realized_world_guarded_branch.ri",
        "realized_world_override_instance_datum.ri",
        "realized_world_override_instance_datum_let.ri",
    ] {
        let path = common::workspace_root()
            .join("tests/prd-gate/fixtures")
            .join(fixture);
        let (_, _, stderr) = common::run_subcommand("check", &path.to_string_lossy());
        assert!(
            !stderr.contains("revolve() expects"),
            "{fixture} must not spell a retired revolve arity\nstderr: {stderr}"
        );
    }
}

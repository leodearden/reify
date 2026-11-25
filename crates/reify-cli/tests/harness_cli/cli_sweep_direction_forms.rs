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
//! emitted IR by reify-eval's `harness_geometry/sweep_direction_forms_e2e.rs`
//! (revolve) and `extrude_infinite_e2e.rs` (extrude_infinite).

use crate::common;
use std::path::Path;

fn error_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .collect()
}

/// `reify check` prints one of these summaries on stdout only once it has read,
/// parsed, compiled and evaluated the file; a read, parse or compile failure
/// prints none of them.
fn reached_the_constraint_verdict(stdout: &str) -> bool {
    stdout.lines().any(|line| {
        line == "All constraints satisfied."
            || line == "Some constraints violated."
            || line.starts_with("No constraints violated (")
    })
}

/// `reify check`'s stderr for a corpus file another task owns and may move, so
/// that an assertion that something is ABSENT from it cannot pass vacuously:
/// the file must exist, and check must compile it through to the constraint
/// verdict.
fn stderr_of_a_compiled_check(path: &Path) -> String {
    assert!(
        path.exists(),
        "{} is missing; if it was renamed or relocated, update this call site",
        path.display()
    );
    let (_, stdout, stderr) = common::run_subcommand("check", &path.to_string_lossy());
    assert!(
        reached_the_constraint_verdict(&stdout),
        "check of {} must compile it through to the constraint verdict\n\
         stdout: {stdout}\nstderr: {stderr}",
        path.display()
    );
    stderr
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

/// `sweep_degenerate.ri` is a parse/compile fixture: its profiles are Length
/// params, not geometry, so every structure's build fails whatever the sweep
/// spelling. Only the absence of a `revolve_full` error is this leaf's to pin.
#[test]
fn check_of_sweep_degenerate_reports_no_revolve_full_error() {
    let stderr =
        stderr_of_a_compiled_check(Path::new(&common::example_path("sweep_degenerate.ri")));
    assert!(
        !error_lines(&stderr)
            .iter()
            .any(|line| line.contains("revolve_full")),
        "examples/sweep_degenerate.ri must spell revolve_full's Direction form\nstderr: {stderr}"
    );
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
        let stderr = stderr_of_a_compiled_check(
            &common::workspace_root()
                .join("tests/prd-gate/fixtures")
                .join(fixture),
        );
        assert!(
            !stderr.contains("revolve() expects"),
            "{fixture} must not spell a retired revolve arity\nstderr: {stderr}"
        );
    }
}

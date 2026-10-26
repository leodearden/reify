//! CLI integration gates for `reify check` and `reify eval` on the committed
//! datum-projection examples (task 4382 β / PRD docs/prds/v0_6/geometric-relations.md §9 β).
//!
//! `examples/datum_projections.ri` exercises only the VALID datum projections
//! (`axis.dir`/`.origin`, `plane.normal`/`.origin`,
//! `frame.x`/`.y`/`.z`/`.origin`/`.xy_plane`, and `direction` component
//! access). `reify check` must accept the file cleanly — exit 0 with zero
//! `error:` diagnostics. This pins the user-observable β signal end-to-end:
//! `resolve_type_name` datum names + first-class `Direction` + datum-projection
//! member-access type-checking.
//!
//! RED (step-13): the example file does not exist yet, so `reify check` fails;
//! step-14 creates it.

use crate::common;

#[test]
fn check_datum_projections_example_is_clean() {
    let path = common::example_path("datum_projections.ri");
    let (status, stdout, stderr) = common::run_with_args(&["check", &path]);

    assert!(
        status.success(),
        "reify check should exit 0 on the valid datum-projection example.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("error:") && !stderr.contains("error:"),
        "reify check should emit no 'error:' diagnostics on the valid example.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// `reify eval` prints a value for every frame projection in
/// `examples/first_class_frame_datums.ri` (task #6100): let-bound frames, a
/// frame built from let-bound `self` directions, and `self.frame`, none of
/// which may print `undef`. The exact numbers are pinned at the engine level
/// (reify-eval `harness_engine::first_class_frame_datum_eval`); this row pins
/// that they reach the CLI.
#[test]
fn eval_first_class_frame_datums_example_prints_every_projection() {
    let path = common::example_path("first_class_frame_datums.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 on the first-class frame-datum example.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("= undef"),
        "no frame projection may evaluate to undef.\nstdout: {stdout}\nstderr: {stderr}"
    );
    for member in [
        "f_o", "f_x", "f_z", "f_xy", "f_yz", "f_zx", "r_x", "r_y", "r_z", "r_xy", "r_yz", "r_zx",
        "s_o", "s_yz", "s_zx", "sf_yz", "sf_zx",
    ] {
        assert!(
            stdout.contains(&format!("FirstClassFrameDatums.{member} = ")),
            "stdout should print FirstClassFrameDatums.{member}.\nstdout: {stdout}"
        );
    }
}

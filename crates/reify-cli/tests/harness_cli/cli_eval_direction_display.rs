//! End-to-end CLI test for direction-typing C2: `reify eval` prints a
//! `Value::Direction` in its round-trip spelling `dir(x, y, z)`.
//!
//! The three rows cover the three projection routes that yield a Direction:
//! an axis's `.dir`, a plane's `.normal`, and a frame's `.y` (which reads the
//! frame basis through the shared `frame_basis` helper).

use crate::common;

#[test]
fn eval_prints_projected_directions_in_dir_form() {
    let path = common::fixture_path("direction_display.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "direction_display.ri should evaluate cleanly;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for row in [
        "DirectionDisplay.ax = dir(1, 0, 0)",
        "DirectionDisplay.pn = dir(0, 0, 1)",
        "DirectionDisplay.fy = dir(0, 1, 0)",
    ] {
        assert!(
            stdout.contains(row),
            "stdout should carry `{row}`;\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    assert!(
        !stdout.contains("direction("),
        "the pre-C2 `direction(..)` spelling must not appear;\nstdout: {stdout}"
    );
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "direction_display.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// A kernel-minted datum (`cylinder(..).axis`, `rectangle(..).plane`) carries a
/// `Value::Direction`; its let-bound `.dir` / `.normal` prints in `dir(..)` form.
///
/// Exit status is asserted unconditionally; the printed rows only when the OCCT
/// kernel is compiled in, since without it the datums never realise.
#[test]
fn eval_prints_kernel_minted_datum_projections_in_dir_form() {
    let path = common::fixture_path("direction_display_kernel_datum.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "direction_display_kernel_datum.ri should evaluate cleanly;\nstdout: {stdout}\nstderr: {stderr}"
    );
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping kernel-datum direction rows: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }
    for row in [
        "KernelDatumDirections.d = dir(0, 0, 1)",
        "KernelDatumDirections.n = dir(0, 0, 1)",
    ] {
        assert!(
            stdout.contains(row),
            "stdout should carry `{row}`;\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

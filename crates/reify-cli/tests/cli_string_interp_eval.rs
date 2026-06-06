//! End-to-end CLI tests for string interpolation eval integration (task ζ).
//!
//! Drives `reify eval examples/interpolation.ri` and asserts that interpolated
//! String cells are printed with correct values — exercising the full pipeline:
//! grammar (task α 3967) → __interp_render builtin (task β 3964) →
//! render-then-concat lowering (task γ 3968) → cmd_eval display (unmodified).
//!
//! Step-1 (RED): examples/interpolation.ri does not exist yet, so eval exits
//! non-zero.  Step-2 creates the example (GREEN for these two tests).
//! Step-3 (RED): adds escape and undef tests that fail until step-4 extends
//! the example with the corresponding cells.

mod common;

/// PRD §1 anchor: `"thickness is {t}, doubled is {2 * t}"` with `t = 5mm`
/// must render to `"thickness is 5 mm, doubled is 10 mm"` — this pins the
/// format_display engineering-unit rendering path (5 mm / 10 mm), NOT
/// Display's SI output (0.005 m).
///
/// Also asserts the arithmetic checkpoint: `"x={1+1}"` → `"x=2"`.
///
/// Benign compiler warnings (e.g. missing module declaration) may appear on
/// stderr — we do NOT assert stderr is empty (per cli_affine_eval.rs precedent).
#[test]
fn eval_interpolation_renders_label_and_arith() {
    let path = common::example_path("interpolation.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval interpolation.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // PRD §1 anchor: engineering-unit rendering (5 mm, 10 mm — NOT SI "0.005 m").
    assert!(
        stdout.contains(r#"Demo.label = "thickness is 5 mm, doubled is 10 mm""#),
        "stdout should contain the label anchor line;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Arithmetic checkpoint: 1+1 → 2.
    assert!(
        stdout.contains(r#"Demo.arith = "x=2""#),
        "stdout should contain the arith checkpoint;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

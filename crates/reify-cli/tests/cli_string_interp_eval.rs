//! End-to-end CLI tests for string interpolation eval integration (task ζ).
//!
//! Drives `reify eval examples/interpolation.ri` and asserts that interpolated
//! String cells are printed with correct values — exercising the full pipeline:
//! grammar (task α 3967) → __interp_render builtin (task β 3964) →
//! render-then-concat lowering (task γ 3968) → cmd_eval display (unmodified).
//!
//! All four PRD §8 checkpoints are asserted in a single eval run to avoid
//! spawning the subprocess once per checkpoint for identical stdout.

mod common;

/// PRD §8 end-to-end golden: all four task-ζ checkpoints in one eval run.
///
/// 1. §1 anchor (`label`): `"thickness is {t}, doubled is {2 * t}"` with
///    `t = 5mm` → `"thickness is 5 mm, doubled is 10 mm"`.  Pins
///    format_display engineering-unit rendering (5 mm / 10 mm, NOT SI "0.005 m").
///
/// 2. Arithmetic (`arith`): `"x={1+1}"` → `"x=2"`.
///
/// 3. Escape (`escaped`): `"{{braces}}"` → `"{braces}"`.  The `{{`/`}}`
///    literal-brace escape collapses to single braces with no interpolation node.
///
/// 4. Undef-totality (`undef_demo`): `"gap is {gap}"` where `gap : Length = auto`
///    (no optimization objective) stays `Value::Undef` at eval time.
///    __interp_render maps Undef → literal `"undef"`, so the cell becomes a
///    determinate `Value::String("gap is undef")` — the QUOTED form in stdout
///    distinguishes render-totality (PRD §6.3) from String+Undef poisoning,
///    which would print a bare unquoted `undef`.
///
/// Benign compiler warnings (e.g. missing module declaration) may appear on
/// stderr — we do NOT assert stderr is empty (per cli_affine_eval.rs precedent).
#[test]
fn eval_interpolation_all_checkpoints() {
    let path = common::example_path("interpolation.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval interpolation.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // §1 anchor: engineering-unit rendering (5 mm, 10 mm — NOT SI "0.005 m").
    assert!(
        stdout.contains(r#"Demo.label = "thickness is 5 mm, doubled is 10 mm""#),
        "stdout should contain the label anchor line;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Arithmetic: 1+1 → 2.
    assert!(
        stdout.contains(r#"Demo.arith = "x=2""#),
        "stdout should contain the arith checkpoint;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Escape: `{{braces}}` collapses to literal `{braces}`.
    assert!(
        stdout.contains(r#"Demo.escaped = "{braces}""#),
        "stdout should contain the escaped-braces checkpoint;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Undef-totality (PRD §6.3): the quoted `"gap is undef"` proves that
    // __interp_render rendered the Undef hole to literal text rather than
    // poisoning the cell to a bare unquoted `undef`.
    assert!(
        stdout.contains(r#"Demo.undef_demo = "gap is undef""#),
        "stdout should contain the undef_demo quoted line;\ngot stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

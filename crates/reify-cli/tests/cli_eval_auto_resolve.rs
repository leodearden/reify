//! End-to-end CLI tests for default-solver auto-param resolution via `reify eval`.
//!
//! Tests are RED on the base branch (before task-4132) because cmd_eval constructs
//! the Engine without a solver, so auto params Display as `undef`.  They turn GREEN
//! once the DimensionalSolver is wired into cmd_eval (step-2 of task-4132).

mod common;

/// `reify eval tests/fixtures/auto_minimize.ri` should exit 0 and resolve the
/// `thickness` auto param to a finite numeric SI value ≤ 5 mm (0.005 m in SI),
/// proving that the default DimensionalSolver minimised the objective.
///
/// Assertions:
/// 1. Exit status is success.
/// 2. The stdout line for `thickness` has an RHS that is NOT "undef".
/// 3. The leading token of the RHS parses as a finite f64 ≤ 0.005 (5 mm in SI).
/// 4. Determinism: running eval a second time produces the identical thickness line.
#[test]
fn eval_resolves_auto_param_via_default_solver() {
    let path = common::fixture_path("auto_minimize.ri");

    // ── Run 1 ──────────────────────────────────────────────────────────────────
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval auto_minimize.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Find the line containing both "thickness" and "="
    let thickness_line = stdout
        .lines()
        .find(|l| l.contains("thickness") && l.contains('='))
        .unwrap_or_else(|| {
            panic!(
                "no 'thickness = ...' line found in stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        });

    // 2. RHS must not be "undef"
    let rhs = thickness_line
        .split_once('=')
        .map(|x| x.1)
        .unwrap_or("")
        .trim();

    assert!(
        !rhs.starts_with("undef"),
        "thickness should be resolved (not undef) after default solver wired;\n\
         line: {thickness_line:?}\nstdout:\n{stdout}"
    );

    // 3. Leading token of the RHS should parse as a finite f64 ≤ 0.005 (5 mm in SI)
    let si_token = rhs.split_whitespace().next().unwrap_or("");
    let si_value: f64 = si_token.parse().unwrap_or_else(|_| {
        panic!(
            "RHS leading token {si_token:?} is not a valid f64;\nline: {thickness_line:?}"
        )
    });

    assert!(
        si_value.is_finite(),
        "resolved thickness SI value should be finite, got {si_value}"
    );
    assert!(
        si_value > 0.0,
        "resolved thickness must be physically positive (> 0), got {si_value:.9} m;\n\
         line: {thickness_line:?}"
    );
    assert!(
        si_value <= 0.005,
        "expected thickness ≤ 5 mm (0.005 m) after minimize, got {si_value:.6} m;\n\
         line: {thickness_line:?}"
    );

    // 4. Determinism: second run produces the identical thickness line
    let (status2, stdout2, _stderr2) = common::run_subcommand("eval", &path);

    assert!(
        status2.success(),
        "second run of reify eval auto_minimize.ri should also exit 0"
    );

    let thickness_line2 = stdout2
        .lines()
        .find(|l| l.contains("thickness") && l.contains('='))
        .unwrap_or_else(|| {
            panic!("no 'thickness = ...' line found in second run stdout:\n{stdout2}")
        });

    assert_eq!(
        thickness_line,
        thickness_line2,
        "resolved thickness line must be byte-identical across two runs (determinism check)"
    );
}

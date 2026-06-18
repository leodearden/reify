//! CLI integration tests for `reify explain` subcommand (task 4017, phase 5 B9 leaf).
//!
//! Cycle 1 (step-1 RED / step-2 GREEN): governing objective + combination tokens.
//! Cycle 2 (step-3 RED / step-4 GREEN): synthetic-vs-explicit source= token.
//! Cycle 3 (step-5 RED / step-6 GREEN): missing-file usage guard.
//!
//! RED on base branch because `explain` is an unknown command — `reify` prints
//! "Unknown command: explain" to stderr and exits FAILURE.

mod common;

/// `reify explain <explain_weighted.ri>` should exit 0 and print one B9-triple line
/// per auto cell (mass, stiffness) containing the objective and combination tokens.
///
/// Assertions:
/// (a) Exit status is success.
/// (b) stdout has a line for `mass` and a line for `stiffness`; each contains
///     `combination=weighted-sum`, `objective=` (but NOT `objective=none`).
/// (c) Determinism: a second run produces byte-identical stdout.
#[test]
fn explain_prints_governing_objective_and_combination() {
    let path = common::fixture_path("explain_weighted.ri");

    // ── Run 1 ──────────────────────────────────────────────────────────────────
    let (status, stdout, stderr) = common::run_subcommand("explain", &path);

    assert!(
        status.success(),
        "reify explain should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Find lines containing the cell names.
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("mass"))
        .unwrap_or_else(|| {
            panic!("no line containing 'mass' in stdout:\n{stdout}\nstderr:\n{stderr}")
        });

    let stiffness_line = stdout
        .lines()
        .find(|l| l.contains("stiffness"))
        .unwrap_or_else(|| {
            panic!("no line containing 'stiffness' in stdout:\n{stdout}\nstderr:\n{stderr}")
        });

    // Each line must contain combination=weighted-sum.
    assert!(
        mass_line.contains("combination=weighted-sum"),
        "mass line should contain 'combination=weighted-sum';\nline: {mass_line:?}\nstdout:\n{stdout}"
    );
    assert!(
        stiffness_line.contains("combination=weighted-sum"),
        "stiffness line should contain 'combination=weighted-sum';\nline: {stiffness_line:?}\nstdout:\n{stdout}"
    );

    // Each line must have objective= but NOT objective=none.
    assert!(
        mass_line.contains("objective="),
        "mass line should contain 'objective=';\nline: {mass_line:?}"
    );
    assert!(
        !mass_line.contains("objective=none"),
        "mass line should NOT contain 'objective=none' (explicit obj governs it);\nline: {mass_line:?}"
    );
    assert!(
        stiffness_line.contains("objective="),
        "stiffness line should contain 'objective=';\nline: {stiffness_line:?}"
    );
    assert!(
        !stiffness_line.contains("objective=none"),
        "stiffness line should NOT contain 'objective=none';\nline: {stiffness_line:?}"
    );

    // ── Run 2 (determinism) ────────────────────────────────────────────────────
    let (status2, stdout2, _) = common::run_subcommand("explain", &path);
    assert!(status2.success(), "second run of reify explain should exit 0");
    assert_eq!(
        stdout, stdout2,
        "reify explain output must be deterministic (byte-identical across runs)"
    );
}

/// `reify explain <explain_centrality.ri>` should show `objective=none`,
/// `combination=none`, and `source=synthetic-centrality` for the `x` cell.
/// `reify explain <explain_weighted.ri>` should show `source=explicit`.
///
/// RED until cycle-2 (step-4) appends the `source=` token.
#[test]
fn explain_prints_synthetic_vs_explicit_flag() {
    // ── Centrality fixture ─────────────────────────────────────────────────────
    let path_c = common::fixture_path("explain_centrality.ri");
    let (status_c, stdout_c, stderr_c) = common::run_subcommand("explain", &path_c);

    assert!(
        status_c.success(),
        "reify explain explain_centrality.ri should exit 0;\nstdout: {stdout_c}\nstderr: {stderr_c}"
    );

    let x_line = stdout_c
        .lines()
        .find(|l| l.contains(".x:") || l.ends_with(".x") || {
            // Match any line whose first token before ':' ends with '.x'
            l.split(':').next().map(|t| t.trim().ends_with(".x")).unwrap_or(false)
        })
        .unwrap_or_else(|| {
            panic!("no line for cell 'x' in stdout:\n{stdout_c}\nstderr:\n{stderr_c}")
        });

    assert!(
        x_line.contains("objective=none"),
        "x cell line should contain 'objective=none';\nline: {x_line:?}\nstdout:\n{stdout_c}"
    );
    assert!(
        x_line.contains("combination=none"),
        "x cell line should contain 'combination=none';\nline: {x_line:?}\nstdout:\n{stdout_c}"
    );
    assert!(
        x_line.contains("source=synthetic-centrality"),
        "x cell line should contain 'source=synthetic-centrality';\nline: {x_line:?}\nstdout:\n{stdout_c}"
    );

    // ── Weighted fixture ───────────────────────────────────────────────────────
    let path_w = common::fixture_path("explain_weighted.ri");
    let (status_w, stdout_w, stderr_w) = common::run_subcommand("explain", &path_w);

    assert!(
        status_w.success(),
        "reify explain explain_weighted.ri should exit 0;\nstdout: {stdout_w}\nstderr: {stderr_w}"
    );

    for line in stdout_w.lines().filter(|l| l.contains("mass") || l.contains("stiffness")) {
        assert!(
            line.contains("source=explicit"),
            "weighted cell line should contain 'source=explicit';\nline: {line:?}\nstdout:\n{stdout_w}"
        );
    }
}

/// `reify explain` with no file argument should exit FAILURE and print "Usage"
/// to stderr.
///
/// RED until cycle-3 (step-6) adds the usage guard.
#[test]
fn explain_without_file_prints_usage() {
    let (status, _stdout, stderr) = common::run_with_args(&["explain"]);

    assert!(
        !status.success(),
        "reify explain with no file should exit FAILURE;\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Usage"),
        "stderr should contain 'Usage' when no file given;\nstderr: {stderr}"
    );
}

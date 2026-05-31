//! End-to-end CLI tests for the 3-part bore-stack tolerance analysis example.
//!
//! TDD cycle:
//!   step-1 RED  — this file; examples/tolerance-stackup-3part.ri absent → eval fails
//!   step-2 GREEN— example created; worst/rss assertions pass
//!   step-3 RED  — MC gate added; mc_sigma key absent → extract_scalar panics
//!   step-4 GREEN— monte_carlo_stackup cell added to example; all assertions pass

mod common;

/// Extract the first numeric token following `"<key>": ` in the eval stdout.
///
/// `reify eval` prints Map entries as `"key": <value>`.  For LENGTH scalars the
/// value is `<si_value> <dimension>`, so the first whitespace-bounded token after
/// `": "` is the numeric SI value — independent of the dimension suffix.
fn extract_scalar(stdout: &str, key: &str) -> f64 {
    let needle = format!("\"{key}\": ");
    let start = stdout
        .find(&needle)
        .unwrap_or_else(|| panic!("key '{key}' not found in stdout:\n{stdout}"));
    let rest = &stdout[start + needle.len()..];
    let token = rest
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("no token after '{key}'"));
    // Scalar (LENGTH) values print as `<si_value> <dimension>`, so the numeric
    // token is already whitespace-delimited.  Real/Int values print bare — they
    // pick up trailing map punctuation (`,` or `}`).  Strip both.
    let clean = token.trim_end_matches([',', '}']);
    clean
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("parse f64 for '{key}' (token={token:?}): {e}"))
}

fn assert_rel_close(actual: f64, expected: f64, rel_tol: f64, label: &str) {
    let rel_err = (actual - expected).abs() / expected.abs();
    assert!(
        rel_err <= rel_tol,
        "{label}: rel_err {rel_err:.2e} > {rel_tol:.2e}: actual={actual:.15e}, expected={expected:.15e}"
    );
}

/// Rust-computed oracle for `rss_band` (sigma-invariant).
///
/// Chain (SI m): bore_depth ±5e-5, shaft ±1e-4, spacer ±4e-5, ring ±1e-5.
///   sum_tol_sq = (5e-5)²+(1e-4)²+(4e-5)²+(1e-5)² = 1.42e-8 m²
fn rss_band_oracle() -> f64 {
    (1.42e-8f64).sqrt()
}

/// `reify eval examples/tolerance-stackup-3part.ri` exits 0 and worst-case/RSS
/// values match the in-file hand-calc oracle to 1e-12 relative tolerance.
///
/// RED until step-2 creates the example file.
#[test]
fn eval_tolerance_stackup_3part_exact_values() {
    let path = common::example_path("tolerance-stackup-3part.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval tolerance-stackup-3part.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("worst_case_band"),
        "stdout should contain 'worst_case_band';\n{stdout}"
    );
    assert!(
        stdout.contains("rss_sigma"),
        "stdout should contain 'rss_sigma';\n{stdout}"
    );

    // Exact-math oracle assertions at 1e-12 relative.
    assert_rel_close(extract_scalar(&stdout, "nominal_gap"),     3.0e-3,              1e-12, "nominal_gap");
    assert_rel_close(extract_scalar(&stdout, "worst_case_band"), 2.0e-4,              1e-12, "worst_case_band");
    assert_rel_close(extract_scalar(&stdout, "worst_case_max"),  3.2e-3,              1e-12, "worst_case_max");
    assert_rel_close(extract_scalar(&stdout, "worst_case_min"),  2.8e-3,              1e-12, "worst_case_min");
    assert_rel_close(extract_scalar(&stdout, "rss_band"),        rss_band_oracle(),         1e-12, "rss_band");
    assert_rel_close(extract_scalar(&stdout, "rss_sigma"),       rss_band_oracle() / 3.0,   1e-12, "rss_sigma");
}

/// Monte-Carlo gate: mc_sigma converges to rss_sigma within 2% (all-Normal chain,
/// N=100k, seed=42 — PRD §3.3); mc_yield_fraction ≈ 1.0 for the [2.5mm, 3.5mm]
/// spec window (~±12σ_gap); two runs are byte-identical (INV-3).
///
/// RED until step-4 adds the monte_carlo_stackup cell to the example.
#[test]
fn eval_tolerance_stackup_3part_mc_gate() {
    let path = common::example_path("tolerance-stackup-3part.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval tolerance-stackup-3part.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("mc_sigma"),
        "stdout should contain 'mc_sigma' (MC cell missing from example);\n{stdout}"
    );

    let rss_sigma = extract_scalar(&stdout, "rss_sigma");
    let mc_sigma  = extract_scalar(&stdout, "mc_sigma");
    let rel_err   = (mc_sigma - rss_sigma).abs() / rss_sigma;
    assert!(
        rel_err <= 0.02,
        "mc_sigma not within 2% of rss_sigma: mc_sigma={mc_sigma:.6e}, rss_sigma={rss_sigma:.6e}, rel_err={rel_err:.4}"
    );

    let mc_yf = extract_scalar(&stdout, "mc_yield_fraction");
    assert!(
        (0.0..=1.0).contains(&mc_yf),
        "mc_yield_fraction out of [0,1]: {mc_yf}"
    );
    assert!(
        mc_yf >= 0.999,
        "mc_yield_fraction below 0.999 for ~±12σ spec window: {mc_yf}"
    );

    // INV-3: two runs must produce byte-identical stdout.
    let (_, stdout2, _) = common::run_subcommand("eval", &path);
    assert_eq!(stdout, stdout2, "two reify eval runs must be byte-identical (INV-3)");
}

/// Integration test: `reify eval examples/shells/thin_walled_bracket.ri`
/// exits 0, prints a `ThinWalledBracket.max_von_mises` line with a numeric
/// value in the one-OOM band [1.5e7, 1.5e9] Pa and the SI-base dimension
/// substring "kg·m^-1·s^-2", and emits no tet-fallback warning.
///
/// Analytical reference: σ = 6·P·L/(b·h²) = 6·20·0.1/(0.02·0.002²) = 1.5×10⁸ Pa.
///
/// `reify eval` prints cells via `Value::Display`:
///   `Value::Scalar { si_value, dimension }` → "{si_value} {dimension}"
/// where `dimension` for `PRESSURE` is "kg·m^-1·s^-2" (dimension.rs Display),
/// NOT the human unit "Pa".  The numeric token is base-SI Pascals, so the band
/// [1.5e7, 1.5e9] applies directly without any unit conversion.
///
/// OCCT independence: `box(...)` is a deferred GHR-β handle; the flat-plate
/// shell solve is pure-Rust.  Both `status.success()` and the absence of the
/// tet-fallback warning hold unconditionally (no `cfg(has_occt)` gate needed).
mod common;

#[test]
fn eval_thin_walled_bracket_exits_zero_with_in_band_max_von_mises() {
    let path = common::example_path("shells/thin_walled_bracket.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // (a) Exit 0.
    assert!(
        status.success(),
        "reify eval exited non-zero.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // (b) stdout has a ThinWalledBracket.max_von_mises line with an in-band value.
    let mvm_line = stdout
        .lines()
        .find(|l| l.contains("ThinWalledBracket.max_von_mises ="))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'ThinWalledBracket.max_von_mises =' line in stdout\n\
                 stdout:\n{stdout}\nstderr:\n{stderr}"
            )
        });

    // Split on '=' and parse the leading numeric token of the RHS.
    let rhs = mvm_line
        .split_once('=')
        .map(|x| x.1)
        .expect("line must contain '='")
        .trim();
    let numeric_token = rhs
        .split_whitespace()
        .next()
        .expect("RHS must have at least one token");
    let mvm: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!("leading RHS token {numeric_token:?} must parse as f64; line: {mvm_line:?}")
    });
    assert!(
        (1.5e7..=1.5e9).contains(&mvm),
        "ThinWalledBracket.max_von_mises {mvm:.4e} Pa outside one-OOM band \
         [1.5e7, 1.5e9] Pa around σ=6·P·L/(b·h²)=1.5e8.\nLine: {mvm_line:?}"
    );

    // The RHS must contain the SI-base pressure dimension (NOT "Pa").
    assert!(
        rhs.contains("kg\u{00b7}m^-1\u{00b7}s^-2"),
        "expected SI-base dimension 'kg·m^-1·s^-2' in the RHS of the \
         max_von_mises line.\nRHS: {rhs:?}"
    );

    // (c) No tet-fallback warning.
    // NOTE: the generic tet-fallback regression for the shell-extract path is
    // already covered by cli_eval_shell_no_tet_warning.rs (against
    // examples/fea_shell_flexure.ri).  The check here is a per-fixture guard
    // that ensures thin_walled_bracket.ri in particular never silently regresses
    // to the tet path; the intentional overlap is deliberate.
    assert!(
        !stderr.contains("falling back to tet meshing"),
        "Unexpected soft-fallback warning on stderr.\nstderr:\n{stderr}"
    );

    // (d) No Error-prefixed line.
    let error_line = stderr.lines().find(|l| l.starts_with("Error:"));
    assert!(
        error_line.is_none(),
        "unexpected 'Error:' line in stderr: {:?}\nstderr:\n{stderr}",
        error_line.unwrap()
    );
}

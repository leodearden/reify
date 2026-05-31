mod common;

use std::path::Path;

// ── step-1: check integration_full_v01.ri succeeds (SKIP if absent) ──────────

#[test]
fn check_integration_full_v01_succeeds() {
    let path = common::example_path("integration_full_v01.ri");
    if !Path::new(&path).exists() {
        // Note: `#[ignore]` cannot be used here because the skip decision is
        // made at runtime (file exists only after upstream task 291 merges).
        // Run with `--nocapture` to see SKIP messages.
        eprintln!(
            "SKIP cli_integration_smoke::check_integration_full_v01_succeeds: \
             {path} not present (dependency: task 291)"
        );
        return;
    }

    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check should exit 0 for integration_full_v01.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied") || stdout.contains("No constraints violated"),
        "stdout should contain a passing check summary, got: {stdout}"
    );
}

// ── step-3: reify test on m11_annotations.ri reports results (no exit-code assert) ──

#[test]
fn test_m11_annotations_reports_results() {
    let path = common::example_path("m11_annotations.ri");

    // m11_annotations.ri is a committed example in the worktree — no SKIP needed.
    let (_status, stdout, stderr) = common::run_subcommand("test", &path);

    // The fixture has PASS, FAIL, and INDETERMINATE rows.
    assert!(
        stdout.contains("PASS"),
        "stdout should contain 'PASS', got: {stdout}"
    );
    assert!(
        stdout.contains("FAIL"),
        "stdout should contain 'FAIL', got: {stdout}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );

    // At least one test name should be visible.
    assert!(
        stdout.contains("TestPositiveWidth")
            || stdout.contains("TestNegativeWidth")
            || stdout.contains("TestUnknownLoad"),
        "stdout should contain at least one test name, got: {stdout}"
    );

    // Summary line: fixture declares 3 passed, 2 failed, 1 indeterminate.
    // These counts correspond to the @test structures in examples/m11_annotations.ri:
    //   PASS:          TestPositiveWidth, TestColumnPositive, TestAssemblyFits  (3)
    //   FAIL:          TestNegativeWidth, TestColumnNegative                    (2, intentional)
    //   INDETERMINATE: TestUnknownLoad                                          (1)
    // If the fixture is updated, these counts must be updated here too.
    assert!(
        stdout.contains("3 passed"),
        "stdout should contain '3 passed' in summary, got: {stdout}"
    );
    assert!(
        stdout.contains("2 failed"),
        "stdout should contain '2 failed' in summary, got: {stdout}"
    );
    assert!(
        stdout.contains("1 indeterminate"),
        "stdout should contain '1 indeterminate' in summary, got: {stdout}"
    );

    // No internal errors or panics.
    assert!(
        !stderr.contains("panic"),
        "stderr should not contain 'panic', got: {stderr}"
    );
    assert!(
        !stderr.contains("Error reading"),
        "stderr should not contain 'Error reading', got: {stderr}"
    );
    // Exit code is NOT asserted — the fixture has intentional FAILs, so it exits non-zero by design.
}

// ── step-5: build integration_full_v01.ri compiles without panic ─────────────
//
// integration_full_v01.ri is a parametric constraint model, not a solid-geometry
// model. `reify build` currently exits non-zero for all parametric files because
// the STEP exporter requires explicit solid geometry (B-rep) which isn't produced
// by constraint evaluation. This test therefore only asserts that the build command
// does NOT panic or produce an internal error — the "No geometry output produced"
// exit-1 is the expected graceful failure for this file type.
//
// If STEP export is later added for constraint models, update this test to assert
// `out.status.success()` and check for the STEP output file.

#[test]
fn build_integration_full_v01_compiles_without_panic() {
    let path = common::example_path("integration_full_v01.ri");
    if !Path::new(&path).exists() {
        // Note: `#[ignore]` cannot be used here because the skip decision is
        // made at runtime (file exists only after upstream task 291 merges).
        // Run with `--nocapture` to see SKIP messages.
        eprintln!(
            "SKIP cli_integration_smoke::build_integration_full_v01_compiles_without_panic: \
             {path} not present (dependency: task 291)"
        );
        return;
    }

    let out = common::run_build_at(&path);

    // The build command must not panic (stderr must not contain "panicked at").
    assert!(
        !out.stderr.contains("panicked at"),
        "reify build must not panic for integration_full_v01.ri.\nstderr: {}",
        out.stderr
    );

    // It either succeeds (STEP exported — future when geometry support lands) or
    // fails gracefully with the expected "No geometry output produced" message.
    if out.status.success() {
        // Future path: STEP file was exported.
        assert!(
            out.stdout.contains("Wrote"),
            "successful build should contain 'Wrote', got: {}",
            out.stdout
        );
    } else {
        // Current expected path: constraint model has no solid geometry.
        assert!(
            out.stderr.contains("No geometry output produced")
                || out.stdout.contains("No geometry output produced"),
            "build failure should report 'No geometry output produced', got stdout: {} / stderr: {}",
            out.stdout,
            out.stderr
        );
    }
}

// ── task-2176 step-1: m5_geometry_flange.ri resolves stdlib (Material, Rigid) ─

#[test]
fn check_m5_geometry_flange_resolves_stdlib() {
    let path = common::example_path("m5_geometry_flange.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !stderr.contains("unresolved type: Material"),
        "stdlib type Material should be resolved; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("unresolved trait"),
        "stdlib traits (Rigid, Physical, MaterialSpec) should be resolved; stderr: {stderr}"
    );
    assert!(
        status.success(),
        "reify check should exit 0 for m5_geometry_flange.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied") || stdout.contains("No constraints violated"),
        "stdout should contain a passing check summary, got: {stdout}"
    );
}

// ── step-7: all present new M11 examples pass reify check ────────────────────

/// New M11 milestone example files.  Files created by sibling tasks are
/// included here but skipped gracefully when absent.
///
/// m11_annotations.ri is intentionally EXCLUDED: it contains @test structures
/// with violated constraints that make `reify check` exit non-zero (pre-2 confirms
/// this — cmd_check evaluates @test templates as regular constraints).
const NEW_M11_EXAMPLES: &[(&str, &str)] = &[
    ("m11_field_calculus.ri", ""),           // always present in worktree
    ("m11_combined.ri", "task 290"),         // created by task 290, may not be merged yet
    ("integration_full_v01.ri", "task 291"), // created by task 291, may not be merged yet
];

#[test]
fn new_m11_examples_pass_check_when_present() {
    let mut failures: Vec<String> = Vec::new();
    let mut tested: usize = 0;

    for (name, dep_task) in NEW_M11_EXAMPLES {
        let path = common::example_path(name);
        if !Path::new(&path).exists() {
            let dep_note = if dep_task.is_empty() {
                String::new()
            } else {
                format!(" (dependency: {})", dep_task)
            };
            eprintln!(
                "SKIP cli_integration_smoke::new_m11_examples_pass_check_when_present: \
                 {path} not present{dep_note}"
            );
            continue;
        }

        tested += 1;
        let (status, stdout, _stderr) = common::run_subcommand("check", &path);

        let passes = status.success()
            && (stdout.contains("All constraints satisfied")
                || stdout.contains("No constraints violated"));

        if !passes {
            failures.push(format!(
                "{name}: status={:?}, stdout snippet: {:?}",
                status.code(),
                // include first 300 chars of stdout for context
                stdout.chars().take(300).collect::<String>()
            ));
        }
    }

    // Guard against vacuous pass: at least m11_field_calculus.ri is always
    // committed and must be exercised.
    assert!(
        tested > 0,
        "no M11 examples were tested — all files are absent \
         (check that m11_field_calculus.ri exists in the examples/ directory)"
    );
    assert!(
        failures.is_empty(),
        "The following M11 examples failed reify check:\n{:#?}",
        failures
    );
}

// ── task-3953 step-7: complex_div.ri eval prints 0-1i ────────────────────────

/// End-to-end signal: `reify eval examples/complex_div.ri` must succeed and
/// stdout must contain "0-1i", the Display of complex(0,-1) (= 1/i = -i).
#[test]
fn eval_complex_div_prints_one_over_i_is_minus_i() {
    let path = common::example_path("complex_div.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for complex_div.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("quotient = 0-1i"),
        "stdout should contain 'quotient = 0-1i' (binding + value of 1/i = -i), got: {stdout}"
    );
    assert!(
        !stderr.contains("Error"),
        "stderr should not contain 'Error', got: {stderr}"
    );
}

// ── task-3950 step-7: complex_literals.ri eval exercises imaginary literal + add ─

/// End-to-end signal: `reify eval examples/complex_literals.ri` must succeed.
/// The file exercises `3.2 + 4.1j` (Real + imaginary literal → Complex) and
/// `3 + 4j` (Int + imaginary literal → Complex). Asserts:
///   - exit 0
///   - stdout contains "3.2" (re(z)) and "4.1" (im(z))
///   - stdout does NOT contain "undef" for the w = 3 + 4j binding
///   - stderr contains no "Error"
#[test]
fn eval_complex_literals_prints_re_and_im() {
    let path = common::example_path("complex_literals.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for complex_literals.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("3.2"),
        "stdout should contain '3.2' (re(z) or z value), got: {stdout}"
    );
    assert!(
        stdout.contains("4.1"),
        "stdout should contain '4.1' (im(z) or z value), got: {stdout}"
    );
    assert!(
        !stdout.contains("undef"),
        "stdout should not contain 'undef' (w = 3+4j must evaluate to a Complex), got: {stdout}"
    );
    assert!(
        !stderr.contains("Error"),
        "stderr should not contain 'Error', got: {stderr}"
    );
}

// ── task-3955: complex_numbers.ri combined eval (integration gate) ──

/// End-to-end signal: `reify eval examples/complex_numbers.ri` must succeed.
/// The file exercises literal sugar + abs/arg + division + complex_pow in a
/// single structure. Asserts:
/// - exit 0
/// - stdout contains "-7+24i" (complex_pow(3+4j, 2) = -7+24i, integer-trimmed Display)
/// - stdout contains "w_abs = 5" (|3+4i| = 5 exactly, integer-trimmed; verifies abs correctness
///   since constraints are inert under `eval` — only an explicit value check catches a wrong result)
/// - stdout does NOT contain "undef" (every binding evaluates)
/// - stderr contains no "Error"
#[test]
fn eval_complex_numbers_combined_demo() {
    let path = common::example_path("complex_numbers.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for complex_numbers.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("-7+24i"),
        "stdout should contain '-7+24i' (complex_pow(3+4j,2)), got: {stdout}"
    );
    assert!(
        stdout.contains("w_abs = 5"),
        "stdout should contain 'w_abs = 5' (|3+4i| = 5.0, integer-trimmed), got: {stdout}"
    );
    assert!(
        !stdout.contains("undef"),
        "stdout should not contain 'undef' (all bindings must evaluate), got: {stdout}"
    );
    assert!(
        !stderr.contains("Error"),
        "stderr should not contain 'Error', got: {stderr}"
    );
}

/// Constraint gate: `reify check examples/complex_numbers.ri` must pass.
/// The tight constraint pair `w_abs > 4.999` / `w_abs < 5.001` on the exact
/// 3-4-5 magnitude is enforced here — `eval` only prints binding values and
/// does not enforce constraints. This test catches any regression where abs
/// returns a wrong value that happens to be defined (non-undef) but numerically off.
#[test]
fn check_complex_numbers_constraints_pass() {
    let path = common::example_path("complex_numbers.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check should exit 0 for complex_numbers.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied") || stdout.contains("No constraints violated"),
        "stdout should contain a passing check summary, got: {stdout}"
    );
    let _ = stderr; // stderr not asserted — check may emit notes; success + summary are sufficient
}

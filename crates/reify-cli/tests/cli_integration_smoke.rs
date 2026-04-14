mod common;

use std::path::Path;
use std::process::Command;

/// Run `reify check <path>` and return (status, stdout, stderr).
fn run_check(path: &str) -> (std::process::ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

/// Run `reify test <path>` and return (status, stdout, stderr).
fn run_test(path: &str) -> (std::process::ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["test", path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

// ── step-1: check integration_full_v01.ri succeeds (SKIP if absent) ──────────

#[test]
fn check_integration_full_v01_succeeds() {
    let path = common::example_path("integration_full_v01.ri");
    if !Path::new(&path).exists() {
        eprintln!(
            "SKIP cli_integration_smoke::check_integration_full_v01_succeeds: \
             {path} not present (dependency: task 291)"
        );
        return;
    }

    let (status, stdout, stderr) = run_check(&path);

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
    let (_status, stdout, stderr) = run_test(&path);

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
    assert!(
        stdout.contains("3 passed"),
        "stdout should contain '3 passed' in summary, got: {stdout}"
    );
    assert!(
        stdout.contains("2 failed"),
        "stdout should contain '2 failed' in summary, got: {stdout}"
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

// ── step-5: build integration_full_v01.ri produces geometry (SKIP if absent) ─

#[test]
fn build_integration_full_v01_produces_geometry() {
    let path = common::example_path("integration_full_v01.ri");
    if !Path::new(&path).exists() {
        eprintln!(
            "SKIP cli_integration_smoke::build_integration_full_v01_produces_geometry: \
             {path} not present (dependency: task 291)"
        );
        return;
    }

    let out = common::run_build_at(&path);

    assert!(
        out.status.success(),
        "reify build should exit 0 for integration_full_v01.ri.\nstdout: {}\nstderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {}",
        out.stdout
    );
    assert!(
        out.output_path.exists(),
        "output STEP file should exist at {:?}",
        out.output_path
    );
    assert!(
        std::fs::metadata(&out.output_path).unwrap().len() > 0,
        "output STEP file should be non-empty at {:?}",
        out.output_path
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
    ("m11_field_calculus.ri", ""),          // always present in worktree
    ("m11_combined.ri", "task 290"),        // created by task 290, may not be merged yet
    ("integration_full_v01.ri", "task 291"), // created by task 291, may not be merged yet
];

#[test]
fn new_m11_examples_pass_check_when_present() {
    let mut failures: Vec<String> = Vec::new();

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

        let (status, stdout, _stderr) = run_check(&path);

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

    assert!(
        failures.is_empty(),
        "The following M11 examples failed reify check:\n{:#?}",
        failures
    );
}

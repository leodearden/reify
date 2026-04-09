mod common;

use std::process::Command;

/// Run `reify test <fixture>` and return (status, stdout, stderr).
fn run_test(fixture: &str) -> (std::process::ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["test", fixture])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

// Step 6: passing file exits success, shows PASS and test name
#[test]
fn test_command_on_passing_file_exits_success() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_all_pass.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when all tests pass.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("PASS"),
        "stdout should contain 'PASS', got: {stdout}"
    );
    assert!(
        stdout.contains("TestWidth") || stdout.contains("TestHeight"),
        "stdout should contain a test name, got: {stdout}"
    );
}

// Step 8: failing file exits with failure, shows FAIL and the failing test name
#[test]
fn test_command_on_failing_file_exits_failure() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_one_fail.ri"));

    assert!(
        !status.success(),
        "reify test should exit non-zero when a test fails.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("FAIL"),
        "stdout should contain 'FAIL', got: {stdout}"
    );
    assert!(
        stdout.contains("TestNegative"),
        "stdout should contain the failing test name 'TestNegative', got: {stdout}"
    );
}

// Step 9: indeterminate file exits success (indeterminate is non-violating)
#[test]
fn test_command_on_indeterminate_file_exits_success() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_indeterminate.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when all tests are indeterminate (not violated).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
}

// Step 10: mixed file — exits failure (has FAIL), shows all three statuses and summary counts
#[test]
fn test_command_on_mixed_file_shows_summary() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_mixed.ri"));

    assert!(
        !status.success(),
        "reify test should exit non-zero when some tests fail.\nstdout: {stdout}\nstderr: {stderr}"
    );
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
    // Summary line should show counts
    assert!(
        stdout.contains("1 passed"),
        "stdout should contain '1 passed' in summary, got: {stdout}"
    );
    assert!(
        stdout.contains("1 failed"),
        "stdout should contain '1 failed' in summary, got: {stdout}"
    );
    assert!(
        stdout.contains("1 indeterminate"),
        "stdout should contain '1 indeterminate' in summary, got: {stdout}"
    );
}

// S3: labels are left-padded to 13 chars so test names align
#[test]
fn test_command_on_mixed_file_aligns_labels() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_mixed.ri"));
    let _ = (status, stderr); // outcome already tested elsewhere

    // PASS is 4 chars; padded to 13 = 9 trailing spaces; then 2-space separator
    assert!(
        stdout.contains("  PASS           TestPass"),
        "stdout should contain padded PASS line, got: {stdout}"
    );
    // FAIL is 4 chars; same padding
    assert!(
        stdout.contains("  FAIL           TestFail"),
        "stdout should contain padded FAIL line, got: {stdout}"
    );
    // INDETERMINATE is exactly 13 chars; no extra padding, just 2-space separator
    assert!(
        stdout.contains("  INDETERMINATE  TestIndet"),
        "stdout should contain INDETERMINATE line, got: {stdout}"
    );
}

// S1: violated constraint details appear nested under FAIL line
#[test]
fn test_command_on_failing_file_shows_violated_constraint_details() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_one_fail.ri"));
    let _ = (status, stderr); // exit code already covered by another test

    // (a) sanity: FAIL line for TestNegative is present
    assert!(
        stdout.contains("FAIL") && stdout.contains("TestNegative"),
        "stdout should show the FAIL line for TestNegative, got: {stdout}"
    );
    // (b) "    VIOLATED " with 4-space indent appears after the FAIL line
    assert!(
        stdout.contains("    VIOLATED "),
        "stdout should contain nested '    VIOLATED ' line, got: {stdout}"
    );
    // (c) unlabeled constraint falls back to ConstraintNodeId display
    assert!(
        stdout.contains("TestNegative#constraint"),
        "stdout should contain the id fallback 'TestNegative#constraint', got: {stdout}"
    );
}

// Step 11: no-tests file exits success (vacuously passing)
#[test]
fn test_command_on_no_tests_file_exits_success() {
    let (status, stdout, stderr) = run_test(&common::fixture_path("test_none.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when there are no @test entities.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Should indicate zero tests ran
    assert!(
        stdout.contains("0 passed") || stdout.contains("no tests"),
        "stdout should mention 0 tests or 'no tests', got: {stdout}"
    );
}

// Step 12: no arguments shows usage on stderr
#[test]
fn test_command_no_args_shows_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["test"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify test with no args should exit non-zero"
    );
    assert!(
        stderr.contains("Usage"),
        "stderr should contain 'Usage', got: {stderr}"
    );
}

// Step 13: nonexistent file exits failure with error message
#[test]
fn test_command_nonexistent_file_exits_failure() {
    let (status, _stdout, stderr) = run_test("nonexistent_file_that_does_not_exist.ri");

    assert!(
        !status.success(),
        "reify test should exit non-zero for missing file.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain 'Error reading', got: {stderr}"
    );
}

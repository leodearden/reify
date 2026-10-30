use crate::common;

// Step 6: passing file exits success, shows PASS and test name
#[test]
fn test_command_on_passing_file_exits_success() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_all_pass.ri"));

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
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_one_fail.ri"));

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
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_indeterminate.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when all tests are indeterminate (not violated).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
}

// A FAIL names the violated constraint under its status line, and the
// violation diagnostic reaches stderr.
#[test]
fn test_command_prints_the_violation_behind_a_fail() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_one_fail.ri"));

    assert!(
        !status.success(),
        "reify test should exit non-zero when a test fails.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "  FAIL  TestNegative",
            "    VIOLATED TestNegative#constraint[0]",
            "test result: FAIL. 0 passed; 1 failed; 0 indeterminate",
        ],
        "stderr: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|line| line == "error: constraint TestNegative#constraint[0] violated"),
        "stderr should carry the violation diagnostic, got: {stderr}"
    );
}

// An INDETERMINATE names the undecided constraint and its recorded reason
// under its status line, and the indeterminate diagnostic reaches stderr.
#[test]
fn test_command_prints_the_reason_behind_an_indeterminate() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_indeterminate.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when no test fails.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            "  INDETERMINATE  TestUndefTolerance",
            "    INDETERMINATE TestUndefTolerance#constraint[0]: undefined inputs: TestUndefTolerance.tolerance",
            "test result: ok. 0 passed; 0 failed; 1 indeterminate",
        ],
        "stderr: {stderr}"
    );
    assert!(
        stderr.lines().any(|line| line
            == "warning: constraint TestUndefTolerance#constraint[0] indeterminate: undefined inputs: TestUndefTolerance.tolerance"),
        "stderr should carry the indeterminate diagnostic, got: {stderr}"
    );
}

// Solver-driver parity B8: `reify test` resolves autos, so a test whose
// constraint reads a two-sided auto gets a real verdict, not Indeterminate.
#[test]
fn test_command_reports_a_real_verdict_for_a_test_reading_an_auto() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_auto_resolves.ri"));

    assert!(
        status.success(),
        "reify test should exit 0 when the auto resolves and its constraint holds.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.lines().any(|line| line == "  PASS  TestSolvedAuto"),
        "stdout should report TestSolvedAuto as PASS, got: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line == "test result: ok. 1 passed; 0 failed; 0 indeterminate"),
        "stdout should summarise one passing test, got: {stdout}"
    );
    assert!(
        !stdout.lines().any(|line| line.contains("INDETERMINATE")),
        "a solved auto must not leave any INDETERMINATE line, got: {stdout}"
    );
}

// Step 10: mixed file — exits failure (has FAIL), shows all three statuses and summary counts
#[test]
fn test_command_on_mixed_file_shows_summary() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_mixed.ri"));

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
}

// Step 11: no-tests file exits success (vacuously passing)
#[test]
fn test_command_on_no_tests_file_exits_success() {
    let (status, stdout, stderr) =
        common::run_subcommand("test", &common::fixture_path("test_none.ri"));

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
    let (status, _stdout, stderr) = common::run_with_args(&["test"]);

    assert!(
        !status.success(),
        "reify test with no args should exit non-zero; got exit {:?}",
        status.code()
    );
    assert!(
        stderr.contains("Usage"),
        "stderr should contain 'Usage', got: {stderr}"
    );
}

// Step 13: nonexistent file exits failure with error message
#[test]
fn test_command_nonexistent_file_exits_failure() {
    let (status, _stdout, stderr) =
        common::run_subcommand("test", "nonexistent_file_that_does_not_exist.ri");

    assert!(
        !status.success(),
        "reify test should exit non-zero for missing file.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain 'Error reading', got: {stderr}"
    );
}

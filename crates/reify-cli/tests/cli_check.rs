use std::process::Command;

fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/{}", manifest_dir, name)
}

#[test]
fn check_valid_bracket_exits_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", &fixture_path("bracket.ri")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "reify check should exit 0 for valid bracket.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
    assert!(
        !stderr.contains("Unknown command"),
        "stderr should not contain 'Unknown command', got: {stderr}"
    );
    // Channel-regression: constraint output must NOT leak to stderr
    assert!(
        !stderr.contains("All constraints satisfied"),
        "stderr should not contain constraint summary, got: {stderr}"
    );
    assert!(
        !stderr.contains("OK "),
        "stderr should not contain constraint status 'OK', got: {stderr}"
    );
}

#[test]
fn check_violating_bracket_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", &fixture_path("bracket_violating.ri")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify check should exit non-zero for violating bracket.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated"),
        "stdout should contain 'Some constraints violated', got: {stdout}"
    );
    // Channel-regression: constraint output must NOT leak to stderr
    assert!(
        !stderr.contains("VIOLATED"),
        "stderr should not contain 'VIOLATED', got: {stderr}"
    );
    assert!(
        !stderr.contains("Some constraints violated"),
        "stderr should not contain constraint summary, got: {stderr}"
    );
}

#[test]
fn check_constraint_output_on_stdout_not_stderr() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", &fixture_path("bracket_violating.ri")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Positive: constraint output appears on stdout
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated"),
        "stdout should contain 'Some constraints violated', got: {stdout}"
    );
    // Negative: constraint output must NOT appear on stderr
    assert!(
        !stderr.contains("VIOLATED"),
        "stderr must not contain 'VIOLATED' (regression for output channel bug), got: {stderr}"
    );
    assert!(
        !stderr.contains("Some constraints violated"),
        "stderr must not contain 'Some constraints violated' (regression for output channel bug), got: {stderr}"
    );
}

#[test]
fn check_parse_error_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", &fixture_path("bracket_parse_error.ri")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify check should exit non-zero for file with parse errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Parse error"),
        "stderr should contain 'Parse error', got: {stderr}"
    );
}

#[test]
fn check_compile_error_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", &fixture_path("bracket_compile_error.ri")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify check should exit non-zero for file with compiler errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
}

#[test]
fn check_nonexistent_file_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check", "nonexistent_file_that_does_not_exist.ri"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify check should exit non-zero for missing file.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain error message about reading, got: {stderr}"
    );
}

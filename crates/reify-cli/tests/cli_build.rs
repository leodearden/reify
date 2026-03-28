use std::process::Command;

fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/{}", manifest_dir, name)
}

#[test]
fn build_parse_error_exits_failure() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_parse_error.ri"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify build should exit non-zero for file with parse errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Parse error"),
        "stderr should contain 'Parse error', got: {stderr}"
    );
}

#[test]
fn build_violating_bracket_exits_failure() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_violating.ri"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify build should exit non-zero when constraints are violated.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain summary message, got: {stdout}"
    );
    // Geometry file should still be written even when constraints are violated
    assert!(
        output_path.exists(),
        "geometry file should still be written even with constraint violations"
    );
}

#[test]
fn build_valid_bracket_exits_success() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket.ri"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "reify build should exit 0 for valid bracket.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {stdout}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED' for valid bracket, got: {stdout}"
    );
    assert!(
        output_path.exists(),
        "geometry file should be written on success"
    );
}

#[test]
fn build_compile_error_exits_failure() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_compile_error.ri"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify build should exit non-zero for file with compiler errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
}

#[test]
fn build_indeterminate_constraint_exits_success() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_indeterminate.ri"),
            "-o",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "reify build should exit 0 when constraints are indeterminate (not violated).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {stdout}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("OK"),
        "stdout should contain 'OK' for the satisfied thickness constraint, got: {stdout}"
    );
    assert!(
        !stdout.contains("Some constraints violated"),
        "stdout should NOT contain violation summary, got: {stdout}"
    );
    // Note: build path does not print constraint_summary_message (unlike check path),
    // so we only verify absence of wrong summaries, not presence of correct one.
    assert!(
        !stdout.contains("All constraints satisfied"),
        "stdout should NOT contain 'All constraints satisfied' when indeterminate, got: {stdout}"
    );
    assert!(
        output_path.exists(),
        "geometry file should be written when constraints are only indeterminate"
    );
}

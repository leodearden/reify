use std::process::Command;

fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/{}", manifest_dir, name)
}

#[test]
fn build_parse_error_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_parse_error.ri"),
            "-o",
            "/tmp/reify_test_parse_error_out.step",
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
    let output_path = "/tmp/reify_test_violating_build_out.step";
    // Pre-cleanup: remove any stale file from a prior panicked run to avoid
    // a false positive on the exists() assertion below.
    let _ = std::fs::remove_file(output_path);
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_violating.ri"),
            "-o",
            output_path,
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
        stderr.contains("VIOLATED"),
        "stderr should contain 'VIOLATED', got: {stderr}"
    );
    assert!(
        stderr.contains("Some constraints violated."),
        "stderr should contain summary message, got: {stderr}"
    );
    // Geometry file should still be written even when constraints are violated
    assert!(
        std::path::Path::new(output_path).exists(),
        "geometry file should still be written even with constraint violations"
    );
    // Clean up
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn build_valid_bracket_exits_success() {
    let output_path = "/tmp/reify_test_valid_build_out.step";
    let _ = std::fs::remove_file(output_path);
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket.ri"),
            "-o",
            output_path,
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
        !stderr.contains("VIOLATED"),
        "stderr should NOT contain 'VIOLATED' for valid bracket, got: {stderr}"
    );
    assert!(
        std::path::Path::new(output_path).exists(),
        "geometry file should be written on success"
    );
    // Clean up
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn build_compile_error_exits_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path("bracket_compile_error.ri"),
            "-o",
            "/tmp/reify_test_compile_error_out.step",
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

use std::process::Command;

// Test fixture for gui subcommand integration tests.
// Uses the same pattern as cli_smoke.rs.
fn reify_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify"));
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd
}

#[test]
fn gui_no_file_shows_usage() {
    let output = reify_cmd()
        .arg("gui")
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with no file should exit non-zero"
    );
    assert!(
        stderr.contains("Usage") || stderr.contains("usage"),
        "should show usage message mentioning file path, got: {stderr}"
    );
    assert!(
        stderr.contains("<file>") || stderr.contains("file"),
        "usage message should mention a file argument, got: {stderr}"
    );
}

#[test]
fn gui_nonexistent_file_shows_error() {
    let output = reify_cmd()
        .args(["gui", "nonexistent.ri"])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify gui with nonexistent file should exit non-zero"
    );
    assert!(
        stderr.contains("not found") || stderr.contains("does not exist") || stderr.contains("No such file"),
        "should report file not found error, got: {stderr}"
    );
}

#[test]
fn gui_non_ri_file_shows_error() {
    // Create a temporary .txt file
    let tmp_dir = std::env::temp_dir();
    let txt_file = tmp_dir.join("test_reify_gui.txt");
    std::fs::write(&txt_file, "not a reify file").expect("failed to create temp file");

    let output = reify_cmd()
        .args(["gui", txt_file.to_str().unwrap()])
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Cleanup
    let _ = std::fs::remove_file(&txt_file);

    assert!(
        !output.status.success(),
        "reify gui with non-.ri file should exit non-zero"
    );
    assert!(
        stderr.contains(".ri"),
        "should mention .ri extension requirement, got: {stderr}"
    );
}

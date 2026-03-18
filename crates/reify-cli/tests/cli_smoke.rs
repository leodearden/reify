use std::process::Command;

#[test]
fn no_args_shows_help_with_all_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify with no args should exit non-zero"
    );
    assert!(
        stderr.contains("check"),
        "help text should mention 'check' command, got: {stderr}"
    );
    assert!(
        stderr.contains("build"),
        "help text should mention 'build' command, got: {stderr}"
    );
    assert!(
        stderr.contains("lsp"),
        "help text should mention 'lsp' command, got: {stderr}"
    );
}

#[test]
fn check_no_file_shows_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["check"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify check with no file should exit non-zero"
    );
    assert!(
        stderr.contains("Usage"),
        "should show usage message, got: {stderr}"
    );
}

#[test]
fn build_no_file_shows_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["build"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "reify build with no file should exit non-zero"
    );
    assert!(
        stderr.contains("Usage"),
        "should show usage message, got: {stderr}"
    );
}

#[test]
fn lsp_command_is_recognized() {
    // Run 'reify lsp' with null stdin — it should not output "Unknown command"
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["lsp"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Unknown command"),
        "CLI should recognize 'lsp' command, got: {stderr}"
    );
}

use std::process::Command;

#[test]
fn lsp_command_is_recognized() {
    // Run 'reify lsp' — it should not output "Unknown command: lsp"
    // Since the actual LSP server reads stdin, we'll just check the binary
    // compiles and the command is recognized by looking for the absence
    // of "Unknown command" in stderr.
    // We use a timeout approach: send nothing to stdin and check the exit.
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
        "CLI should recognize 'lsp' command, but got: {stderr}"
    );
}

#[test]
fn help_text_includes_lsp() {
    // Run with no args to get help text
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lsp"),
        "Help text should mention 'lsp' command, but got: {stderr}"
    );
}

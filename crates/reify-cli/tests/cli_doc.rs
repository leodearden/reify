//! End-to-end subprocess tests for `reify doc`.
//!
//! Each test invokes the compiled `reify` binary as a subprocess (via
//! `CARGO_BIN_EXE_reify`) and asserts on exit code, stdout, and stderr.
//! Exit-code conventions:
//! - `0` — success.
//! - `1` — parse / compile errors prevented doc generation.
//! - `2` — CLI usage errors (bad flag, missing positional, conflicting flags).

mod common;

use std::process::{Command, ExitStatus, Stdio};

/// Run `reify doc <args...>` and return `(status, stdout, stderr)`.
///
/// Thin wrapper around `Command::new(env!("CARGO_BIN_EXE_reify"))` that
/// prepends the `"doc"` subcommand and forwards the rest of `args`.
fn run_doc(args: &[&str]) -> (ExitStatus, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify"));
    cmd.arg("doc");
    cmd.args(args);
    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

#[test]
fn doc_no_args_prints_usage_and_exits_two() {
    let (status, stdout, stderr) = run_doc(&[]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc with no args must exit 2 (usage error).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Usage: reify doc"),
        "stderr should contain 'Usage: reify doc', got: {stderr}"
    );
}

#[test]
fn doc_unknown_flag_exits_two() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--frobnicate", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc with an unknown flag must exit 2.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unknown flag"),
        "stderr should contain 'unknown flag', got: {stderr}"
    );
    assert!(
        stderr.contains("--frobnicate"),
        "stderr should name the offending flag '--frobnicate', got: {stderr}"
    );
}

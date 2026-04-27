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
fn doc_format_json_pretty_emits_valid_json_to_stdout() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "json", &path]);

    assert!(
        status.success(),
        "reify doc --format json on bracket.ri must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let model: reify_doc::model::DocModel = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must parse as DocModel JSON: {e}\nstdout: {stdout}"));
    assert!(
        model.modules.iter().any(|m| m.path == "bracket"),
        "DocModel should contain a module with path == 'bracket', got modules: {:?}",
        model.modules.iter().map(|m| &m.path).collect::<Vec<_>>()
    );
    // Pretty mode: stdout must contain newlines.
    assert!(
        stdout.contains('\n'),
        "pretty json must be multi-line, got: {stdout}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should not contain 'error:' on success, got: {stderr}"
    );
}

#[test]
fn doc_compile_error_exits_one_with_stderr() {
    let path = common::fixture_path("bracket_compile_error.ri");
    let (status, stdout, stderr) = run_doc(&[&path]);

    assert_eq!(
        status.code(),
        Some(1),
        "compile errors must exit 1.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:' from a compile diagnostic, got: {stderr}"
    );
    // No doc body should reach stdout when compilation fails.
    assert!(
        !stdout.contains("<!DOCTYPE html>"),
        "stdout should not contain HTML doc body on compile error, got: {stdout}"
    );
    assert!(
        !stdout.contains("\"modules\""),
        "stdout should not contain JSON doc body on compile error, got: {stdout}"
    );
}

#[test]
fn doc_missing_file_exits_one() {
    let (status, stdout, stderr) = run_doc(&["nonexistent_file_2361.ri"]);

    assert_eq!(
        status.code(),
        Some(1),
        "missing file must exit 1.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain 'Error reading' for missing file, got: {stderr}"
    );
}

#[test]
fn doc_format_json_compact_emits_single_line() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "json", "--compact", &path]);

    assert!(
        status.success(),
        "reify doc --format json --compact must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // After stripping a single trailing newline (cmd_doc emits one for tidy
    // shell output), the JSON body itself must be a single line.
    let body = stdout.trim_end_matches('\n');
    assert!(
        !body.contains('\n'),
        "compact json body must be single-line, got: {body}"
    );
    let _model: reify_doc::model::DocModel = serde_json::from_str(body).unwrap_or_else(|e| {
        panic!("compact stdout must parse as DocModel JSON: {e}\nbody: {body}")
    });
}

#[test]
fn doc_format_markdown_emits_markdown_to_stdout() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "markdown", &path]);

    assert!(
        status.success(),
        "reify doc --format markdown must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.starts_with("# bracket"),
        "markdown output must start with '# bracket' (the module H1 from \
         render_markdown's single-mode), got: {stdout}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should not contain 'error:' on success, got: {stderr}"
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

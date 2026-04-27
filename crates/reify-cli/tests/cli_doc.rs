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
fn doc_default_format_is_html_stub() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&[&path]);

    assert!(
        status.success(),
        "reify doc with no --format must default to html and exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.starts_with("<!DOCTYPE html>"),
        "default html stub must start with '<!DOCTYPE html>', got: {stdout}"
    );
    assert!(
        stdout.contains("<html"),
        "html output must contain '<html', got: {stdout}"
    );
    assert!(
        stdout.contains("</html>"),
        "html output must contain '</html>', got: {stdout}"
    );
    assert!(
        stdout.contains("<title>bracket</title>"),
        "html output must contain '<title>bracket</title>' (from the \
         minimal DocModel's module path), got: {stdout}"
    );
    assert!(
        stdout.contains("<pre>"),
        "html stub must wrap markdown body in a <pre> block, got: {stdout}"
    );
    assert!(
        stdout.contains("# bracket"),
        "html stub's <pre> body must contain the markdown H1 '# bracket', \
         got: {stdout}"
    );
}

#[test]
fn doc_split_with_json_exits_two() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "json", "--split", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format json --split must exit 2 (usage error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--split"),
        "stderr should mention '--split', got: {stderr}"
    );
    assert!(
        stderr.contains("markdown only") || stderr.contains("markdown"),
        "stderr should explain that --split is markdown-only, got: {stderr}"
    );
}

#[test]
fn doc_split_with_html_exits_two() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "html", "--split", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format html --split must exit 2 (usage error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--split"),
        "stderr should mention '--split', got: {stderr}"
    );
    assert!(
        stderr.contains("markdown only") || stderr.contains("markdown"),
        "stderr should explain that --split is markdown-only, got: {stderr}"
    );
}

#[test]
fn doc_compact_with_markdown_exits_two() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "markdown", "--compact", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format markdown --compact must exit 2 (usage error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--compact"),
        "stderr should mention '--compact', got: {stderr}"
    );
    assert!(
        stderr.contains("json only") || stderr.contains("json"),
        "stderr should explain that --compact is json-only, got: {stderr}"
    );
}

#[test]
fn doc_compact_with_html_exits_two() {
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "html", "--compact", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format html --compact must exit 2 (usage error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--compact"),
        "stderr should mention '--compact', got: {stderr}"
    );
    assert!(
        stderr.contains("json only") || stderr.contains("json"),
        "stderr should explain that --compact is json-only, got: {stderr}"
    );
}

#[test]
fn doc_o_flag_writes_to_file_for_json() {
    let path = common::fixture_path("bracket.ri");
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let out_path = dir.path().join("doc.json");
    let out_str = out_path.to_str().expect("tmp path is utf-8");

    let (status, stdout, stderr) = run_doc(&["--format", "json", "-o", out_str, &path]);

    assert!(
        status.success(),
        "reify doc --format json -o <file> must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "stdout must be empty when -o <file> is supplied, got: {stdout}"
    );
    assert!(
        out_path.exists(),
        "expected file at {} to exist after `-o`",
        out_path.display()
    );
    let bytes = std::fs::read(&out_path).expect("read tmp doc.json");
    let _model: reify_doc::model::DocModel = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "tmp doc.json must parse as DocModel JSON: {e}\n\
             body: {}",
            String::from_utf8_lossy(&bytes)
        )
    });
}

#[test]
fn doc_split_markdown_writes_files_to_directory() {
    let path = common::fixture_path("bracket.ri");
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let dir_str = dir.path().to_str().expect("tmp dir path is utf-8");

    let (status, stdout, stderr) =
        run_doc(&["--format", "markdown", "--split", "-o", dir_str, &path]);

    assert!(
        status.success(),
        "reify doc --format markdown --split -o <dir> must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let entries: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read tmp dir")
        .map(|e| e.expect("read entry").file_name().to_string_lossy().into_owned())
        .collect();
    // Minimal placeholder DocModel has zero items, so render_split emits
    // only `index.md` (matches fmt_markdown::render_split's known
    // item-less behaviour).
    assert_eq!(
        entries,
        vec!["index.md".to_string()],
        "expected exactly index.md in {}, got: {entries:?}",
        dir.path().display()
    );
    let index = std::fs::read_to_string(dir.path().join("index.md")).expect("read index.md");
    assert!(
        index.starts_with("# bracket"),
        "index.md must start with '# bracket', got: {index}"
    );
}

#[test]
fn doc_split_without_output_path_exits_two() {
    // Regression guard: step 28's Split arm requires `-o <dir>`.  This test
    // pins that behaviour; if a future refactor accidentally allows
    // `--split` without `-o`, this test fails loudly.
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "markdown", "--split", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format markdown --split without -o must exit 2.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--split requires -o"),
        "stderr should explain that --split requires -o, got: {stderr}"
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

#[test]
fn doc_listed_in_top_level_usage() {
    // Regression guard: invoking `reify` with no arguments prints a usage
    // listing on stderr.  That listing must mention the `doc` subcommand so
    // users can discover it.  Pins the line added to `main()` in step 6.
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("doc <file>"),
        "top-level usage hint must list 'doc <file>', got stderr: {stderr}"
    );
}

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
fn doc_parse_error_exits_one_with_stderr() {
    // Mirrors `doc_compile_error_exits_one_with_stderr` but exercises the
    // *parse-error* branch of `parse_and_compile`.  Compile errors print
    // `error: ...` (severity tag); parse errors print `Parse error: ...`.
    // Without this test, the parse-error branch of cmd_doc's pipeline is
    // exercised only transitively by other crates.
    let path = common::fixture_path("bracket_parse_error.ri");
    let (status, stdout, stderr) = run_doc(&[&path]);

    assert_eq!(
        status.code(),
        Some(1),
        "parse errors must exit 1.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Parse error:"),
        "stderr should contain 'Parse error:' for a parse failure, got: {stderr}"
    );
    // No doc body should reach stdout when parsing fails.
    assert!(
        !stdout.contains("<!DOCTYPE html>"),
        "stdout should not contain HTML doc body on parse error, got: {stdout}"
    );
    assert!(
        !stdout.contains("\"modules\""),
        "stdout should not contain JSON doc body on parse error, got: {stdout}"
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
fn doc_markdown_populates_cross_refs() {
    // bracket_with_trait.ri declares `trait HasHole` and `structure Bracket : HasHole`.
    // After step-10 wires build_cross_refs, the markdown output for Bracket must
    // contain a "Conforms to" cross-ref section listing HasHole.
    // Fails against the stub because the stub passes None for cross_refs.
    let path = common::fixture_path("bracket_with_trait.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "markdown", &path]);

    assert!(
        status.success(),
        "reify doc --format markdown on bracket_with_trait.ri must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // Real items must appear: the trait and the conforming structure.
    assert!(
        stdout.contains("HasHole"),
        "markdown must contain trait name 'HasHole', got: {stdout}"
    );
    assert!(
        stdout.contains("Bracket"),
        "markdown must contain structure name 'Bracket', got: {stdout}"
    );
    // Cross-ref section must appear because Bracket : HasHole.
    assert!(
        stdout.contains("Conforms to"),
        "markdown must contain 'Conforms to' cross-ref section for Bracket, got: {stdout}"
    );
}

#[test]
fn doc_default_format_is_real_html() {
    // Regression guard: default format is HTML via reify_doc::fmt_html::render_html.
    // Asserts real (non-stub) output: embedded <style>, module <h1>, item names,
    // and param names must appear; the old <pre>-wrapped stub body must NOT.
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&[&path]);

    assert!(
        status.success(),
        "reify doc with no --format must default to html and exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.starts_with("<!DOCTYPE html>"),
        "html output must start with '<!DOCTYPE html>', got: {stdout}"
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
        "html output must contain '<title>bracket</title>' (module path as page title), \
         got: {stdout}"
    );
    // Real render_html embeds a stylesheet — the stub does not.
    assert!(
        stdout.contains("<style>"),
        "real html must contain an embedded '<style>' block, got: {stdout}"
    );
    // Real render_html emits module path as an <h1> — the stub does not.
    assert!(
        stdout.contains("<h1>bracket</h1>"),
        "real html must contain '<h1>bracket</h1>' (module H1), got: {stdout}"
    );
    // Real DocModel from build_doc_model contains the 'Bracket' structure item.
    assert!(
        stdout.contains("Bracket"),
        "real html must contain item name 'Bracket', got: {stdout}"
    );
    // Real DocModel from build_doc_model contains the 'width' param.
    assert!(
        stdout.contains("width"),
        "real html must contain param name 'width', got: {stdout}"
    );
    // The old stub wrapped a <pre> block — real render_html does not.
    assert!(
        !stdout.contains("<pre>"),
        "real html must NOT contain a '<pre>' stub block, got: {stdout}"
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
fn doc_o_flag_writes_markdown_without_extra_trailing_newline() {
    // Pins the contract: the CLI file-write path produces the EXACT same bytes
    // as the stdout path for markdown single-file mode.  Both paths call
    // write_single_file_or_stdout with trailing_newline=false, so the file
    // content must equal the stdout content byte-for-byte.
    //
    // This replaces the weaker `!written.ends_with("\n\n\n")` proxy that would
    // miss a single spurious trailing newline when the formatter body already
    // ends in exactly one newline.
    let path = common::fixture_path("bracket.ri");
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let out_path = dir.path().join("doc.md");
    let out_str = out_path.to_str().expect("tmp path is utf-8");

    // (a) Write to file with -o.
    let (file_status, file_stdout, file_stderr) =
        run_doc(&["--format", "markdown", "-o", out_str, &path]);
    assert!(
        file_status.success(),
        "reify doc --format markdown -o <file> must exit 0.\n\
         stdout: {file_stdout}\nstderr: {file_stderr}"
    );
    assert!(
        file_stdout.is_empty(),
        "stdout must be empty when -o <file> is supplied, got: {file_stdout}"
    );

    // (b) Emit to stdout without -o.
    let (stdout_status, stdout_output, stdout_stderr) = run_doc(&["--format", "markdown", &path]);
    assert!(
        stdout_status.success(),
        "reify doc --format markdown must exit 0.\n\
         stdout: {stdout_output}\nstderr: {stdout_stderr}"
    );

    let written = std::fs::read_to_string(&out_path).expect("read tmp doc.md");

    // Structural assertions on the real DocModel content.
    assert!(
        written.starts_with("# bracket"),
        "markdown file must start with '# bracket' (module H1), got: {written}"
    );
    assert!(
        written.contains("Bracket"),
        "markdown file must contain item name 'Bracket' (from real DocModel), got: {written}"
    );
    assert!(
        written.contains("width"),
        "markdown file must contain param name 'width' (from real DocModel), got: {written}"
    );

    // Byte-for-byte: file content must exactly equal stdout output.
    // Markdown single-file mode uses trailing_newline=false for BOTH paths, so
    // no extra bytes should be added in file mode.
    assert_eq!(
        written, stdout_output,
        "markdown file content must exactly match stdout output \
         (file mode must not append extra bytes on top of the formatter output)"
    );
}

#[test]
fn doc_o_flag_writes_html_without_extra_trailing_newline() {
    // Same contract as the markdown variant: `-o <file>` writes the raw
    // formatter output without appending a stdout-style trailing newline.
    // render_html ends with `</html>\n` (one newline), so the on-disk byte
    // count is exactly that — no extra `\n` after.
    let path = common::fixture_path("bracket.ri");
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let out_path = dir.path().join("doc.html");
    let out_str = out_path.to_str().expect("tmp path is utf-8");

    let (status, stdout, stderr) = run_doc(&["--format", "html", "-o", out_str, &path]);

    assert!(
        status.success(),
        "reify doc --format html -o <file> must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "stdout must be empty when -o <file> is supplied, got: {stdout}"
    );
    let written = std::fs::read_to_string(&out_path).expect("read tmp doc.html");
    // Real html from render_html must contain the 'Bracket' item name.
    assert!(
        written.contains("Bracket"),
        "html file must contain item name 'Bracket' (from real DocModel), got: {written}"
    );
    // render_html ends with exactly one `\n` after `</html>`.
    // File-mode must NOT add a second trailing newline on top of that.
    assert!(
        written.ends_with("</html>\n"),
        "html file body must end with '</html>\\n' (one newline from the \
         formatter, not two), got tail: {:?}",
        &written[written.len().saturating_sub(20)..]
    );
    assert!(
        !written.ends_with("</html>\n\n"),
        "html file body must NOT end with double newlines (file mode does \
         not append a trailing newline), got tail: {:?}",
        &written[written.len().saturating_sub(20)..]
    );
}

#[test]
fn doc_format_without_value_exits_two() {
    // Pins the `--format` requires-a-value branch in cmd_doc's arg loop.
    // Easy regression to introduce when refactoring; this test catches it.
    let (status, stdout, stderr) = run_doc(&["--format"]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format with no value must exit 2.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("--format requires a value"),
        "stderr should contain '--format requires a value', got: {stderr}"
    );
}

#[test]
fn doc_format_with_invalid_value_exits_two() {
    // Pins the unknown-`--format` value path: must exit 2 with a usage-error on
    // stderr naming the bad value and the valid choices.
    // The input positional is required because cmd_doc's missing-input check
    // runs *before* format resolution, so omitting it would test the wrong branch.
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = run_doc(&["--format", "xml", &path]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --format xml must exit 2 (usage error).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unknown --format value"),
        "stderr should contain 'unknown --format value', got: {stderr}"
    );
    assert!(
        stderr.contains("xml"),
        "stderr should name the offending value 'xml', got: {stderr}"
    );
    assert!(
        stderr.contains("expected html|markdown|json"),
        "stderr should guide the user to valid choices 'expected html|markdown|json', got: {stderr}"
    );
    assert!(
        stderr.contains("Usage: reify doc"),
        "stderr should contain 'Usage: reify doc' (DOC_USAGE line), got: {stderr}"
    );
}

#[test]
fn doc_o_without_value_exits_two() {
    // Pins the `-o` requires-a-value branch in cmd_doc's arg loop.
    let (status, stdout, stderr) = run_doc(&["-o"]);

    assert_eq!(
        status.code(),
        Some(2),
        "reify doc -o with no path must exit 2.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("-o requires a path"),
        "stderr should contain '-o requires a path', got: {stderr}"
    );
}

#[test]
fn doc_split_markdown_writes_files_to_directory() {
    // bracket.ri declares `structure Bracket` — render_split emits index.md
    // plus one per-item file named `structure-Bracket.md`.
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
    let mut entries: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read tmp dir")
        .map(|e| {
            e.expect("read entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    // Real DocModel from build_doc_model has the Bracket structure item, so
    // render_split must emit both index.md and structure-Bracket.md.
    assert!(
        entries.contains(&"index.md".to_string()),
        "expected index.md in {}, got: {entries:?}",
        dir.path().display()
    );
    assert!(
        entries.contains(&"structure-Bracket.md".to_string()),
        "expected structure-Bracket.md in {}, got: {entries:?}",
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

// ---------------------------------------------------------------------------
// --stdlib / --out tests (step-5 / task-3565)
// ---------------------------------------------------------------------------

/// Helper: return a unique temp dir path for a test.  Creates a fresh
/// directory under the system temp dir; the caller is responsible for the
/// lifetime (we don't clean up in these tests since they run isolated).
fn stdlib_out_dir(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("reify-test-stdlib-doc-{suffix}"));
    // Remove any stale remnant then create fresh.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp out dir");
    dir
}

/// `reify doc --stdlib --out <dir>` must exit 0, write `<dir>/index.html`
/// whose contents include ElasticMaterial, Bounded, and Manifold, and write
/// at least one other `*.html` file under `<dir>` (a per-symbol page).
#[test]
fn doc_stdlib_produces_html_pages() {
    let out_dir = stdlib_out_dir("produces");
    let dir_str = out_dir.to_string_lossy().into_owned();
    let (status, stdout, stderr) = run_doc(&["--stdlib", "--out", &dir_str]);

    assert_eq!(
        status.code(),
        Some(0),
        "reify doc --stdlib --out <dir> must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // index.html must exist and contain the known symbol names.
    let index_path = out_dir.join("index.html");
    assert!(
        index_path.exists(),
        "index.html must be created at {:?}",
        index_path
    );
    let index_contents = std::fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("read index.html failed: {e}"));
    assert!(
        index_contents.contains("ElasticMaterial"),
        "index.html must contain 'ElasticMaterial'; got (truncated):\n{}",
        &index_contents[..index_contents.len().min(3000)]
    );
    assert!(
        index_contents.contains("Bounded"),
        "index.html must contain 'Bounded'; got (truncated):\n{}",
        &index_contents[..index_contents.len().min(3000)]
    );
    assert!(
        index_contents.contains("Manifold"),
        "index.html must contain 'Manifold'; got (truncated):\n{}",
        &index_contents[..index_contents.len().min(3000)]
    );

    // At least one per-symbol .html file must exist somewhere under <dir>.
    let html_files: Vec<_> = walkdir_html(&out_dir)
        .into_iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("index.html"))
        .collect();
    assert!(
        !html_files.is_empty(),
        "expected at least one per-symbol .html page under {:?}; only index.html found",
        out_dir
    );
}

/// Recursive walk helper: collect all .html files under `dir`.
fn walkdir_html(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(walkdir_html(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("html") {
                results.push(path);
            }
        }
    }
    results
}

/// `reify doc --stdlib` without `--out` must exit 2 and print the usage hint.
#[test]
fn doc_stdlib_without_out_exits_two() {
    let (status, _stdout, stderr) = run_doc(&["--stdlib"]);
    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --stdlib without --out must exit 2.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Usage: reify doc"),
        "stderr must contain 'Usage: reify doc'; got: {stderr}"
    );
}

/// `reify doc --stdlib --out <dir> --format json` must exit 2 because
/// --stdlib is HTML-only.
#[test]
fn doc_stdlib_rejects_json_format() {
    let out_dir = stdlib_out_dir("rejects-json");
    let dir_str = out_dir.to_string_lossy().into_owned();
    let (status, _stdout, stderr) = run_doc(&["--stdlib", "--out", &dir_str, "--format", "json"]);
    assert_eq!(
        status.code(),
        Some(2),
        "reify doc --stdlib --format json must exit 2.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Usage: reify doc"),
        "stderr must contain 'Usage: reify doc'; got: {stderr}"
    );
}

//! `reify doc <file>`: renders the module's documentation as HTML, Markdown or JSON.

use std::process::ExitCode;

use reify_core::Severity;

use crate::args::doc_usage_error;
use crate::parse_and_compile;

/// Output format for `reify doc`.
///
/// Default is `Html` per the PRD; the `--format` flag accepts `html`,
/// `markdown`, or `json`.  Bad values exit 1 with a usage error written to
/// stderr; the match is inline in `cmd_doc` since it has only one call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Markdown,
    Json,
}

pub(crate) fn cmd_doc(args: &[String]) -> ExitCode {
    // Mirrors `cmd_gui`'s explicit-flag pattern: walk args, accept the
    // documented flags, and reject any other `--`-prefixed token with a
    // usage error.  The first non-flag positional is the input path; a
    // second positional is rejected as a usage error.
    let mut format: Option<String> = None;
    let mut output: Option<String> = None;
    let mut split = false;
    let mut compact = false;
    let mut stdlib = false;
    let mut input: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--split" => {
                split = true;
                i += 1;
            }
            "--compact" => {
                compact = true;
                i += 1;
            }
            "--stdlib" => {
                stdlib = true;
                i += 1;
            }
            "--format" => {
                if i + 1 >= args.len() {
                    return doc_usage_error("--format requires a value");
                }
                format = Some(args[i + 1].clone());
                i += 2;
            }
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    return doc_usage_error(format_args!("{a} requires a path"));
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                return doc_usage_error(format_args!("unknown flag for `doc`: {flag}"));
            }
            _ => {
                if input.is_some() {
                    return doc_usage_error(format_args!(
                        "unexpected extra positional argument: {a}"
                    ));
                }
                input = Some(a);
                i += 1;
            }
        }
    }

    // --stdlib mode: HTML-only, directory-output-only.  Guard all conflicting
    // flags before doing any compilation work.
    if stdlib {
        if output.is_none() {
            return doc_usage_error("--stdlib requires --out <dir>");
        }
        if input.is_some() {
            return doc_usage_error("--stdlib does not accept an input file positional");
        }
        if split {
            return doc_usage_error("--split is not valid with --stdlib");
        }
        if compact {
            return doc_usage_error("--compact is not valid with --stdlib");
        }
        if !matches!(format.as_deref(), None | Some("html")) {
            return doc_usage_error("--stdlib only supports --format html (the default)");
        }
        // Build the stdlib doc model, render multi-page HTML, and write files.
        let model = reify_doc_build::build_stdlib_doc_model();
        // Cross-refs (trait conformance) are omitted for now: build_cross_refs
        // operates on a single module's templates while the stdlib spans many.
        // A combined cross-refs pass is deferred to a follow-up task.
        let pages = reify_doc::fmt_html::render_html_pages(&model, None);
        let out_dir = std::path::PathBuf::from(output.as_deref().unwrap());
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("Error writing {}: {}", out_dir.display(), e);
            return ExitCode::FAILURE;
        }
        for (name, body) in pages {
            let file_path = out_dir.join(&name);
            if let Some(parent) = file_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!("Error writing {}: {}", parent.display(), e);
                return ExitCode::FAILURE;
            }
            if let Err(e) = std::fs::write(&file_path, body.as_bytes()) {
                eprintln!("Error writing {}: {}", file_path.display(), e);
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    let Some(input) = input else {
        return doc_usage_error("missing input file");
    };

    // Resolve `--format` (default `html`) into a typed `Format`.  Bad values
    // exit 1 with a usage-error on stderr.
    let format = match format.as_deref() {
        Some("html") => Format::Html,
        Some("markdown") => Format::Markdown,
        Some("json") => Format::Json,
        Some(other) => {
            return doc_usage_error(format_args!(
                "unknown --format value: {other} (expected html|markdown|json)"
            ));
        }
        None => Format::Html,
    };

    // `--split` is markdown-only.  Reject json/html + split before doing any
    // expensive parse/compile work so usage errors are fast and stderr stays
    // crisp.
    if split && format != Format::Markdown {
        return doc_usage_error("--split is only valid with --format markdown");
    }

    // `--compact` is json-only.  Mirror the `--split` guard.
    if compact && format != Format::Json {
        return doc_usage_error("--compact is only valid with --format json");
    }

    // `--split` requires `-o <dir>` so we know where to write the per-item
    // files.  Hoisted above `parse_and_compile` so usage errors don't pay for parsing.
    // Reachable only when format == Markdown thanks to the guard above.
    if split && output.is_none() {
        return doc_usage_error("--split requires -o <directory>");
    }

    let compiled = match parse_and_compile(input) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return ExitCode::FAILURE;
    }

    // Read the source file so build_doc_model can slice SourceSpan offsets
    // into the source string for constraint expr_repr and line numbers.
    // parse_and_compile already read and validated the file, so a second
    // read error is unexpected but handled consistently with the existing
    // `Error reading {path}: {e}` convention.
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input, e);
            return ExitCode::FAILURE;
        }
    };

    let model = reify_doc_build::build_doc_model(&compiled, &source);
    let xrefs = reify_doc_build::cross_refs::build_cross_refs(&compiled.templates);

    match format {
        Format::Json => {
            // The JSON formatter has no trailing newline of its own; we add
            // one to keep shell output tidy in stdout mode.  The `-o <file>`
            // write does NOT add the trailing newline so the file body is
            // exactly the formatter output.
            let body = reify_doc::fmt_json::render_json(&model, compact);
            write_single_file_or_stdout(output.as_deref(), &body, /*trailing_newline=*/ true)
        }
        Format::Markdown => {
            let opts = reify_doc::fmt_markdown::MarkdownOptions { split };
            let rendered = reify_doc::fmt_markdown::render_markdown(&model, Some(&xrefs), &opts);
            match rendered {
                reify_doc::fmt_markdown::MarkdownOutput::Single(body) => {
                    write_single_file_or_stdout(
                        output.as_deref(),
                        &body,
                        /*trailing_newline=*/ false,
                    )
                }
                reify_doc::fmt_markdown::MarkdownOutput::Split(files) => {
                    // The `--split requires -o <dir>` guard runs in the early
                    // usage-validation block above, so by the time we get here
                    // `output` is guaranteed `Some`.  `expect` rather than
                    // `unwrap` so a future refactor that bypasses the guard
                    // panics with a loud, attributable message instead of
                    // silently writing to a wrong path.
                    let dir = std::path::PathBuf::from(output.as_deref().expect(
                        "--split + --format markdown without -o is rejected by the early \
                             usage-validation block; reaching this branch means that guard was \
                             accidentally bypassed",
                    ));
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("Error writing {}: {}", dir.display(), e);
                        return ExitCode::FAILURE;
                    }
                    for (name, body) in files {
                        let file_path = dir.join(&name);
                        if let Err(e) = std::fs::write(&file_path, body.as_bytes()) {
                            eprintln!("Error writing {}: {}", file_path.display(), e);
                            return ExitCode::FAILURE;
                        }
                    }
                    ExitCode::SUCCESS
                }
            }
        }
        Format::Html => {
            // Default + explicit `--format html`: emit the real HTML formatter output.
            let body = reify_doc::fmt_html::render_html(&model, Some(&xrefs));
            write_single_file_or_stdout(output.as_deref(), &body, /*trailing_newline=*/ false)
        }
    }
}

/// Write `body` to `target` (when `Some`) or stdout (when `None`).
///
/// On stdout mode, appends a single `'\n'` after `body` iff `trailing_newline`
/// is true so JSON output ends in a newline (matches `cmd_check`'s
/// `println!` style and keeps shell output tidy).  On file-write mode the
/// trailing newline is *not* added; the on-disk body is exactly the
/// formatter output so it round-trips cleanly through tools that expect
/// canonical content.
///
/// Mirrors `cmd_build`'s `Error writing {path}: {e}` stderr format on I/O
/// failure; returns `ExitCode::FAILURE` (1) on write errors.
fn write_single_file_or_stdout(
    target: Option<&str>,
    body: &str,
    trailing_newline: bool,
) -> ExitCode {
    match target {
        Some(path) => {
            if let Err(e) = std::fs::write(path, body.as_bytes()) {
                eprintln!("Error writing {}: {}", path, e);
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        None => {
            if trailing_newline {
                println!("{body}");
            } else {
                print!("{body}");
            }
            ExitCode::SUCCESS
        }
    }
}

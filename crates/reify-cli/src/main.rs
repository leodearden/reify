use std::process::ExitCode;
use std::sync::Arc;

use reify_constraints::SimpleConstraintChecker;
use reify_eval::TestStatus;

mod mcp_context;
use reify_geometry::DispatchPlanner;
use reify_kernel_occt::OcctKernelHandle;
use reify_types::{ExportFormat, ModulePath, Satisfaction, Severity};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: reify <command> [options]");
        eprintln!("Commands:");
        eprintln!("  check <file>              Check constraints");
        eprintln!("  test <file>               Run @test-annotated structures");
        eprintln!("  build <file> -o <output>   Build geometry and export");
        eprintln!("  lsp                        Start language server (stdin/stdout)");
        eprintln!("  gui [--debug] <file>       Open file in GUI (--debug enables MCP debug listener)");
        eprintln!("  gui-debug <file>           Open file in GUI with debug MCP listener (alias for `gui --debug`)");
        eprintln!("  mcp-server [file] [--project-dir <dir>]  Start MCP server (stdin/stdout)");
        eprintln!("  doc <file> [-o <path>] [--format html|markdown|json] [--split] [--compact]  Generate documentation");
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "check" => cmd_check(&args[2..]),
        "test" => cmd_test(&args[2..]),
        "build" => cmd_build(&args[2..]),
        "doc" => cmd_doc(&args[2..]),
        "lsp" => cmd_lsp(),
        "gui" => cmd_gui(&args[2..]),
        "gui-debug" => {
            // `gui-debug` is sugar for `gui --debug`: prepend the flag and
            // route through the same code path as `cmd_gui` so the two entry
            // points share argument parsing and binary-launch logic.
            let mut forwarded: Vec<String> = Vec::with_capacity(args.len() - 1);
            forwarded.push("--debug".to_string());
            forwarded.extend(args[2..].iter().cloned());
            cmd_gui(&forwarded)
        }
        "mcp-server" => cmd_mcp_server(&args[2..]),
        other => {
            eprintln!("Unknown command: {}", other);
            ExitCode::FAILURE
        }
    }
}

fn parse_and_compile(path: &str) -> Result<reify_compiler::CompiledModule, ExitCode> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path, e);
            return Err(ExitCode::FAILURE);
        }
    };

    let module_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed");

    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));

    if !parsed.errors.is_empty() {
        for err in &parsed.errors {
            eprintln!("Parse error: {}", err.message);
        }
        return Err(ExitCode::FAILURE);
    }

    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    for diag in &compiled.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    Ok(compiled)
}

fn cmd_check(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify check <file>");
        return ExitCode::FAILURE;
    }

    let compiled = match parse_and_compile(&args[0]) {
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let outcome = report_eval_output(
        &result.constraint_results,
        &result.diagnostics,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );

    match outcome {
        ConstraintOutcome::AllSatisfied => {
            println!("All constraints satisfied.");
            ExitCode::SUCCESS
        }
        ConstraintOutcome::SomeIndeterminate(n) => {
            println!("No constraints violated ({n} indeterminate).");
            ExitCode::SUCCESS
        }
        ConstraintOutcome::SomeViolated => {
            println!("Some constraints violated.");
            ExitCode::FAILURE
        }
    }
}

fn cmd_test(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify test <file>");
        return ExitCode::FAILURE;
    }

    let compiled = match parse_and_compile(&args[0]) {
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

    let results = reify_eval::run_tests(&compiled, || Box::new(SimpleConstraintChecker));

    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut indeterminate: usize = 0;

    for result in &results {
        let label = match result.status {
            TestStatus::Pass => {
                passed += 1;
                "PASS"
            }
            TestStatus::Fail => {
                failed += 1;
                "FAIL"
            }
            TestStatus::Indeterminate => {
                indeterminate += 1;
                "INDETERMINATE"
            }
        };
        println!("  {}  {}", label, result.name);
    }

    let overall = if failed > 0 { "FAIL" } else { "ok" };
    println!(
        "test result: {}. {} passed; {} failed; {} indeterminate",
        overall, passed, failed, indeterminate
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_build(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify build <file> -o <output>");
        return ExitCode::FAILURE;
    }

    let file = &args[0];
    let output_path = match args.iter().position(|a| a == "-o") {
        Some(i) if i + 1 < args.len() => &args[i + 1],
        _ => {
            eprintln!("Usage: reify build <file> -o <output>");
            return ExitCode::FAILURE;
        }
    };

    let format = if output_path.ends_with(".step") || output_path.ends_with(".stp") {
        ExportFormat::Step
    } else if output_path.ends_with(".stl") {
        ExportFormat::Stl
    } else {
        eprintln!("Unknown output format, defaulting to STEP");
        ExportFormat::Step
    };

    let compiled = match parse_and_compile(file) {
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

    let checker = SimpleConstraintChecker;
    let mut planner = DispatchPlanner::new();
    planner.register_kernel(Box::new(OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, format);

    let outcome = report_eval_output(
        &result.constraint_results,
        &result.diagnostics,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );

    match result.geometry_output {
        Some(data) => {
            if let Err(e) = std::fs::write(output_path, &data) {
                eprintln!("Error writing {}: {}", output_path, e);
                return ExitCode::FAILURE;
            }
            println!("Wrote {} ({} bytes)", output_path, data.len());
            match outcome {
                ConstraintOutcome::AllSatisfied => ExitCode::SUCCESS,
                ConstraintOutcome::SomeIndeterminate(n) => {
                    println!("No constraints violated ({n} indeterminate).");
                    ExitCode::SUCCESS
                }
                ConstraintOutcome::SomeViolated => {
                    println!("Some constraints violated.");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            eprintln!("No geometry output produced");
            ExitCode::FAILURE
        }
    }
}

/// Usage line printed to stderr for any `reify doc` usage error.
const DOC_USAGE: &str =
    "Usage: reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]";

/// Output format for `reify doc`.
///
/// Default is `Html` per the PRD; the `--format` flag accepts `html`,
/// `markdown`, or `json`.  Bad values exit 2 with a usage error written to
/// stderr; the match is inline in `cmd_doc` since it has only one call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Markdown,
    Json,
}

/// Build a near-empty but well-formed [`reify_doc::model::DocModel`] from a
/// compiled module.
///
/// This is a deliberate placeholder that preserves only the module path so
/// the `reify doc` CLI pipeline (formatter dispatch, output plumbing,
/// usage/error handling) can be exercised end-to-end without depending on
/// the full lowering pass that would walk `compiled.templates`,
/// `compiled.functions`, etc.
///
/// TODO(post-2361): replace with `reify_doc_build::build_doc_model` when
/// slice 2 of the reify-doc PRD lands.  See `docs/prds/reify-doc-tool.md`
/// and the `scope_caveat="build_doc_model_not_yet_implemented"` notes on
/// sibling tasks 2351/2355/2357/2359.
fn minimal_doc_model_from_compiled(
    compiled: &reify_compiler::CompiledModule,
) -> reify_doc::model::DocModel {
    reify_doc::model::DocModel {
        modules: vec![reify_doc::model::ModuleDoc {
            path: compiled.path.to_string(),
            ..Default::default()
        }],
    }
}

/// Escape the five HTML-significant characters (`&`, `<`, `>`, `"`, `'`) so
/// that arbitrary text — including the markdown body we wrap inside `<pre>` —
/// cannot break out of the HTML structure.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render a self-contained HTML page wrapping the markdown body inside a
/// `<pre>` block.
///
/// This is a deliberate placeholder so the spec-mandated `--format html`
/// default has *some* HTML-shaped output end-to-end without preempting the
/// real formatter.
///
/// TODO(post-2361): replace with `reify_doc::fmt_html::render_html` when
/// task 2359 lands the dedicated HTML formatter (`fmt_html.rs`).
fn render_html_stub(model: &reify_doc::model::DocModel) -> String {
    let md_body = match reify_doc::fmt_markdown::render_markdown(
        model,
        None,
        &reify_doc::fmt_markdown::MarkdownOptions::default(),
    ) {
        reify_doc::fmt_markdown::MarkdownOutput::Single(s) => s,
        // `render_markdown` only emits `Split` when `opts.split == true`, and
        // we always pass `MarkdownOptions::default()` (split = false).  Use
        // `unreachable!` so a future refactor that breaks this invariant
        // panics loudly instead of silently emitting an empty `<pre>` block.
        reify_doc::fmt_markdown::MarkdownOutput::Split(_) => unreachable!(
            "render_html_stub always uses MarkdownOptions::default() (split = false)"
        ),
    };
    let path = model
        .modules
        .first()
        .map(|m| m.path.as_str())
        .unwrap_or("");
    let escaped_path = escape_html(path);
    let escaped_body = escape_html(&md_body);
    format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"><title>{escaped_path}</title></head>\n<body>\n<pre>\n{escaped_body}\n</pre>\n</body>\n</html>\n"
    )
}

fn cmd_doc(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

    // Mirrors `cmd_gui`'s explicit-flag pattern: walk args, accept the
    // documented flags, and reject any other `--`-prefixed token with a
    // usage error.  The first non-flag positional is the input path; a
    // second positional is rejected as a usage error.
    let mut format: Option<String> = None;
    let mut output: Option<String> = None;
    let mut split = false;
    let mut compact = false;
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
            "--format" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --format requires a value");
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
                }
                format = Some(args[i + 1].clone());
                i += 2;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -o requires a path");
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
                }
                output = Some(args[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `doc`: {}", flag);
                eprintln!("{}", DOC_USAGE);
                return ExitCode::from(2u8);
            }
            _ => {
                if input.is_some() {
                    eprintln!("Error: unexpected extra positional argument: {}", a);
                    eprintln!("{}", DOC_USAGE);
                    return ExitCode::from(2u8);
                }
                input = Some(a);
                i += 1;
            }
        }
    }

    let input = match input {
        Some(s) => s,
        None => {
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
    };

    // Resolve `--format` (default `html`) into a typed `Format`.  Bad values
    // exit 2 with a usage-error on stderr.
    let format = match format.as_deref() {
        Some("html") => Format::Html,
        Some("markdown") => Format::Markdown,
        Some("json") => Format::Json,
        Some(other) => {
            eprintln!(
                "Error: unknown --format value: {} (expected html|markdown|json)",
                other
            );
            eprintln!("{}", DOC_USAGE);
            return ExitCode::from(2u8);
        }
        None => Format::Html,
    };

    // `--split` is markdown-only.  Reject json/html + split before doing any
    // expensive parse/compile work so usage errors are fast and stderr stays
    // crisp.
    if split && format != Format::Markdown {
        eprintln!("Error: --split is only valid with --format markdown");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

    // `--compact` is json-only.  Mirror the `--split` guard.
    if compact && format != Format::Json {
        eprintln!("Error: --compact is only valid with --format json");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
    }

    // `--split` requires `-o <dir>` so we know where to write the per-item
    // files.  Hoisted above `parse_and_compile` so usage errors don't pay for parsing.
    // Reachable only when format == Markdown thanks to the guard above.
    debug_assert!(
        !split || format == Format::Markdown,
        "upstream guard should have rejected split with non-markdown format"
    );
    if split && output.is_none() {
        eprintln!("Error: --split requires -o <directory>");
        eprintln!("{}", DOC_USAGE);
        return ExitCode::from(2u8);
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

    let model = minimal_doc_model_from_compiled(&compiled);

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
            // TODO(post-2361): once `reify_doc_build::build_doc_model`
            // lands, wire `reify_doc_build::cross_refs::build_cross_refs(
            // &compiled.templates)` here and pass `Some(&xrefs)` instead of
            // `None`.  With the placeholder empty `DocModel`, cross-refs
            // would be degenerate, so `None` is byte-equivalent and saves a
            // workspace dep.
            let opts = reify_doc::fmt_markdown::MarkdownOptions { split };
            let rendered = reify_doc::fmt_markdown::render_markdown(&model, None, &opts);
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
                    let dir = std::path::PathBuf::from(
                        output.as_deref().expect(
                            "--split + --format markdown without -o is rejected by the early \
                             usage-validation block; reaching this branch means that guard was \
                             accidentally bypassed",
                        ),
                    );
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
            // Default + explicit `--format html`: emit the in-CLI HTML stub.
            let body = render_html_stub(&model);
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

fn cmd_gui(args: &[String]) -> ExitCode {
    // Parse `--debug` / `--mcp` flags (both set the same `debug` boolean) and
    // strip them from the positional args before extracting the file path.
    // Any other `--`-prefixed token is rejected explicitly so a typo like
    // `--debugg` fails loud instead of being silently treated as a file path.
    let mut debug = false;
    let mut positional: Vec<&String> = Vec::with_capacity(args.len());
    for a in args {
        match a.as_str() {
            "--debug" | "--mcp" => debug = true,
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `gui`: {}", flag);
                eprintln!("Usage: reify gui [--debug] <file>");
                return ExitCode::FAILURE;
            }
            _ => positional.push(a),
        }
    }

    if positional.is_empty() {
        eprintln!("Usage: reify gui [--debug] <file>");
        return ExitCode::FAILURE;
    }

    let file = positional[0].as_str();
    let path = std::path::Path::new(file);

    // Validate .ri extension (checked before existence to give a clear error for wrong file types)
    match path.extension().and_then(|e| e.to_str()) {
        Some("ri") => {}
        _ => {
            eprintln!("Error: file must have .ri extension: {}", file);
            return ExitCode::FAILURE;
        }
    }

    // Validate file exists
    if !path.exists() {
        eprintln!("Error: file does not exist: {}", file);
        return ExitCode::FAILURE;
    }

    // Check if launch is suppressed (for testing / CI). The user-facing error
    // is kept clean (no internal flag state). Tests that need to assert on the
    // parsed debug-mode set `REIFY_GUI_DEBUG_PROBE=1` to enable a structured
    // probe line — keeping the test seam off the default error path.
    if std::env::var("REIFY_GUI_SKIP_LAUNCH").is_ok() {
        if std::env::var("REIFY_GUI_DEBUG_PROBE").is_ok() {
            eprintln!("REIFY_GUI_DEBUG_PROBE: debug={}", debug);
        }
        eprintln!("Error: could not launch reify-gui (launch skipped via REIFY_GUI_SKIP_LAUNCH)");
        return ExitCode::FAILURE;
    }

    // Locate the reify-gui binary: same directory as this binary, then PATH
    let gui_binary_name = if cfg!(target_os = "windows") {
        "reify-gui.exe"
    } else {
        "reify-gui"
    };

    let gui_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(gui_binary_name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(gui_binary_name));

    let mut cmd = build_gui_command(&gui_path, file, debug);
    match cmd.status() {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!(
                "Error: could not launch reify-gui ({}): {}",
                gui_path.display(),
                e
            );
            ExitCode::FAILURE
        }
    }
}

/// Build a [`std::process::Command`] for launching `reify-gui` with the given
/// file argument and (optionally) `REIFY_DEBUG=1` set in the child's
/// environment when `debug` is true.
///
/// Extracted as a pure helper so it can be unit-tested via `Command::get_envs()`
/// without spawning a subprocess.
fn build_gui_command(
    gui_path: &std::path::Path,
    file: &str,
    debug: bool,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(gui_path);
    cmd.arg(file);
    if debug {
        cmd.env("REIFY_DEBUG", "1");
    }
    cmd
}

fn cmd_lsp() -> ExitCode {
    // Use a multi-thread runtime with a capped worker count.  A current-thread
    // runtime was tried (ceede7afc) to reduce startup latency, but tower-lsp
    // relies on `tokio::spawn` internally to drive request/response futures
    // concurrently with the stdin-reading loop.  With a single-threaded
    // executor those spawned futures may not be polled until the next I/O
    // yield, causing the initialize response to never arrive when the test
    // sends only one message.  Two worker threads is the minimum safe count:
    // one drives the serve loop, one drives handler/notification tasks.
    match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => {
            rt.block_on(reify_lsp::run_server());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to create async runtime: {}", e);
            ExitCode::FAILURE
        }
    }
}

/// Outcome of constraint checking.
#[derive(Debug, PartialEq)]
enum ConstraintOutcome {
    /// Every constraint evaluated to `Satisfied`.
    AllSatisfied,
    /// No constraints violated, but some were `Indeterminate` (undef inputs).
    SomeIndeterminate(usize),
    /// At least one constraint evaluated to `Violated`.
    SomeViolated,
}

/// Report constraint check results to the given writer.
///
/// Returns a [`ConstraintOutcome`] indicating the overall result.
/// Each entry is printed as `  {STATUS} {label}` where label falls back to the
/// constraint id's Display representation when `entry.label` is `None`.
///
/// **Indeterminate constraints are intentionally treated as non-violating.**
/// `Indeterminate` arises when a constraint's inputs are undefined — typically
/// from `auto` parameters not yet resolved by the solver. Treating these as
/// violations would block evaluations that are otherwise valid and break the
/// incremental evaluation engine. Only explicit `Violated` results cause
/// a `SomeViolated` outcome.
fn report_constraint_results(
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let mut violated = false;
    let mut indeterminate_count: usize = 0;
    for entry in results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "OK",
            Satisfaction::Violated => {
                violated = true;
                "VIOLATED"
            }
            // Indeterminate does not count as violated — undef inputs
            // (auto params, partial evaluation) are not violations.
            // Undef propagates as quiet-NaN semantics.
            Satisfaction::Indeterminate => {
                indeterminate_count += 1;
                "INDETERMINATE"
            }
        };
        let id_str = format!("{}", entry.id);
        let label = entry.label.as_deref().unwrap_or(&id_str);
        let _ = writeln!(out, "  {} {}", status, label);
    }
    if violated {
        ConstraintOutcome::SomeViolated
    } else if indeterminate_count > 0 {
        ConstraintOutcome::SomeIndeterminate(indeterminate_count)
    } else {
        ConstraintOutcome::AllSatisfied
    }
}

/// Report constraint results and eval diagnostics in a consistent order.
///
/// Writes constraint status lines to `out` (via [`report_constraint_results`]),
/// then writes each diagnostic to `err`. This ensures both `cmd_check` and
/// `cmd_build` produce output in the same order: constraints first, diagnostics
/// second.
fn report_eval_output(
    constraint_results: &[reify_eval::ConstraintCheckEntry],
    diagnostics: &[reify_types::Diagnostic],
    out: &mut impl std::io::Write,
    err: &mut impl std::io::Write,
) -> ConstraintOutcome {
    let outcome = report_constraint_results(constraint_results, out);
    for diag in diagnostics {
        let _ = writeln!(err, "{}: {}", diag.severity, diag.message);
    }
    outcome
}

fn cmd_mcp_server(args: &[String]) -> ExitCode {
    // Parse optional file argument and --project-dir flag
    let mut file_path: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--project-dir" {
            if i + 1 < args.len() {
                project_dir = Some(args[i + 1].clone());
                i += 2;
                continue;
            } else {
                eprintln!("--project-dir requires a value");
                return ExitCode::FAILURE;
            }
        } else if file_path.is_none() {
            file_path = Some(args[i].clone());
        }
        i += 1;
    }

    let project_dir = project_dir
        .map(std::path::PathBuf::from)
        .or_else(|| {
            file_path
                .as_ref()
                .and_then(|f| std::path::Path::new(f).parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let context = mcp_context::CliToolContext::new(project_dir);

    if let Some(ref path) = file_path
        && let Err(e) = context.load_file(path)
    {
        eprintln!("Error loading {}: {}", path, e);
        return ExitCode::FAILURE;
    }

    let server = reify_mcp::McpServer::new(Arc::new(context));

    match tokio::runtime::Runtime::new() {
        Ok(rt) => {
            rt.block_on(server.run_stdio());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to create async runtime: {}", e);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_eval::ConstraintCheckEntry;
    use reify_types::{ConstraintNodeId, Satisfaction};

    /// Helper: capture `report_constraint_results` output into an in-memory
    /// buffer and return the outcome plus the formatted output as a `String`.
    fn run_report(entries: &[ConstraintCheckEntry]) -> (ConstraintOutcome, String) {
        let mut buf = Vec::new();
        let result = report_constraint_results(entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        (result, output)
    }

    #[test]
    fn empty_entries_returns_all_satisfied_with_no_output() {
        let (result, output) = run_report(&[]);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "empty entries should return AllSatisfied (vacuous truth)"
        );
        assert!(
            output.is_empty(),
            "empty entries should produce no output, got: {:?}",
            output
        );
    }

    fn make_entry(
        entity: &str,
        index: u32,
        label: Option<&str>,
        satisfaction: Satisfaction,
    ) -> ConstraintCheckEntry {
        ConstraintCheckEntry {
            id: ConstraintNodeId::new(entity, index),
            label: label.map(|s| s.to_string()),
            satisfaction,
        }
    }

    #[test]
    fn all_satisfied_returns_true_and_formats_ok() {
        let entries = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Satisfied),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::AllSatisfied,
            "should return AllSatisfied when all satisfied"
        );
        assert!(output.contains("  OK stress_limit"));
        assert!(output.contains("  OK size_bound"));
        assert!(!output.contains("VIOLATED"));
    }

    #[test]
    fn violated_returns_false_and_formats_violated() {
        let entries = vec![
            make_entry("Part", 0, Some("max_force"), Satisfaction::Satisfied),
            make_entry("Part", 1, Some("clearance"), Satisfaction::Violated),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when any violated"
        );
        assert!(output.contains("  OK max_force"));
        assert!(output.contains("VIOLATED clearance"));
    }

    #[test]
    fn indeterminate_formats_correctly_and_counts_as_satisfied() {
        let entries = vec![make_entry(
            "Beam",
            0,
            Some("load"),
            Satisfaction::Indeterminate,
        )];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeIndeterminate(1),
            "indeterminate should return SomeIndeterminate with count"
        );
        assert!(output.contains("INDETERMINATE load"));
    }

    #[test]
    fn violated_with_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Bracket", 0, Some("thickness"), Satisfaction::Violated),
            make_entry("Bracket", 1, Some("tolerance"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "should return SomeViolated when violated + indeterminate coexist"
        );
        assert!(
            output.contains("VIOLATED thickness"),
            "output should contain 'VIOLATED thickness', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE tolerance"),
            "output should contain 'INDETERMINATE tolerance', got: {}",
            output
        );
        assert!(
            !output.contains("  OK "),
            "output should NOT contain '  OK ' when no constraints are satisfied, got: {}",
            output
        );
    }

    #[test]
    fn three_way_satisfied_violated_indeterminate_returns_some_violated() {
        let entries = vec![
            make_entry("Assembly", 0, Some("weight_limit"), Satisfaction::Satisfied),
            make_entry("Assembly", 1, Some("clearance"), Satisfaction::Violated),
            make_entry("Assembly", 2, Some("thermal"), Satisfaction::Indeterminate),
        ];
        let (result, output) = run_report(&entries);

        assert_eq!(
            result,
            ConstraintOutcome::SomeViolated,
            "violated takes priority over indeterminate: should return SomeViolated"
        );
        assert!(
            output.contains("  OK weight_limit"),
            "output should contain '  OK weight_limit', got: {}",
            output
        );
        assert!(
            output.contains("VIOLATED clearance"),
            "output should contain 'VIOLATED clearance', got: {}",
            output
        );
        assert!(
            output.contains("INDETERMINATE thermal"),
            "output should contain 'INDETERMINATE thermal', got: {}",
            output
        );
    }

    #[test]
    fn uses_id_display_as_fallback_when_label_is_none() {
        let entries = vec![make_entry("Gear", 2, None, Satisfaction::Satisfied)];
        let (_result, output) = run_report(&entries);

        // ConstraintNodeId Display: "Gear#constraint[2]"
        assert!(
            output.contains("  OK Gear#constraint[2]"),
            "should use id Display as fallback, got: {}",
            output
        );
    }

    #[test]
    fn uses_label_when_present() {
        let entries = vec![make_entry(
            "Axle",
            0,
            Some("torque_limit"),
            Satisfaction::Violated,
        )];
        let (_result, output) = run_report(&entries);

        assert!(
            output.contains("VIOLATED torque_limit"),
            "should use label, got: {}",
            output
        );
        assert!(
            !output.contains("Axle#constraint"),
            "should NOT contain id fallback when label is present"
        );
    }

    #[test]
    fn report_eval_output_writes_constraints_to_out_and_diagnostics_to_err() {
        let constraints = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Violated),
        ];
        let diagnostics = vec![
            reify_types::Diagnostic::warning("some msg"),
            reify_types::Diagnostic::error("bad thing"),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = report_eval_output(&constraints, &diagnostics, &mut out, &mut err);

        let out_str = String::from_utf8(out).unwrap();
        let err_str = String::from_utf8(err).unwrap();

        // (a) out buffer contains constraint status lines
        assert!(
            out_str.contains("  OK stress_limit"),
            "out should contain constraint OK line, got: {}",
            out_str
        );
        assert!(
            out_str.contains("VIOLATED size_bound"),
            "out should contain constraint VIOLATED line, got: {}",
            out_str
        );

        // (b) err buffer contains diagnostic lines
        assert!(
            err_str.contains("warning: some msg"),
            "err should contain warning diagnostic, got: {}",
            err_str
        );
        assert!(
            err_str.contains("error: bad thing"),
            "err should contain error diagnostic, got: {}",
            err_str
        );

        // (c) correct outcome
        assert_eq!(outcome, ConstraintOutcome::SomeViolated);
    }

    #[test]
    fn build_gui_command_sets_reify_debug_when_debug_true() {
        // Verifies that `build_gui_command(.., debug=true)` sets REIFY_DEBUG=1
        // in the child Command's env, without spawning a subprocess.
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", true);
        let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|val| val.to_os_string())))
            .collect();
        let reify_debug_set = envs.iter().any(|(k, v)| {
            k == std::ffi::OsStr::new("REIFY_DEBUG")
                && v.as_deref() == Some(std::ffi::OsStr::new("1"))
        });
        assert!(
            reify_debug_set,
            "REIFY_DEBUG=1 must be set in Command env when debug=true; got envs: {:?}",
            envs
        );
    }

    #[test]
    fn build_gui_command_does_not_set_reify_debug_when_debug_false() {
        // Verifies that `build_gui_command(.., debug=false)` does NOT add
        // REIFY_DEBUG to the child Command's env (parent env is inherited
        // automatically by the OS spawn machinery; we only assert that we
        // don't *override* it here).
        let path = std::path::Path::new("/tmp/fake-reify-gui");
        let cmd = build_gui_command(path, "x.ri", false);
        let has_reify_debug = cmd
            .get_envs()
            .any(|(k, _)| k == std::ffi::OsStr::new("REIFY_DEBUG"));
        assert!(
            !has_reify_debug,
            "REIFY_DEBUG must NOT be set in Command env when debug=false"
        );
    }

    #[test]
    fn report_eval_output_returns_correct_outcome_variants() {
        let no_diags: Vec<reify_types::Diagnostic> = vec![];

        // AllSatisfied: all constraints OK
        {
            let entries = vec![
                make_entry("A", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("A", 1, Some("c2"), Satisfaction::Satisfied),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::AllSatisfied);
        }

        // SomeViolated: at least one violated
        {
            let entries = vec![
                make_entry("B", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("B", 1, Some("c2"), Satisfaction::Violated),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeViolated);
        }

        // SomeIndeterminate: indeterminate but no violated
        {
            let entries = vec![
                make_entry("C", 0, Some("c1"), Satisfaction::Satisfied),
                make_entry("C", 1, Some("c2"), Satisfaction::Indeterminate),
                make_entry("C", 2, Some("c3"), Satisfaction::Indeterminate),
            ];
            let mut out = Vec::new();
            let mut err = Vec::new();
            let outcome = report_eval_output(&entries, &no_diags, &mut out, &mut err);
            assert_eq!(outcome, ConstraintOutcome::SomeIndeterminate(2));
        }
    }
}

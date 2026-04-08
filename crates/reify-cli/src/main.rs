use std::process::ExitCode;
use std::sync::Arc;

use reify_constraints::SimpleConstraintChecker;

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
        eprintln!("  build <file> -o <output>   Build geometry and export");
        eprintln!("  lsp                        Start language server (stdin/stdout)");
        eprintln!("  gui <file>                 Open file in GUI");
        eprintln!("  mcp-server [file] [--project-dir <dir>]  Start MCP server (stdin/stdout)");
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "check" => cmd_check(&args[2..]),
        "build" => cmd_build(&args[2..]),
        "lsp" => cmd_lsp(),
        "gui" => cmd_gui(&args[2..]),
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

    let compiled = reify_compiler::compile(&parsed);

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

fn cmd_gui(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: reify gui <file>");
        return ExitCode::FAILURE;
    }

    let file = &args[0];
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

    // Check if launch is suppressed (for testing / CI)
    if std::env::var("REIFY_GUI_SKIP_LAUNCH").is_ok() {
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

    match std::process::Command::new(&gui_path).arg(file).status() {
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

fn cmd_lsp() -> ExitCode {
    match tokio::runtime::Runtime::new() {
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

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

    let all_satisfied =
        report_constraint_results(&result.constraint_results, &mut std::io::stdout());

    for diag in &result.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    println!("{}", constraint_summary_message(&result.constraint_results, all_satisfied));

    if all_satisfied {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
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

    for diag in &result.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    // Report constraint status
    let all_satisfied =
        report_constraint_results(&result.constraint_results, &mut std::io::stdout());

    match result.geometry_output {
        Some(data) => {
            if let Err(e) = std::fs::write(output_path, &data) {
                eprintln!("Error writing {}: {}", output_path, e);
                return ExitCode::FAILURE;
            }
            println!("Wrote {} ({} bytes)", output_path, data.len());
            if all_satisfied {
                ExitCode::SUCCESS
            } else {
                println!("Some constraints violated.");
                ExitCode::FAILURE
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

    // Validate file exists
    if !path.exists() {
        eprintln!("Error: file does not exist: {}", file);
        return ExitCode::FAILURE;
    }

    // Validate .ri extension
    match path.extension().and_then(|e| e.to_str()) {
        Some("ri") => {}
        _ => {
            eprintln!("Error: file must have .ri extension: {}", file);
            return ExitCode::FAILURE;
        }
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

/// Return the appropriate summary message for constraint results.
///
/// - If `no_violations` is false, returns "Some constraints violated."
/// - If `no_violations` is true but some entries are `Indeterminate`, returns
///   "No constraint violations (some indeterminate)."
/// - Otherwise (all truly satisfied), returns "All constraints satisfied."
fn constraint_summary_message(
    results: &[reify_eval::ConstraintCheckEntry],
    no_violations: bool,
) -> &'static str {
    if !no_violations {
        "Some constraints violated."
    } else if results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Indeterminate)
    {
        "No constraint violations (some indeterminate)."
    } else {
        "All constraints satisfied."
    }
}

/// Report constraint check results to the given writer.
///
/// Returns `true` if all constraints are satisfied, `false` otherwise.
/// Each entry is printed as `  {STATUS} {label}` where label falls back to the
/// constraint id's Display representation when `entry.label` is `None`.
///
/// **Indeterminate constraints are intentionally treated as non-violating.**
/// `Indeterminate` arises when a constraint's inputs are undefined — typically
/// from `auto` parameters not yet resolved by the solver. Treating these as
/// violations would block evaluations that are otherwise valid and break the
/// incremental evaluation engine. Only explicit `Violated` results cause
/// `all_satisfied` to be `false`.
fn report_constraint_results(
    results: &[reify_eval::ConstraintCheckEntry],
    out: &mut impl std::io::Write,
) -> bool {
    let mut all_satisfied = true;
    for entry in results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "OK",
            Satisfaction::Violated => {
                all_satisfied = false;
                "VIOLATED"
            }
            // Indeterminate does not set all_satisfied=false — undef inputs
            // (auto params, partial evaluation) are not violations.
            // Undef propagates as quiet-NaN semantics.
            Satisfaction::Indeterminate => "INDETERMINATE",
        };
        let id_str = format!("{}", entry.id);
        let label = entry.label.as_deref().unwrap_or(&id_str);
        let _ = writeln!(out, "  {} {}", status, label);
    }
    all_satisfied
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
        let mut buf = Vec::new();
        let result = report_constraint_results(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        assert!(result, "should return true when all satisfied");
        assert!(output.contains("OK stress_limit"));
        assert!(output.contains("OK size_bound"));
        assert!(!output.contains("VIOLATED"));
    }

    #[test]
    fn violated_returns_false_and_formats_violated() {
        let entries = vec![
            make_entry("Part", 0, Some("max_force"), Satisfaction::Satisfied),
            make_entry("Part", 1, Some("clearance"), Satisfaction::Violated),
        ];
        let mut buf = Vec::new();
        let result = report_constraint_results(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        assert!(!result, "should return false when any violated");
        assert!(output.contains("OK max_force"));
        assert!(output.contains("VIOLATED clearance"));
    }

    #[test]
    fn indeterminate_formats_correctly_and_counts_as_satisfied() {
        let entries = vec![
            make_entry("Beam", 0, Some("load"), Satisfaction::Indeterminate),
        ];
        let mut buf = Vec::new();
        let result = report_constraint_results(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        assert!(result, "indeterminate should not cause false return");
        assert!(output.contains("INDETERMINATE load"));
    }

    #[test]
    fn uses_id_display_as_fallback_when_label_is_none() {
        let entries = vec![
            make_entry("Gear", 2, None, Satisfaction::Satisfied),
        ];
        let mut buf = Vec::new();
        let _result = report_constraint_results(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

        // ConstraintNodeId Display: "Gear#constraint[2]"
        assert!(
            output.contains("OK Gear#constraint[2]"),
            "should use id Display as fallback, got: {}",
            output
        );
    }

    #[test]
    fn uses_label_when_present() {
        let entries = vec![
            make_entry("Axle", 0, Some("torque_limit"), Satisfaction::Violated),
        ];
        let mut buf = Vec::new();
        let _result = report_constraint_results(&entries, &mut buf);
        let output = String::from_utf8(buf).unwrap();

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
    fn summary_message_all_satisfied() {
        let entries = vec![
            make_entry("Bracket", 0, Some("stress_limit"), Satisfaction::Satisfied),
            make_entry("Bracket", 1, Some("size_bound"), Satisfaction::Satisfied),
        ];
        let msg = constraint_summary_message(&entries, true);
        assert_eq!(msg, "All constraints satisfied.");
    }

    #[test]
    fn summary_message_indeterminate_no_violations() {
        let entries = vec![
            make_entry("Beam", 0, Some("load"), Satisfaction::Satisfied),
            make_entry("Beam", 1, Some("deflection"), Satisfaction::Indeterminate),
        ];
        let msg = constraint_summary_message(&entries, true);
        assert_eq!(msg, "No constraint violations (some indeterminate).");
    }

    #[test]
    fn summary_message_violated() {
        let entries = vec![
            make_entry("Part", 0, Some("max_force"), Satisfaction::Violated),
            make_entry("Part", 1, Some("clearance"), Satisfaction::Indeterminate),
        ];
        let msg = constraint_summary_message(&entries, false);
        assert_eq!(msg, "Some constraints violated.");
    }
}

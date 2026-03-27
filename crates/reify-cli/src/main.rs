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

    if compiled.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return ExitCode::FAILURE;
    }

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let mut all_satisfied = true;
    for entry in &result.constraint_results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "OK",
            Satisfaction::Violated => {
                all_satisfied = false;
                "VIOLATED"
            }
            Satisfaction::Indeterminate => "INDETERMINATE",
        };
        let id_str = format!("{}", entry.id);
        let label = entry.label.as_deref().unwrap_or(&id_str);
        println!("  {} {}", status, label);
    }

    for diag in &result.diagnostics {
        eprintln!("{}: {}", diag.severity, diag.message);
    }

    if all_satisfied {
        println!("All constraints satisfied.");
        ExitCode::SUCCESS
    } else {
        println!("Some constraints violated.");
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

    if compiled.diagnostics.iter().any(|d| d.severity == Severity::Error) {
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
    let mut all_satisfied = true;
    for entry in &result.constraint_results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "OK",
            Satisfaction::Violated => {
                all_satisfied = false;
                "VIOLATED"
            }
            Satisfaction::Indeterminate => "INDETERMINATE",
        };
        let id_str = format!("{}", entry.id);
        let label = entry.label.as_deref().unwrap_or(&id_str);
        println!("  {} {}", status, label);
    }

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
        eprintln!(
            "Error: could not launch reify-gui (launch skipped via REIFY_GUI_SKIP_LAUNCH)"
        );
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

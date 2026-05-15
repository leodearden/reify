//! `reify-audit` CLI binary.
//!
//! Entry point for the `/audit` skill (T-5) and the dark-factory pre-done hook
//! (D-1). See `docs/architecture-audit/f-infra-design.md` §3 and §10.
//!
//! ## Modes
//!
//! - `reify-audit --task <id> --pre-done`  P5 only; exit non-zero on detection.
//! - `reify-audit --task <id>`             Spot-check, all three detectors.
//! - `reify-audit --since <iso-date>`      Window sweep, all three detectors.
//! - `--pattern P1|P2|P5`                  Restrict which detector(s) run.
//!
//! ## Output
//!
//! JSON array of [`Finding`]s on **stderr**; human-readable summary on
//! **stdout**. Exit code = high-severity count, capped at 255.
//!
//! ## Arg parsing
//!
//! Hand-rolled `std::env::args()` — mirrors `crates/reify-cli/src/main.rs` to
//! keep the workspace convention consistent and avoid pulling a new dependency
//! into `reify-audit`. See design §12 (minimal deps).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use reify_audit::{
    AuditContext, ChangedSymbol, Finding, JCodemunchOps, RealGitOps, Severity, SymbolReference,
    TaskMetadata, TimeWindow,
};

// -----------------------------------------------------------------------
// NoopJCodemunchOps — slice-1 stub per design §11 (D-1)
// -----------------------------------------------------------------------

/// Slice-1 no-op implementation of [`JCodemunchOps`].
///
/// The real jcodemunch-MCP-backed impl is D-1's concern. This stub keeps P1
/// quiet on production runs until D-1 lands. Per design §11 ("hookless slice
/// 1"). Never escapes this bin file; the library's `MockJCodemunchOps` remains
/// test-only via the `test-support` feature.
struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(&self, _branch: &str, _since_epoch: i64) -> Vec<ChangedSymbol> {
        vec![]
    }
    fn find_references(&self, _symbol_name: &str) -> Vec<SymbolReference> {
        vec![]
    }
}

// -----------------------------------------------------------------------
// Usage / help
// -----------------------------------------------------------------------

fn print_usage(out: &mut dyn Write) {
    let _ = writeln!(out, "Usage: reify-audit [OPTIONS]");
    let _ = writeln!(out);
    let _ = writeln!(out, "Options:");
    let _ = writeln!(out, "  --task <id>              Spot-check a single task (all detectors)");
    let _ = writeln!(out, "  --pre-done               With --task: run P5 pre-done check only");
    let _ = writeln!(out, "  --since <iso-date>       Window sweep from ISO date (all detectors)");
    let _ = writeln!(out, "  --pattern P1|P2|P5       Restrict to one detector");
    let _ = writeln!(out, "  --tasks-file <path>      JSON array of TaskMetadata (default: .taskmaster/tasks/tasks.json)");
    let _ = writeln!(out, "  --runs-db <path>         SQLite runs.db path (default: data/orchestrator/runs.db)");
    let _ = writeln!(out, "  --project-root <path>    Repo root for git operations (default: .)");
    let _ = writeln!(out, "  --help, -h               Show this help");
    let _ = writeln!(out, "  --version, -V            Print version");
    let _ = writeln!(out);
    let _ = writeln!(out, "Output:");
    let _ = writeln!(out, "  stderr: JSON array of Finding objects");
    let _ = writeln!(out, "  stdout: human-readable summary");
    let _ = writeln!(out, "  exit code: high-severity finding count, capped at 255");
    let _ = writeln!(out);
    let _ = writeln!(out, "Note: --tasks-file must be a JSON array of TaskMetadata objects");
    let _ = writeln!(out, "(all 9 fields required: task_id, status, files, done_provenance,");
    let _ = writeln!(out, " title, prd, consumer_ref, audit_foundation, done_at).");
}

// Use std::io::Write trait alias to accept both stdout and stderr.
use std::io::Write;

// -----------------------------------------------------------------------
// Exit-code helper
// -----------------------------------------------------------------------

/// Count High-severity findings and clamp to u8 (cap at 255).
fn high_severity_exit_code(findings: &[Finding]) -> u8 {
    let count = findings
        .iter()
        .filter(|f| f.severity == Severity::High)
        .count();
    count.min(255) as u8
}

// -----------------------------------------------------------------------
// Parsed CLI arguments
// -----------------------------------------------------------------------

struct Args {
    task_id: Option<String>,
    pre_done: bool,
    since: Option<String>,
    pattern: Option<String>,
    tasks_file: String,
    runs_db: String,
    project_root: String,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut task_id = None;
    let mut pre_done = false;
    let mut since = None;
    let mut pattern = None;
    let mut tasks_file = ".taskmaster/tasks/tasks.json".to_string();
    let mut runs_db = "data/orchestrator/runs.db".to_string();
    let mut project_root = ".".to_string();

    let mut i = 0usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "--task" => {
                i += 1;
                task_id = Some(
                    argv.get(i)
                        .ok_or("--task requires a value")?
                        .clone(),
                );
            }
            "--pre-done" => {
                pre_done = true;
            }
            "--since" => {
                i += 1;
                since = Some(
                    argv.get(i)
                        .ok_or("--since requires a value")?
                        .clone(),
                );
            }
            "--pattern" => {
                i += 1;
                let p = argv.get(i).ok_or("--pattern requires a value")?.as_str();
                match p {
                    "P1" | "P2" | "P5" => pattern = Some(p.to_string()),
                    other => {
                        return Err(format!(
                            "unknown --pattern value '{}'; expected P1, P2, or P5",
                            other
                        ))
                    }
                }
            }
            "--tasks-file" => {
                i += 1;
                tasks_file = argv
                    .get(i)
                    .ok_or("--tasks-file requires a value")?
                    .clone();
            }
            "--runs-db" => {
                i += 1;
                runs_db = argv
                    .get(i)
                    .ok_or("--runs-db requires a value")?
                    .clone();
            }
            "--project-root" => {
                i += 1;
                project_root = argv
                    .get(i)
                    .ok_or("--project-root requires a value")?
                    .clone();
            }
            other => {
                return Err(format!("unknown flag '{}'", other));
            }
        }
        i += 1;
    }

    Ok(Args { task_id, pre_done, since, pattern, tasks_file, runs_db, project_root })
}

// -----------------------------------------------------------------------
// Summary formatter
// -----------------------------------------------------------------------

fn print_summary(findings: &[Finding]) {
    if findings.is_empty() {
        println!("reify-audit: 0 findings.");
        return;
    }
    println!("reify-audit: {} finding(s):", findings.len());
    for f in findings {
        println!(
            "  [{:?}] {:?} task={}: {}",
            f.severity, f.pattern, f.task_id, f.summary
        );
    }
}

// -----------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    // Help / version shortcuts (checked before full parse so they always work).
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print_usage(&mut std::io::stdout());
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("reify-audit {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if argv.is_empty() {
        print_usage(&mut std::io::stderr());
        return ExitCode::from(2);
    }

    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("reify-audit: error: {}", e);
            print_usage(&mut std::io::stderr());
            return ExitCode::from(2);
        }
    };

    // --pre-done requires --task.
    if args.pre_done && args.task_id.is_none() {
        eprintln!("reify-audit: error: --pre-done requires --task");
        return ExitCode::from(2);
    }

    // Load tasks.json.
    let tasks_json = match std::fs::read_to_string(&args.tasks_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("reify-audit: error reading tasks-file '{}': {}", args.tasks_file, e);
            return ExitCode::from(2);
        }
    };
    let tasks_vec: Vec<TaskMetadata> = match serde_json::from_str(&tasks_json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "reify-audit: error parsing tasks-file '{}': {}",
                args.tasks_file, e
            );
            return ExitCode::from(2);
        }
    };
    let task_metadata: HashMap<String, TaskMetadata> = tasks_vec
        .into_iter()
        .map(|t| (t.task_id.clone(), t))
        .collect();

    // Open runs.db.
    let conn = match rusqlite::Connection::open(&args.runs_db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("reify-audit: error opening runs-db '{}': {}", args.runs_db, e);
            return ExitCode::from(2);
        }
    };

    // Construct seam impls.
    let git = RealGitOps::new(PathBuf::from(&args.project_root));
    let jcodemunch = NoopJCodemunchOps;

    // Build window (for --since).
    let window = args.since.as_ref().map(|s| TimeWindow {
        since: Some(s.clone()),
        until: None,
    });

    // Build context.
    let ctx = AuditContext {
        project_root: PathBuf::from(&args.project_root),
        conn: &conn,
        git: &git,
        jcodemunch: &jcodemunch,
        task_metadata,
        target_task_id: args.task_id.clone(),
        window,
        now: None,
    };

    // Dispatch.
    let findings: Vec<Finding> = if args.pre_done {
        // --task <id> --pre-done: P5 only via check_pre_done.
        let task_id = args.task_id.as_deref().expect("pre_done requires task_id");
        reify_audit::p5_phantom_done::check_pre_done(&ctx, task_id)
    } else {
        // Spot-check or window sweep: run selected detectors.
        let run_p1 = args.pattern.as_deref().is_none_or(|p| p == "P1");
        let run_p2 = args.pattern.as_deref().is_none_or(|p| p == "P2");
        let run_p5 = args.pattern.as_deref().is_none_or(|p| p == "P5");

        let mut all = Vec::new();
        if run_p1 {
            all.extend(reify_audit::p1_producer_orphan::check(&ctx));
        }
        if run_p2 {
            all.extend(reify_audit::p2_consumer_stub::check(&ctx));
        }
        if run_p5 {
            all.extend(reify_audit::p5_phantom_done::check(&ctx));
        }
        all
    };

    // Emit JSON findings on stderr.
    {
        let stderr = std::io::stderr();
        let mut lock = stderr.lock();
        if let Err(e) = serde_json::to_writer_pretty(&mut lock, &findings) {
            eprintln!("\nreify-audit: error serializing findings: {}", e);
        }
        // Ensure a trailing newline after the JSON block.
        let _ = writeln!(lock);
    }

    // Emit human-readable summary on stdout.
    print_summary(&findings);

    // Exit code = high-severity count, capped at 255.
    let code = high_severity_exit_code(&findings);
    ExitCode::from(code)
}

// -----------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use reify_audit::{DoneProvenance, EvidenceRef, Pattern, Severity};

    fn make_high() -> Finding {
        Finding {
            pattern: Pattern::P5PhantomDone,
            severity: Severity::High,
            task_id: "t".to_string(),
            summary: "s".to_string(),
            evidence: vec![],
        }
    }

    fn make_medium() -> Finding {
        Finding {
            pattern: Pattern::P2ConsumerStub,
            severity: Severity::Medium,
            task_id: "t".to_string(),
            summary: "s".to_string(),
            evidence: vec![],
        }
    }

    fn make_low() -> Finding {
        Finding {
            pattern: Pattern::P1ProducerOrphan,
            severity: Severity::Low,
            task_id: "t".to_string(),
            summary: "s".to_string(),
            evidence: vec![],
        }
    }

    #[test]
    fn exit_code_caps_high_severity_at_255() {
        // (a) empty slice → 0
        assert_eq!(high_severity_exit_code(&[]), 0);

        // (b) one High + two Medium + one Low → 1
        let mixed = vec![make_high(), make_medium(), make_medium(), make_low()];
        assert_eq!(high_severity_exit_code(&mixed), 1);

        // (c) 300 High findings → 255 (the cap)
        let many_high: Vec<Finding> = (0..300).map(|_| make_high()).collect();
        assert_eq!(high_severity_exit_code(&many_high), 255);
    }
}

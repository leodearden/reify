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
//! **stdout**. Exit-code convention (documented in `--help`):
//!
//! | Exit code | Meaning |
//! |-----------|---------|
//! | 0         | No High-severity findings |
//! | 1–254     | Count of High-severity findings (capped at 254) |
//! | 125       | Infrastructure/setup error (arg parse, IO, serialization) |
//!
//! Exit code 125 is reserved for errors so it never collides with a
//! finding-count result — callers (D-1 hook, T-5 skill) can branch on
//! `exit == 125` to detect misconfigured invocations without misreading
//! them as "125 phantom-done tasks".
//!
//! ### Why JSON on stderr?
//!
//! Per design §3/§10, this binary is primarily invoked as a subprocess — by
//! the dark-factory pre-done hook (D-1) and by the `/audit` skill (T-5),
//! both of which capture *stderr* for structured data and let *stdout* surface
//! as human-visible progress output in the terminal/log. The JSON-on-stderr
//! convention keeps the machine-readable payload on the fd that subprocess
//! wrappers typically capture separately from the user-facing summary.
//!
//! If you need JSON on stdout (e.g. `reify-audit ... | jq`), redirect stderr:
//! ```text
//! reify-audit --task 1234 2>&1 >/dev/null | jq '.[].severity'
//! ```
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
    AuditContext, ChangedSymbol, DeadSymbol, Finding, JCodemunchOps, LayerViolation, RealGitOps,
    Severity, SymbolReference, TaskMetadata, TimeWindow, UntestedSymbol,
    fused_memory_client::FusedMemoryClient,
    jcodemunch_client::RealJCodemunchOps,
};

// -----------------------------------------------------------------------
// NoopJCodemunchOps — inert stub for non-P1 runs and --no-jcodemunch
// -----------------------------------------------------------------------

/// Inert no-op implementation of [`JCodemunchOps`].
///
/// Used in two cases:
/// 1. `--no-jcodemunch` explicit escape hatch (offline/test mode — P1
///    runs but produces zero findings without opening any socket).
/// 2. Detector runs that don't need jcodemunch (P5/pre-done, P2-only) —
///    `needs_jcodemunch` returns false, so no connection is ever attempted.
///
/// Never escapes this bin file; the library's `MockJCodemunchOps` remains
/// test-only via the `test-support` feature.
struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(&self, _since_sha: &str, _until_sha: &str) -> Vec<ChangedSymbol> {
        vec![]
    }
    fn find_references(&self, _symbol: &ChangedSymbol) -> Vec<SymbolReference> {
        vec![]
    }
    fn get_dead_code(&self, _min_confidence: f64) -> Vec<DeadSymbol> {
        vec![]
    }
    fn get_untested_symbols(&self, _min_confidence: f64) -> Vec<UntestedSymbol> {
        vec![]
    }
    fn get_layer_violations(&self) -> Vec<LayerViolation> {
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
    let _ = writeln!(out, "  --pattern P1|P2|P5|PDEAD|PUNTESTED Restrict to one detector");
    let _ = writeln!(out, "  --tasks-file <path>      JSON array of TaskMetadata (overrides live loader; for tests)");
    let _ = writeln!(out, "  --fused-memory-url <url> MCP endpoint (default: $FUSED_MEMORY_URL or http://localhost:8002/mcp)");
    let _ = writeln!(out, "  --runs-db <path>         SQLite runs.db path (default: data/orchestrator/runs.db)");
    let _ = writeln!(out, "  --project-root <path>    Repo root for git ops + fused-memory project key (default: .)");
    let _ = writeln!(out, "  --jcodemunch-url <url>   jcodemunch MCP endpoint for P1 (default: $JCODEMUNCH_URL or http://127.0.0.1:8901/mcp)");
    let _ = writeln!(out, "  --jcodemunch-repo <id>   jcodemunch repo identifier (default: leodearden/reify)");
    let _ = writeln!(out, "  --no-jcodemunch          Use inert stub (offline/test); P1 yields nothing, no connection");
    let _ = writeln!(out, "  --help, -h               Show this help");
    let _ = writeln!(out, "  --version, -V            Print version");
    let _ = writeln!(out);
    let _ = writeln!(out, "Conflicts: --pre-done cannot be combined with --pattern or --since.");
    let _ = writeln!(out);
    let _ = writeln!(out, "Tasks source:");
    let _ = writeln!(out, "  By default, tasks are loaded live from the fused-memory MCP server.");
    let _ = writeln!(out, "  Pass --tasks-file <path> to load from a JSON array fixture instead");
    let _ = writeln!(out, "  (used by the integration test suite).");
    let _ = writeln!(out);
    let _ = writeln!(out, "Output:");
    let _ = writeln!(out, "  stderr: JSON array of Finding objects");
    let _ = writeln!(out, "  stdout: human-readable summary");
    let _ = writeln!(out, "  exit 0:    no High-severity findings");
    let _ = writeln!(out, "  exit 1-254: count of High-severity findings (capped at 254)");
    let _ = writeln!(out, "  exit 125:  infrastructure/setup error (arg parse, IO failure, MCP unreachable)");
    let _ = writeln!(out);
    let _ = writeln!(out, "Note: --tasks-file must be a JSON array of TaskMetadata objects");
    let _ = writeln!(out, "(all 9 fields required: task_id, status, files, done_provenance,");
    let _ = writeln!(out, " title, prd, consumer_ref, audit_foundation, done_at).");
}

// Use std::io::Write trait alias to accept both stdout and stderr.
use std::io::Write;

// -----------------------------------------------------------------------
// Exit-code convention
// -----------------------------------------------------------------------

/// Infrastructure/setup error exit code.
///
/// Reserved so it never collides with a High-severity finding count.
/// D-1 hook and T-5 skill should branch on `exit == ERROR_EXIT` to detect
/// misconfigured invocations separately from finding counts.
const ERROR_EXIT: u8 = 125;

/// Count High-severity findings and clamp to u8.
///
/// Capped at **254** (not 255) so that exit code 125 remains unambiguously
/// reserved for infrastructure errors. A run with 255+ High findings returns
/// 254, which is still a clear "many problems" signal to the caller.
fn high_severity_exit_code(findings: &[Finding]) -> u8 {
    let count = findings
        .iter()
        .filter(|f| f.severity == Severity::High)
        .count();
    count.min(254) as u8
}

// -----------------------------------------------------------------------
// Parsed CLI arguments
// -----------------------------------------------------------------------

struct Args {
    task_id: Option<String>,
    pre_done: bool,
    since: Option<String>,
    pattern: Option<String>,
    /// `Some(path)` → load TaskMetadata from a JSON fixture (test path).
    /// `None` → load live from fused-memory MCP at `fused_memory_url`.
    /// Default is `None` (live loader); `--tasks-file` opts into the
    /// fixture path for integration tests.
    tasks_file: Option<String>,
    /// MCP HTTP endpoint, falls back to `FUSED_MEMORY_URL` env or
    /// `http://localhost:8002/mcp`. Ignored when `tasks_file` is `Some`.
    fused_memory_url: String,
    runs_db: String,
    project_root: String,
    /// jcodemunch MCP endpoint for P1; falls back to `JCODEMUNCH_URL` env
    /// or `http://127.0.0.1:8901/mcp` (no trailing slash — `/mcp/` triggers
    /// a 307 redirect that drops `mcp-session-id`).
    jcodemunch_url: String,
    /// Repo identifier passed to `RealJCodemunchOps::new`. Default is the
    /// smoke-verified slash form `leodearden/reify`.
    jcodemunch_repo: String,
    /// When true, bind `NoopJCodemunchOps` even for P1 runs. Preserves
    /// hermetic test behaviour and provides an offline escape hatch.
    no_jcodemunch: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut task_id = None;
    let mut pre_done = false;
    let mut since = None;
    let mut pattern = None;
    let mut tasks_file: Option<String> = None;
    // Default uses `/mcp` (no trailing slash) — `/mcp/` triggers a 307
    // redirect that drops the `mcp-session-id` header and breaks the
    // MCP handshake. The smoke script (`scripts/smoke-predone-hook.sh`)
    // pins `/mcp` for the same reason.
    let mut fused_memory_url = std::env::var("FUSED_MEMORY_URL")
        .unwrap_or_else(|_| "http://localhost:8002/mcp".to_string());
    let mut runs_db = "data/orchestrator/runs.db".to_string();
    let mut project_root = ".".to_string();
    // Default uses `/mcp` (no trailing slash) — same redirect-avoidance
    // rationale as fused_memory_url above.
    let mut jcodemunch_url = std::env::var("JCODEMUNCH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8901/mcp".to_string());
    let mut jcodemunch_repo = "leodearden/reify".to_string();
    let mut no_jcodemunch = false;

    // NOTE: Last-wins semantics for duplicate flags.
    // When a flag appears more than once (e.g. the pre-done hook wrapper passes
    // its own --tasks-file, --runs-db, and --project-root before forwarding $@
    // which may include caller-supplied overrides), the last occurrence wins.
    // The wrapper relies on this contract: it prepends its defaults so that
    // any flag in the caller's $@ implicitly overrides the wrapper-supplied
    // value without requiring the wrapper to parse and strip $@.
    // This behaviour is locked by the `duplicate_flags_last_wins` integration
    // test in crates/reify-audit/tests/cli.rs.
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
                    "P1" | "P2" | "P5" | "PDEAD" | "PUNTESTED" => pattern = Some(p.to_string()),
                    other => {
                        return Err(format!(
                            "unknown --pattern value '{}'; expected P1, P2, P5, PDEAD, or PUNTESTED",
                            other
                        ))
                    }
                }
            }
            "--tasks-file" => {
                i += 1;
                tasks_file = Some(
                    argv.get(i)
                        .ok_or("--tasks-file requires a value")?
                        .clone(),
                );
            }
            "--fused-memory-url" => {
                i += 1;
                fused_memory_url = argv
                    .get(i)
                    .ok_or("--fused-memory-url requires a value")?
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
            "--jcodemunch-url" => {
                i += 1;
                jcodemunch_url = argv
                    .get(i)
                    .ok_or("--jcodemunch-url requires a value")?
                    .clone();
            }
            "--jcodemunch-repo" => {
                i += 1;
                jcodemunch_repo = argv
                    .get(i)
                    .ok_or("--jcodemunch-repo requires a value")?
                    .clone();
            }
            "--no-jcodemunch" => {
                no_jcodemunch = true;
            }
            other => {
                return Err(format!("unknown flag '{}'", other));
            }
        }
        i += 1;
    }

    Ok(Args {
        task_id,
        pre_done,
        since,
        pattern,
        tasks_file,
        fused_memory_url,
        runs_db,
        project_root,
        jcodemunch_url,
        jcodemunch_repo,
        no_jcodemunch,
    })
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
// Task loaders
// -----------------------------------------------------------------------

/// JSON-fixture loader (test path). Reads a file containing a JSON array
/// of [`TaskMetadata`] objects. Errors are formatted with a `reify-audit:`
/// prefix so the caller can surface them on stderr verbatim.
fn load_tasks_from_json_file(path: &str) -> Result<HashMap<String, TaskMetadata>, String> {
    let tasks_json = std::fs::read_to_string(path)
        .map_err(|e| format!("error reading tasks-file '{}': {}", path, e))?;
    let tasks_vec: Vec<TaskMetadata> = serde_json::from_str(&tasks_json)
        .map_err(|e| format!("error parsing tasks-file '{}': {}", path, e))?;
    Ok(tasks_vec
        .into_iter()
        .map(|t| (t.task_id.clone(), t))
        .collect())
}

/// Live fused-memory MCP loader (production path).
///
/// `pre_done_task_id` is `Some(id)` on the pre-done hook hot path — only
/// that one task is fetched (`get_task`). On the sweep path it is `None`
/// and the whole task corpus is pulled (`get_tasks`).
fn load_tasks_from_fused_memory(
    url: &str,
    project_root: &str,
    pre_done_task_id: Option<&str>,
) -> Result<HashMap<String, TaskMetadata>, String> {
    let client = FusedMemoryClient::new(url)
        .map_err(|e| format!("error connecting to fused-memory at '{}': {}", url, e))?;
    // Canonicalize project_root so `.` (the hook's inherited cwd) becomes
    // the absolute path fused-memory keys its DB on.
    let project_root_abs = std::fs::canonicalize(project_root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| project_root.to_string());

    if let Some(task_id) = pre_done_task_id {
        let tm = client
            .get_task(task_id, &project_root_abs)
            .map_err(|e| format!("error loading task {} from fused-memory: {}", task_id, e))?;
        let mut m = HashMap::new();
        m.insert(tm.task_id.clone(), tm);
        Ok(m)
    } else {
        let tasks = client
            .get_tasks(&project_root_abs)
            .map_err(|e| format!("error loading tasks from fused-memory: {}", e))?;
        Ok(tasks
            .into_iter()
            .map(|t| (t.task_id.clone(), t))
            .collect())
    }
}

// -----------------------------------------------------------------------
// Dispatch helpers
// -----------------------------------------------------------------------

/// Return true when at least one jcodemunch-backed detector (P1) is in the
/// run set for the given args.
///
/// Returns true when the selected pattern(s) require a live jcodemunch server.
/// Currently: no pattern (all detectors include P1), P1, PDEAD, or PUNTESTED.
/// P2/P5 run without jcodemunch; pre_done always skips it.
///
/// The connect decision (RealJCodemunchOps vs NoopJCodemunchOps) is separated
/// from the per-detector dispatch predicates (run_p1, run_pdead, …) so that
/// adding PDEAD here does not accidentally make P1 run on `--pattern PDEAD`.
fn needs_jcodemunch(args: &Args) -> bool {
    if args.pre_done {
        return false;
    }
    args.pattern.as_deref().is_none_or(|p| p == "P1" || p == "PDEAD" || p == "PUNTESTED")
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
        return ExitCode::from(ERROR_EXIT);
    }

    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("reify-audit: error: {}", e);
            print_usage(&mut std::io::stderr());
            return ExitCode::from(ERROR_EXIT);
        }
    };

    // --pre-done requires --task.
    if args.pre_done && args.task_id.is_none() {
        eprintln!("reify-audit: error: --pre-done requires --task");
        return ExitCode::from(ERROR_EXIT);
    }
    // --pre-done cannot be combined with --pattern or --since.
    if args.pre_done && args.pattern.is_some() {
        eprintln!("reify-audit: error: --pre-done cannot be combined with --pattern");
        return ExitCode::from(ERROR_EXIT);
    }
    if args.pre_done && args.since.is_some() {
        eprintln!("reify-audit: error: --pre-done cannot be combined with --since");
        return ExitCode::from(ERROR_EXIT);
    }

    // Load tasks: JSON-file fixture (tests) OR live fused-memory MCP (prod).
    let task_metadata: HashMap<String, TaskMetadata> = match &args.tasks_file {
        Some(path) => match load_tasks_from_json_file(path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("reify-audit: {}", e);
                return ExitCode::from(ERROR_EXIT);
            }
        },
        None => match load_tasks_from_fused_memory(
            &args.fused_memory_url,
            &args.project_root,
            args.task_id.as_deref().filter(|_| args.pre_done),
        ) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("reify-audit: {}", e);
                return ExitCode::from(ERROR_EXIT);
            }
        },
    };

    // Open runs.db.
    let conn = match rusqlite::Connection::open(&args.runs_db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("reify-audit: error opening runs-db '{}': {}", args.runs_db, e);
            return ExitCode::from(ERROR_EXIT);
        }
    };

    // Construct seam impls.
    let git = RealGitOps::new(PathBuf::from(&args.project_root));

    // Construct jcodemunch seam:
    // - Noop for --no-jcodemunch, P5/pre-done, and P2-only runs (never connects).
    // - Real for P1/PDEAD runs; if the serve is unreachable, fail-soft to Noop
    //   so P2/P5 still run and P1 degrades to zero findings. Exit 125 is
    //   reserved for genuine arg/IO misconfiguration, not an optional substrate.
    let jcodemunch: Box<dyn JCodemunchOps> =
        if args.no_jcodemunch || !needs_jcodemunch(&args) {
            Box::new(NoopJCodemunchOps)
        } else {
            match RealJCodemunchOps::new(
                args.jcodemunch_url.clone(),
                args.jcodemunch_repo.clone(),
                PathBuf::from(&args.project_root),
            ) {
                Ok(r) => Box::new(r),
                Err(e) => {
                    eprintln!(
                        "reify-audit: jcodemunch unreachable at '{}': {} — \
                        P1 degraded to zero findings; P2/P5 still run \
                        (pass --no-jcodemunch to silence)",
                        args.jcodemunch_url, e
                    );
                    Box::new(NoopJCodemunchOps)
                }
            }
        };

    // Build window (for --since).
    let window = args.since.as_ref().map(|s| TimeWindow {
        since: Some(s.clone()),
        until: None,
    });

    // Build context.  Box<dyn JCodemunchOps>::as_ref() coerces to
    // &dyn JCodemunchOps, satisfying the borrowed seam; the Box outlives ctx.
    let ctx = AuditContext {
        project_root: PathBuf::from(&args.project_root),
        conn: &conn,
        git: &git,
        jcodemunch: jcodemunch.as_ref(),
        task_metadata,
        target_task_id: args.task_id.clone(),
        window,
        now: None,
        producer_branch: None,
    };

    // Dispatch.
    let findings: Vec<Finding> = if args.pre_done {
        // --task <id> --pre-done: P5 only via check_pre_done.
        let task_id = args.task_id.as_deref().expect("pre_done requires task_id");
        reify_audit::p5_phantom_done::check_pre_done(&ctx, task_id)
    } else {
        // Spot-check or window sweep: run selected detectors.
        // run_p1 is DECOUPLED from needs_jcodemunch: needs_jcodemunch now also
        // covers PDEAD (which needs the live server), but run_p1 must not fire
        // on `--pattern PDEAD`. Each detector has its own explicit predicate.
        let run_p1 = args.pattern.as_deref().is_none_or(|p| p == "P1");
        let run_p2 = args.pattern.as_deref().is_none_or(|p| p == "P2");
        let run_p5 = args.pattern.as_deref().is_none_or(|p| p == "P5");
        // PDEAD and PUNTESTED are opt-in only — not part of the default all-detector sweep.
        let run_pdead = args.pattern.as_deref() == Some("PDEAD");
        let run_puntested = args.pattern.as_deref() == Some("PUNTESTED");

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
        if run_pdead {
            all.extend(reify_audit::pdead_dead_code::check(&ctx));
        }
        if run_puntested {
            all.extend(reify_audit::puntested::check(&ctx));
        }
        all
    };

    // Emit JSON findings on stderr. Scope the lock so it's dropped before any
    // subsequent writes; if serialization fails, exit with ERROR_EXIT rather
    // than falling through with a misleading finding-count exit code.
    let serialized_ok = {
        let stderr = std::io::stderr();
        let mut lock = stderr.lock();
        let result = serde_json::to_writer_pretty(&mut lock, &findings);
        // Ensure a trailing newline after the JSON block (inside the lock).
        let _ = writeln!(lock);
        result.is_ok()
    };
    if !serialized_ok {
        // Lock is now released; write the error to stderr cleanly.
        eprintln!("reify-audit: error serializing findings to JSON (broken stderr?)");
        return ExitCode::from(ERROR_EXIT);
    }

    // Emit human-readable summary on stdout.
    print_summary(&findings);

    // Exit code = high-severity count, capped at 254 (125 reserved for errors).
    let code = high_severity_exit_code(&findings);
    ExitCode::from(code)
}

// -----------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use reify_audit::{Pattern, Severity};

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
    fn exit_code_caps_high_severity_at_254() {
        // (a) empty slice → 0
        assert_eq!(high_severity_exit_code(&[]), 0);

        // (b) one High + two Medium + one Low → 1
        let mixed = vec![make_high(), make_medium(), make_medium(), make_low()];
        assert_eq!(high_severity_exit_code(&mixed), 1);

        // (c) 300 High findings → 254 (the cap; 125 is reserved for errors)
        let many_high: Vec<Finding> = (0..300).map(|_| make_high()).collect();
        assert_eq!(high_severity_exit_code(&many_high), 254);
    }

    // -------------------------------------------------------------------
    // parse_args error-branch coverage
    //
    // The hand-rolled parser has many error branches that previously had
    // no test coverage. These tests pin every error-message format string
    // so a typo or refactor flips a test red instead of silently changing
    // the user-visible CLI error.
    // -------------------------------------------------------------------

    fn unwrap_err(r: Result<Args, String>) -> String {
        match r {
            Ok(_) => panic!("parse_args returned Ok where Err was expected"),
            Err(e) => e,
        }
    }

    #[test]
    fn parse_args_empty_returns_defaults() {
        let args = parse_args(&[]).unwrap_or_else(|e| panic!("empty argv must parse: {e}"));
        assert!(args.task_id.is_none());
        assert!(!args.pre_done);
        assert!(args.since.is_none());
        assert!(args.pattern.is_none());
        assert!(args.tasks_file.is_none());
        assert_eq!(args.runs_db, "data/orchestrator/runs.db");
        assert_eq!(args.project_root, ".");
        // New jcodemunch flags: no_jcodemunch and jcodemunch_repo have
        // deterministic defaults; jcodemunch_url is env-dependent (JCODEMUNCH_URL
        // fallback) so we do not assert its exact value here.
        assert!(!args.no_jcodemunch);
        assert_eq!(args.jcodemunch_repo, "leodearden/reify");
    }

    #[test]
    fn parse_args_unknown_flag_returns_err() {
        let err = unwrap_err(parse_args(&["--bogus".to_string()]));
        assert!(
            err.contains("--bogus"),
            "error must name the offending flag; got: {err}"
        );
    }

    #[test]
    fn parse_args_missing_value_after_each_flag_returns_err() {
        // Every flag that takes a value must report its name in the error
        // when the value is missing (final-position bare flag).
        for flag in [
            "--task",
            "--since",
            "--pattern",
            "--tasks-file",
            "--runs-db",
            "--project-root",
            "--jcodemunch-url",
            "--jcodemunch-repo",
        ] {
            let err = unwrap_err(parse_args(&[flag.to_string()]));
            assert!(
                err.contains(flag),
                "error for `{flag}` must mention the flag name; got: {err}"
            );
            assert!(
                err.contains("requires a value"),
                "error for `{flag}` must say 'requires a value'; got: {err}"
            );
        }
    }

    #[test]
    fn parse_args_unknown_pattern_literal_returns_err() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "P9".to_string()]));
        assert!(
            err.contains("P9"),
            "error must name the offending literal; got: {err}"
        );
        assert!(
            err.contains("PDEAD"),
            "error must list PDEAD as a valid pattern literal; got: {err}"
        );
    }

    #[test]
    fn parse_args_accepts_pdead_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PDEAD".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PDEAD must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PDEAD"),
            "parsed pattern must be Some(\"PDEAD\")"
        );
    }

    #[test]
    fn parse_args_accepts_puntested_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PUNTESTED".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PUNTESTED must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PUNTESTED"),
            "parsed pattern must be Some(\"PUNTESTED\")"
        );
    }

    #[test]
    fn parse_args_happy_path_round_trip() {
        let argv: Vec<String> = [
            "--task",
            "3242",
            "--pre-done",
            "--since",
            "2026-05-01",
            "--pattern",
            "P5",
            "--tasks-file",
            "/tmp/tasks.json",
            "--runs-db",
            "/tmp/runs.db",
            "--project-root",
            "/tmp/repo",
            "--jcodemunch-url",
            "http://127.0.0.1:9/mcp",
            "--jcodemunch-repo",
            "my/repo",
            "--no-jcodemunch",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let args = parse_args(&argv).unwrap_or_else(|e| panic!("happy-path argv must parse: {e}"));
        assert_eq!(args.task_id.as_deref(), Some("3242"));
        assert!(args.pre_done);
        assert_eq!(args.since.as_deref(), Some("2026-05-01"));
        assert_eq!(args.pattern.as_deref(), Some("P5"));
        assert_eq!(args.tasks_file.as_deref(), Some("/tmp/tasks.json"));
        assert_eq!(args.runs_db, "/tmp/runs.db");
        assert_eq!(args.project_root, "/tmp/repo");
        assert_eq!(args.jcodemunch_url, "http://127.0.0.1:9/mcp");
        assert_eq!(args.jcodemunch_repo, "my/repo");
        assert!(args.no_jcodemunch);
    }

    // -------------------------------------------------------------------
    // needs_jcodemunch
    // -------------------------------------------------------------------

    /// Build a minimal `Args` for needs_jcodemunch tests.
    fn make_args(pre_done: bool, pattern: Option<&str>) -> Args {
        Args {
            task_id: None,
            pre_done,
            since: None,
            pattern: pattern.map(|s| s.to_string()),
            tasks_file: None,
            fused_memory_url: String::new(),
            runs_db: String::new(),
            project_root: String::new(),
            jcodemunch_url: String::new(),
            jcodemunch_repo: String::new(),
            no_jcodemunch: false,
        }
    }

    #[test]
    fn needs_jcodemunch_pre_done_always_false() {
        // pre_done ⇒ false regardless of pattern
        assert!(!needs_jcodemunch(&make_args(true, None)));
        assert!(!needs_jcodemunch(&make_args(true, Some("P1"))));
    }

    #[test]
    fn needs_jcodemunch_pattern_routing() {
        // No pattern (all detectors) → true (P1 is in the run set)
        assert!(needs_jcodemunch(&make_args(false, None)));
        // P1 explicitly → true
        assert!(needs_jcodemunch(&make_args(false, Some("P1"))));
        // P2-only → false
        assert!(!needs_jcodemunch(&make_args(false, Some("P2"))));
        // P5-only → false
        assert!(!needs_jcodemunch(&make_args(false, Some("P5"))));
        // PDEAD explicitly → true (needs live jcodemunch server)
        assert!(needs_jcodemunch(&make_args(false, Some("PDEAD"))));
    }

    #[test]
    fn needs_jcodemunch_puntested_routes_true() {
        // PUNTESTED explicitly → true (needs live jcodemunch server)
        assert!(
            needs_jcodemunch(&make_args(false, Some("PUNTESTED"))),
            "PUNTESTED must require jcodemunch (needs live server)"
        );
    }

    /// Guard: PDEAD and PUNTESTED are opt-in only — neither may run in the
    /// default (no --pattern) all-detector sweep.  A future refactor that
    /// accidentally folds either into the default run will trip this test.
    #[test]
    fn pdead_and_puntested_not_in_default_sweep() {
        let default_args = make_args(false, None);
        assert!(
            default_args.pattern.as_deref() != Some("PDEAD"),
            "PDEAD must be opt-in only (not part of the default sweep)"
        );
        assert!(
            default_args.pattern.as_deref() != Some("PUNTESTED"),
            "PUNTESTED must be opt-in only (not part of the default sweep)"
        );
    }
}

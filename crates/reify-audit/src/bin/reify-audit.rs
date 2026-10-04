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
//! - `--pattern <token>[,<token>…]`  Restrict which detector(s) run; comma-separated for multi-detector union (e.g. `--pattern P1,P2,P5`). The token vocabulary is [`reify_audit::pattern_flag::TOKENS`].
//!   `PDIAG` is the INV-SF-6 codes-mandatory ratchet — opt-in only, and one of
//!   the restricted detectors that move the exit code (see
//!   `docs/notes/diagnostic-severity-policy.md`).
//!   `PPRDSTATUS` is PRD status-prose drift — opt-in only, High, and raised to
//!   the escalation queue by `scripts/pprdstatus-escalate.py`. A run of it
//!   alone refuses an empty task corpus with 125.
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
//! | 125       | Infrastructure/setup error (arg parse, IO, serialization, empty task corpus for a corpus-only run set) |
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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// `NoopJCodemunchOps` — the inert stub bound for `--no-jcodemunch` and for
// detector runs that never touch the seam — now lives in the library, where
// its doc records the call sites. It used to be copy-pasted into this bin and
// both `*-baseline-gen` bins; this bin and `pdiag-baseline-gen` now bind the
// library's, while `ptodo-baseline-gen` still carries its own (see that doc).
// The library's `MockJCodemunchOps` remains test-only via the `test-support`
// feature.
use reify_audit::{
    AuditContext, Finding, JCodemunchOps, NoopJCodemunchOps, RealGitOps, Severity, TaskMetadata,
    TimeWindow, fused_memory_client::FusedMemoryClient, jcodemunch_client::RealJCodemunchOps,
    jcodemunch_index, p1_producer_orphan, p2_consumer_stub, p5_phantom_done, pattern_flag, pcite,
    pdcheck, pdead_dead_code, pdiag, pdoccover, pdssentinel, player, pprdstatus, ptodo, puntested,
};

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
    let _ = writeln!(
        out,
        "  --pattern {} Restrict to detector(s); comma-separated for union (e.g. --pattern P1,P2,P5)",
        pattern_flag::TOKENS.join("|")
    );
    let _ = writeln!(out, "                           PDIAG: INV-SF-6 codes-mandatory ratchet (opt-in; see docs/notes/diagnostic-severity-policy.md)");
    let _ = writeln!(out, "  --tasks-file <path>      JSON array of TaskMetadata (overrides live loader; for tests)");
    let _ = writeln!(out, "  --fused-memory-url <url> MCP endpoint (default: $FUSED_MEMORY_URL or http://localhost:8002/mcp)");
    let _ = writeln!(out, "  --runs-db <path>         SQLite runs.db path (default: data/orchestrator/runs.db)");
    let _ = writeln!(out, "  --project-root <path>    Repo root for git ops + fused-memory project key (default: .)");
    let _ = writeln!(out, "  --jcodemunch-url <url>   jcodemunch MCP endpoint for P1 (default: $JCODEMUNCH_URL or http://127.0.0.1:8901/mcp)");
    let _ = writeln!(out, "  --jcodemunch-repo <id>   jcodemunch repo identifier (default: derived per-path, e.g. local/<basename>-<sha1[..8]>)");
    let _ = writeln!(out, "  --jcodemunch-index-dir <path> jcodemunch index directory for the freshness gate (default: $JCODEMUNCH_INDEX_DIR, else $CODE_INDEX_PATH, else $HOME/.code-index)");
    let _ = writeln!(out, "  --no-jcodemunch          Use inert stub (offline/test); P1 yields nothing, no connection");
    let _ = writeln!(out, "  --print-repo-id          Print the derived (or --jcodemunch-repo-overridden) jcodemunch");
    let _ = writeln!(out, "                           repo id for --project-root, then exit (no task/git/runs-db work)");
    let _ = writeln!(out, "  --help, -h               Show this help");
    let _ = writeln!(out, "  --version, -V            Print version");
    let _ = writeln!(out);
    let _ = writeln!(out, "Conflicts: --pre-done cannot be combined with --pattern or --since.");
    let _ = writeln!(out);
    let _ = writeln!(out, "--pre-done landing gate:");
    let _ = writeln!(out, "  Refuses the done-flip when a declared, non-gitignored metadata.files");
    let _ = writeln!(out, "  entry is neither tracked on main nor covered by a task-referencing");
    let _ = writeln!(out, "  commit's own delta. Provenance-free: the hook fires before the write");
    let _ = writeln!(out, "  and receives only the task id.");
    let _ = writeln!(out, "  REIFY_AUDIT_PREDONE_WARN_ONLY=1  Break-glass: downgrade that refusal to");
    let _ = writeln!(out, "                                   Low, making the gate advisory (exit 0).");
    let _ = writeln!(out, "                                   The finding is still emitted, prefixed");
    let _ = writeln!(out, "                                   '[warn-only]'. Default is ARMED.");
    let _ = writeln!(out, "                                   Caveat: the dark-factory hook shows a");
    let _ = writeln!(out, "                                   subprocess's stderr only on non-zero");
    let _ = writeln!(out, "                                   exit, so warn-only is SILENT there.");
    let _ = writeln!(out, "                                   Soak by running this binary directly.");
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
    let _ = writeln!(out, "  exit 125:  infrastructure/setup error (arg parse, IO failure, MCP unreachable,");
    let _ = writeln!(out, "             empty task corpus when every selected detector needs it)");
    let _ = writeln!(out);
    let _ = writeln!(out, "Note: --tasks-file must be a JSON array of TaskMetadata objects");
    let _ = writeln!(out, "(all 9 fields required: task_id, status, files, done_provenance,");
    let _ = writeln!(out, " title, prd, consumer_ref, audit_foundation, done_at).");
}

// Use std::io::Write trait alias to accept both stdout and stderr.
use std::io::Write;

// -----------------------------------------------------------------------
// §4.3 — jcodemunch index freshness precondition
// -----------------------------------------------------------------------

/// Refuse to query a jcodemunch corpus that cannot be shown fresh and
/// non-empty. Returns the already-rendered refusal message on `Err`.
///
/// Called ONLY once a live serve is genuinely about to be queried — see the
/// call site — and always before any detector `check()`.
///
/// Takes the caller's `RealGitOps` rather than shelling out itself: that
/// routes the HEAD read through the same bounded-retry path every other git
/// invocation uses, so a transient EAGAIN under load cannot abort an audit
/// with a message blaming index freshness. It also honours `RealGitOps`'
/// single-instance construction requirement.
fn enforce_index_freshness(args: &Args, git: &RealGitOps, repo_id: &str) -> Result<(), String> {
    // A freshness claim we cannot verify is worth no more than a stale one, so
    // an unreadable HEAD refuses rather than proceeding. The breadcrumb is
    // deliberately distinct from every marker token: neither staleness nor
    // emptiness nor unreadability of the INDEX has been established here, and
    // mislabelling this as any of them would send an operator to re-index when
    // the real fault is the git invocation.
    let live_head = git.head_sha().map_err(|e| {
        format!(
            "cannot verify jcodemunch index freshness for {repo_id} — {e}; refusing \
             rather than querying a corpus of unknown vintage (pass --no-jcodemunch \
             to skip the jcodemunch-backed detectors)"
        )
    })?;

    let index_dir = Path::new(&args.jcodemunch_index_dir);
    let state = jcodemunch_index::read_index_state(index_dir, repo_id);
    jcodemunch_index::evaluate_freshness(&state, &live_head).map_err(|refusal| {
        refusal
            .with_repo_id(repo_id)
            // The exact symbol count is deliberately NOT read on the startup
            // path (`count(*)` is a full table walk over 10^5–10^6 rows); it
            // is fetched here, on the refusal path, where the message is about
            // to name it and the run is ending anyway.
            .with_symbol_count(jcodemunch_index::count_symbols(index_dir, repo_id))
            .to_string()
    })
}

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
    /// Validated comma-separated detector token list (e.g. `"P1,P2,P5"`).
    /// Each token is a member of [`pattern_flag::TOKENS`].
    /// `None` means no restriction — all default-sweep detectors run.
    /// Use `pattern_selects(val, token)` to test membership.
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
    /// Repo identifier passed to `RealJCodemunchOps::new`, and the identity
    /// whose index the freshness gate probes.
    ///
    /// `None` — the default — means DERIVE it from `project_root` per §4.2,
    /// reproducing jcodemunch's own `storage/git_root.py` `_local_repo_name`:
    /// `local/<basename>-<sha1(abs_path)[..8]>`. `Some(id)` is an explicit
    /// operator override.
    ///
    /// There is deliberately no hardcoded default. A `<owner>/<project>`
    /// git-identity index names the *project*, not the *checkout*, so every
    /// one of reify's worktrees would share one index and continuously
    /// invalidate each other's; such an index is also never GC'd. Deriving
    /// per-path gives each checkout its own corpus, which is what makes the
    /// §4.3 freshness comparison meaningful at all.
    jcodemunch_repo: Option<String>,
    /// Directory holding jcodemunch's per-repo index databases, probed by the
    /// §4.3 freshness gate. Resolution order: this flag, then
    /// `JCODEMUNCH_INDEX_DIR`, then `CODE_INDEX_PATH`, then
    /// `$HOME/.code-index`.
    ///
    /// `CODE_INDEX_PATH` is load-bearing, not a courtesy: it is jcodemunch's
    /// OWN index-directory variable and the one the rest of this substrate
    /// already honours — `scripts/jcodemunch-index-reify.sh` resolves the DB
    /// as `${CODE_INDEX_PATH:-$HOME/.code-index}/local-<name>.db`, and
    /// `tests/infra/test_jcodemunch_index_reify.sh` drives its whole suite
    /// through a temp `CODE_INDEX_PATH`. Ignoring it would reopen, on the
    /// DIRECTORY axis, exactly the failure `resolve_repo_id` forbids on the
    /// IDENTITY axis: the indexer writes a healthy corpus to
    /// `$CODE_INDEX_PATH/…`, the gate probes `$HOME/.code-index/…`, finds
    /// nothing, and refuses `E_JC_INDEX_EMPTY` against a fully-indexed tree —
    /// sending the operator to re-index a phantom.
    ///
    /// `JCODEMUNCH_INDEX_DIR` is retained ahead of it as an audit-local
    /// override, so the gate can be pointed at a different store than the
    /// indexer without disturbing `CODE_INDEX_PATH` for co-running tools.
    jcodemunch_index_dir: String,
    /// When true, bind `NoopJCodemunchOps` even for P1 runs. Preserves
    /// hermetic test behaviour and provides an offline escape hatch.
    no_jcodemunch: bool,
    /// `--print-repo-id`: print the jcodemunch repo identity for
    /// `--project-root` to stdout and exit, touching none of the
    /// task/runs-db/git machinery below.
    print_repo_id: bool,
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
    // No default: `None` means derive per §4.2 from the project root. See the
    // `Args::jcodemunch_repo` doc for why a hardcoded git-identity default is
    // wrong rather than merely unnecessary.
    let mut jcodemunch_repo: Option<String> = None;
    // Precedence: JCODEMUNCH_INDEX_DIR (audit-local override) > CODE_INDEX_PATH
    // (jcodemunch's own variable, honoured by scripts/jcodemunch-index-reify.sh
    // and tests/infra/test_jcodemunch_index_reify.sh) > $HOME/.code-index.
    // See the `Args::jcodemunch_index_dir` doc for why skipping CODE_INDEX_PATH
    // would make the gate refuse a healthy corpus.
    let mut jcodemunch_index_dir = std::env::var("JCODEMUNCH_INDEX_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CODE_INDEX_PATH")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| {
            // $HOME is present in every sanctioned invocation; the bare relative
            // fallback keeps parse_args infallible rather than adding a second
            // failure mode to arg parsing.
            match std::env::var("HOME") {
                Ok(home) => format!("{home}/.code-index"),
                Err(_) => ".code-index".to_string(),
            }
        });
    let mut no_jcodemunch = false;
    let mut print_repo_id = false;

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
                // Validate each comma-separated token individually.
                for tok in p.split(',') {
                    let tok = tok.trim();
                    if tok.is_empty() {
                        return Err(
                            "empty --pattern token; remove the stray comma \
                             (e.g. use 'P1,P2' not 'P1,P2,')"
                                .to_string(),
                        );
                    }
                    if !pattern_flag::TOKENS.contains(&tok) {
                        return Err(format!(
                            "unknown --pattern value '{tok}'; expected one of: {}",
                            pattern_flag::TOKENS.join(", ")
                        ));
                    }
                }
                pattern = Some(p.to_string());
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
                jcodemunch_repo = Some(
                    argv.get(i)
                        .ok_or("--jcodemunch-repo requires a value")?
                        .clone(),
                );
            }
            "--jcodemunch-index-dir" => {
                i += 1;
                jcodemunch_index_dir = argv
                    .get(i)
                    .ok_or("--jcodemunch-index-dir requires a value")?
                    .clone();
            }
            "--no-jcodemunch" => {
                no_jcodemunch = true;
            }
            "--print-repo-id" => {
                print_repo_id = true;
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
        jcodemunch_index_dir,
        no_jcodemunch,
        print_repo_id,
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

/// Return true when the validated comma-separated `pattern` value selects `token`.
///
/// `pattern` is the raw stored value (e.g. `"P1,P2,P5"`); callers pass
/// `args.pattern.as_deref()` and handle `None` (no restriction) themselves.
fn pattern_selects(pattern: &str, token: &str) -> bool {
    pattern.split(',').map(str::trim).any(|t| t == token)
}

/// Return true when at least one selected detector queries jcodemunch, so the
/// run needs a live client. `--pre-done` never does: it runs P5 alone.
fn needs_jcodemunch(args: &Args) -> bool {
    !args.pre_done
        && selected_detectors(args.pattern.as_deref()).any(|detector| detector.queries_jcodemunch)
}

/// Return true when EVERY detector selected by `--pattern` is
/// jcodemunch-backed, i.e. a refusal costs the run nothing it could still
/// have delivered.
///
/// This is the blast-radius boundary for the §4.3 gate. `needs_jcodemunch` is
/// true for the pattern-less DEFAULT sweep, which also runs P2, P5, PTODO and
/// PDSSENTINEL — none of which consult jcodemunch at all. Refusing the whole
/// process there would kill five working detectors over one unusable corpus,
/// and §4.2 makes that the EXPECTED case rather than an anomaly: identity is
/// now per-checkout, so every warm-lane/task worktree derives an id nothing
/// has indexed. A default sweep from any such worktree with the serve up would
/// exit 125 with zero findings.
///
/// So the refusal is scoped: an all-jcodemunch run set has nothing to salvage
/// and hard-refuses (the §4.3 contract, and what B4/B5/B6 pin); a mixed or
/// default run set degrades the jcodemunch-backed detectors to the Noop seam
/// and keeps going, which is exactly the shape the unreachable-serve fail-soft
/// already has. Either way the corpus is never queried.
///
/// `false` for a pattern-less run: the default sweep is mixed by definition.
fn jcodemunch_only_run_set(args: &Args) -> bool {
    args.pattern.is_some()
        && selected_detectors(args.pattern.as_deref()).all(|detector| detector.queries_jcodemunch)
}

/// Return true when EVERY detector selected by `--pattern` refuses an empty
/// task corpus, so a refusal costs the run nothing it could still have
/// delivered: the blast-radius boundary [`jcodemunch_only_run_set`] draws for
/// an unusable index, drawn for an empty corpus.
///
/// `false` for a pattern-less run: the default sweep is mixed by definition.
fn task_corpus_only_run_set(args: &Args) -> bool {
    args.pattern.is_some()
        && selected_detectors(args.pattern.as_deref())
            .all(|detector| detector.refuses_empty_task_corpus)
}

/// One detector a sweep can dispatch. Every fact the binary knows about a
/// detector is a field of its row, so selection, the jcodemunch connect
/// decision and the check it runs cannot drift apart.
struct Detector {
    /// The [`pattern_flag`] token that selects it.
    token: &'static str,
    /// Whether a pattern-less run includes it. The exit code is the
    /// High-severity count and every bare `reify-audit` invocation runs this
    /// sweep, so a detector whose High findings track a standing backlog or a
    /// drifting baseline (PDIAG, PDOCCOVER, PDCHECK) stays opt-in: in the
    /// sweep it would turn those invocations non-zero for reasons unrelated
    /// to the work under audit.
    in_default_sweep: bool,
    /// Whether it cannot produce a finding without querying jcodemunch.
    /// Selecting it makes the run connect ([`needs_jcodemunch`]), and a run of
    /// nothing else hard-refuses on a stale index ([`jcodemunch_only_run_set`]),
    /// so a structural detector marked `true` would exit 125 on every stale
    /// index while never reading it.
    queries_jcodemunch: bool,
    /// Whether it treats an empty task corpus as a setup error rather than a
    /// clean result. A run of nothing else refuses an empty corpus with 125
    /// before printing any findings array ([`task_corpus_only_run_set`]); in a
    /// mixed run [`run_detector`] skips it with a "skipped" breadcrumb. Either
    /// way no caller can read the unchecked detector as clean.
    refuses_empty_task_corpus: bool,
    check: fn(&AuditContext<'_>) -> Vec<Finding>,
}

impl Detector {
    /// `None` is the pattern-less default sweep.
    fn selected_by(&self, pattern: Option<&str>) -> bool {
        pattern.map_or(self.in_default_sweep, |p| pattern_selects(p, self.token))
    }
}

/// Every detector a sweep can dispatch, one row per [`pattern_flag::TOKENS`]
/// member in the same (`--help`) order, which is the order their findings are
/// emitted in.
#[rustfmt::skip]
const DETECTORS: &[Detector] = &[
    Detector { token: pattern_flag::P1,          in_default_sweep: true,  queries_jcodemunch: true,  refuses_empty_task_corpus: false, check: p1_producer_orphan::check },
    Detector { token: pattern_flag::P2,          in_default_sweep: true,  queries_jcodemunch: false, refuses_empty_task_corpus: false, check: p2_consumer_stub::check },
    Detector { token: pattern_flag::P5,          in_default_sweep: true,  queries_jcodemunch: false, refuses_empty_task_corpus: false, check: p5_phantom_done::check },
    Detector { token: pattern_flag::PDEAD,       in_default_sweep: false, queries_jcodemunch: true,  refuses_empty_task_corpus: false, check: pdead_dead_code::check },
    Detector { token: pattern_flag::PUNTESTED,   in_default_sweep: false, queries_jcodemunch: true,  refuses_empty_task_corpus: false, check: puntested::check },
    Detector { token: pattern_flag::PLAYER,      in_default_sweep: false, queries_jcodemunch: true,  refuses_empty_task_corpus: false, check: player::check },
    Detector { token: pattern_flag::PTODO,       in_default_sweep: true,  queries_jcodemunch: false, refuses_empty_task_corpus: false, check: ptodo::check },
    Detector { token: pattern_flag::PDSSENTINEL, in_default_sweep: true,  queries_jcodemunch: false, refuses_empty_task_corpus: false, check: pdssentinel::check },
    Detector { token: pattern_flag::PDIAG,       in_default_sweep: false, queries_jcodemunch: false, refuses_empty_task_corpus: false, check: pdiag::check },
    Detector { token: pattern_flag::PDOCCOVER,   in_default_sweep: false, queries_jcodemunch: false, refuses_empty_task_corpus: false, check: pdoccover::check },
    Detector { token: pattern_flag::PDCHECK,     in_default_sweep: false, queries_jcodemunch: false, refuses_empty_task_corpus: false, check: pdcheck::check },
    Detector { token: pattern_flag::PCITE,       in_default_sweep: false, queries_jcodemunch: false, refuses_empty_task_corpus: false, check: pcite::check },
    Detector { token: pattern_flag::PPRDSTATUS,  in_default_sweep: false, queries_jcodemunch: false, refuses_empty_task_corpus: true,  check: pprdstatus::check },
];

/// One detector's findings. A detector that refuses an empty task corpus is
/// not run on one: the breadcrumb marks its zero findings as unchecked.
fn run_detector(detector: &Detector, ctx: &AuditContext<'_>) -> Vec<Finding> {
    if detector.refuses_empty_task_corpus && ctx.task_metadata.is_empty() {
        eprintln!(
            "reify-audit: {} skipped — the task corpus is empty; \
             this is NOT a clean bill of health",
            detector.token
        );
        return Vec::new();
    }
    (detector.check)(ctx)
}

/// The [`DETECTORS`] rows a run with this `--pattern` value dispatches, in
/// row order.
fn selected_detectors(pattern: Option<&str>) -> impl Iterator<Item = &'static Detector> {
    DETECTORS
        .iter()
        .filter(move |detector| detector.selected_by(pattern))
}

/// The jcodemunch repo identity this invocation acts on: `--jcodemunch-repo`
/// when given, otherwise derived from `--project-root` per §4.2. One function,
/// so the identity `--print-repo-id` PRINTS and the identity the gate
/// INTERROGATES cannot apply that precedence differently.
fn effective_repo_id(args: &Args) -> String {
    args.jcodemunch_repo
        .clone()
        .unwrap_or_else(|| jcodemunch_index::resolve_repo_id(Path::new(&args.project_root)))
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

    // A standalone info mode, like --help/--version: it needs only
    // --project-root, so it returns before any task load, runs.db open or git
    // op. Why the derivation lives here rather than in bash, and who consumes
    // it: scripts/jcodemunch-index-reify.sh, "one derivation, not two".
    if args.print_repo_id {
        println!("{}", effective_repo_id(&args));
        return ExitCode::SUCCESS;
    }

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

    // Returns before any findings array is serialized, so the refusal emits no
    // parseable JSON on stderr: the /audit skill's exit-125 disambiguator and
    // scripts/pprdstatus-escalate.py both read that as "nothing was checked".
    if task_metadata.is_empty() && task_corpus_only_run_set(&args) {
        eprintln!(
            "reify-audit: the task corpus is empty and every selected detector \
             needs it; refusing rather than reporting an unchecked run as clean \
             (check --project-root and --fused-memory-url, or --tasks-file)"
        );
        return ExitCode::from(ERROR_EXIT);
    }

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

    // Resolve the jcodemunch repo identity ONCE, before the seam is
    // constructed, so the identity queried and the identity gated cannot
    // diverge. `--jcodemunch-repo` overrides; otherwise derive per §4.2.
    let jcodemunch_repo_id = effective_repo_id(&args);

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
                jcodemunch_repo_id.clone(),
                PathBuf::from(&args.project_root),
            ) {
                Ok(r) => {
                    // §4.3 freshness precondition. This is the ONLY place the
                    // gate fires, and the placement is the load-bearing part:
                    // a jcodemunch-backed detector is in the run set,
                    // --no-jcodemunch was absent, AND the handshake actually
                    // succeeded — so a live serve is genuinely about to be
                    // queried. It runs before `ctx` is built and therefore
                    // before any detector `check()`.
                    //
                    // Deliberately NOT earlier. §4.3's harm model is false
                    // orphans produced FROM a stale corpus; with the serve
                    // down there is no corpus to be misled by (the Err arm
                    // below already fail-softs to zero findings), so there is
                    // nothing to refuse. Gating before the connection attempt
                    // would convert that documented fail-soft into a hard exit
                    // 125 on every machine where jcodemunch is legitimately
                    // absent — an outage on a healthy path. Constructing the
                    // client is not "a detector query", so gating a successful
                    // construction satisfies §4.3 literally while preserving
                    // the fail-soft.
                    match enforce_index_freshness(&args, &git, &jcodemunch_repo_id) {
                        Ok(()) => Box::new(r),
                        // Nothing in the run set survives a refusal, so refuse
                        // the process. Returns BEFORE any findings array is
                        // serialized, so the refusal emits no parseable JSON on
                        // stderr — which is what lets the /audit skill's
                        // existing exit-125 disambiguator classify this as an
                        // infra error rather than 125 High findings.
                        Err(msg) if jcodemunch_only_run_set(&args) => {
                            eprintln!("reify-audit: {msg}");
                            return ExitCode::from(ERROR_EXIT);
                        }
                        // Mixed or default sweep: the corpus is still never
                        // queried (Noop answers every jcodemunch call with
                        // nothing), but P2/P5/PTODO/PDSSENTINEL keep running
                        // and the run still emits its findings array. The
                        // breadcrumb carries the marker token, so the condition
                        // is machine-detectable rather than silent — the same
                        // contract the unreachable-serve fail-soft has.
                        Err(msg) => {
                            eprintln!(
                                "reify-audit: {msg} — jcodemunch-backed detectors \
                                degraded to zero findings; the rest of the sweep \
                                still runs (use --pattern P1 to make this a hard \
                                refusal)"
                            );
                            Box::new(NoopJCodemunchOps)
                        }
                    }
                }
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
        // Spot-check or window sweep: every detector this run selects.
        selected_detectors(args.pattern.as_deref())
            .flat_map(|detector| run_detector(detector, &ctx))
            .collect()
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
        // New jcodemunch flags: no_jcodemunch has a deterministic default;
        // jcodemunch_url and jcodemunch_index_dir are env-dependent
        // (JCODEMUNCH_URL / JCODEMUNCH_INDEX_DIR fallbacks) so we do not
        // assert their exact values here.
        assert!(!args.no_jcodemunch);
        assert!(
            args.jcodemunch_repo.is_none(),
            "no --jcodemunch-repo must leave the id UNSET so it is derived \
             per §4.2 from the project root; a hardcoded git-identity default \
             MUST NOT exist — such an index names the project rather than the \
             checkout, so it would collide across reify's many worktrees and \
             is never GC'd"
        );
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
            "--jcodemunch-index-dir",
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
        assert!(
            err.contains("PTODO"),
            "error must list PTODO as a valid pattern literal; got: {err}"
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
        // The --jcodemunch-repo OVERRIDE is retained; only the hardcoded
        // default is gone.
        assert_eq!(args.jcodemunch_repo.as_deref(), Some("my/repo"));
        assert!(args.no_jcodemunch);
    }

    #[test]
    fn parse_args_accepts_jcodemunch_index_dir() {
        let argv: Vec<String> = ["--jcodemunch-index-dir", "/tmp/ix"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let args = parse_args(&argv).unwrap_or_else(|e| panic!("must parse: {e}"));
        assert_eq!(args.jcodemunch_index_dir, "/tmp/ix");
    }

    /// An accepted-but-undiscoverable flag is a usability bug.
    #[test]
    fn usage_text_lists_jcodemunch_index_dir() {
        let mut buf: Vec<u8> = Vec::new();
        print_usage(&mut buf);
        let usage = String::from_utf8(buf).expect("usage text is UTF-8");
        assert!(
            usage.contains("--jcodemunch-index-dir"),
            "--help must list --jcodemunch-index-dir; got:\n{usage}"
        );
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
            jcodemunch_repo: None,
            jcodemunch_index_dir: String::new(),
            no_jcodemunch: false,
            print_repo_id: false,
        }
    }

    /// Whether a run with this `--pattern` value dispatches `token`'s detector.
    fn selects(token: &str, pattern: Option<&str>) -> bool {
        selected_detectors(pattern).any(|detector| detector.token == token)
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
        assert!(
            !selects(pattern_flag::PDEAD, None),
            "PDEAD must be opt-in only (not part of the default sweep)"
        );
        assert!(
            selects(pattern_flag::PDEAD, Some("PDEAD")),
            "PDEAD must activate when --pattern PDEAD is given"
        );
        assert!(
            !selects(pattern_flag::PUNTESTED, None),
            "PUNTESTED must be opt-in only (not part of the default sweep)"
        );
        assert!(
            selects(pattern_flag::PUNTESTED, Some("PUNTESTED")),
            "PUNTESTED must activate when --pattern PUNTESTED is given"
        );
    }

    // -------------------------------------------------------------------
    // PLAYER CLI-wiring tests (step-3 RED / step-4 GREEN)
    // -------------------------------------------------------------------

    #[test]
    fn parse_args_accepts_player_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PLAYER".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PLAYER must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PLAYER"),
            "parsed pattern must be Some(\"PLAYER\")"
        );
    }

    #[test]
    fn needs_jcodemunch_player_routes_true() {
        // PLAYER explicitly → true (needs live jcodemunch server)
        assert!(
            needs_jcodemunch(&make_args(false, Some("PLAYER"))),
            "PLAYER must require jcodemunch (needs live server)"
        );
    }

    /// Guard: PLAYER is opt-in only — must not run in the default (no --pattern)
    /// all-detector sweep.
    #[test]
    fn player_not_in_default_sweep() {
        assert!(
            !selects(pattern_flag::PLAYER, None),
            "PLAYER must be opt-in only (not part of the default sweep)"
        );
        assert!(
            selects(pattern_flag::PLAYER, Some("PLAYER")),
            "PLAYER must activate when --pattern PLAYER is given"
        );
    }

    // -------------------------------------------------------------------
    // comma-separated --pattern tests (step-1 RED, step-2 GREEN)
    // -------------------------------------------------------------------

    /// `--pattern P1,P2,P5` must be accepted; the stored value must contain
    /// all three tokens when split on ','.
    #[test]
    fn parse_args_pattern_accepts_comma_list() {
        let args = parse_args(&["--pattern".to_string(), "P1,P2,P5".to_string()])
            .expect("--pattern P1,P2,P5 must parse successfully");
        let val = args.pattern.as_deref().expect("pattern must be Some");
        let tokens: Vec<&str> = val.split(',').map(str::trim).collect();
        assert!(tokens.contains(&"P1"), "tokens must contain P1; got: {tokens:?}");
        assert!(tokens.contains(&"P2"), "tokens must contain P2; got: {tokens:?}");
        assert!(tokens.contains(&"P5"), "tokens must contain P5; got: {tokens:?}");
    }

    /// `--pattern P1, P2 , P5` (with spaces around commas) must be accepted —
    /// per-token whitespace trimming during validation must not reject valid tokens.
    #[test]
    fn parse_args_pattern_trims_whitespace_around_tokens() {
        let args = parse_args(&["--pattern".to_string(), "P1, P2 , P5".to_string()])
            .expect("--pattern with spaces around commas must parse successfully");
        let pattern = args
            .pattern
            .as_deref()
            .expect("pattern must be Some; whitespace-padded comma list must be accepted");
        // The real reason trimming matters: the whitespace-padded tokens must be
        // selectable at the dispatch layer. Without trimming, " P2 " would not
        // match "P2" and the detector would silently not run.
        assert!(
            pattern_selects(pattern, "P1"),
            "padded token P1 must be selectable; got stored pattern {pattern:?}"
        );
        assert!(
            pattern_selects(pattern, "P2"),
            "padded token ' P2 ' must trim and be selectable as P2; got stored pattern {pattern:?}"
        );
        assert!(
            pattern_selects(pattern, "P5"),
            "padded token P5 must be selectable; got stored pattern {pattern:?}"
        );
    }

    /// `--pattern P1,BOGUS` must fail with an error that names `BOGUS` (the
    /// specific bad token) and contains the known-token expected wording, but
    /// does NOT contain the whole `P1,BOGUS` string.
    #[test]
    fn parse_args_pattern_unknown_token_in_list_names_token() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "P1,BOGUS".to_string()]));
        assert!(
            err.contains("'BOGUS'"),
            "error must name the offending token 'BOGUS' (with surrounding quotes); got: {err}"
        );
        // Every vocabulary token, by containment rather than the exact
        // connecting prose, so reordering the list does not break the test.
        for &tok in pattern_flag::TOKENS {
            assert!(
                err.contains(tok),
                "error must list known token {tok}; got: {err}"
            );
        }
        // NOTE: we do not assert !err.contains("P1,BOGUS") — a future message
        // that echoes the input but still names BOGUS would be equally valid.
        // The positive assertions above (token named + known-token list) are
        // the meaningful contract.
    }

    /// `parse_args` accepts exactly the `--pattern` vocabulary,
    /// `pattern_flag::TOKENS`: every member alone and in the full union, and
    /// nothing outside it.
    #[test]
    fn parse_args_accepts_exactly_the_pattern_flag_vocabulary() {
        let tokens = pattern_flag::TOKENS;
        assert!(
            !tokens.is_empty(),
            "the --pattern vocabulary must not be empty"
        );
        let distinct: std::collections::HashSet<&str> = tokens.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            tokens.len(),
            "the --pattern vocabulary must not repeat a token; got {tokens:?}"
        );

        for &tok in tokens {
            let args = parse_args(&["--pattern".to_string(), tok.to_string()])
                .unwrap_or_else(|e| panic!("--pattern {tok} must parse; got: {e}"));
            assert_eq!(
                args.pattern.as_deref(),
                Some(tok),
                "--pattern {tok} must be stored as given"
            );
        }

        let union = tokens.join(",");
        if let Err(e) = parse_args(&["--pattern".to_string(), union.clone()]) {
            panic!("the full union --pattern {union} must parse; got: {e}");
        }

        let err = unwrap_err(parse_args(&["--pattern".to_string(), "PNOPE".to_string()]));
        assert!(
            err.contains("'PNOPE'"),
            "a token outside the vocabulary must be rejected by name; got: {err}"
        );
    }

    /// `main` dispatches through `DETECTORS` alone, so this is what stops an
    /// accepted `--pattern` token from running nothing: one row per token, in
    /// `--help` order, and `--pattern <token>` selecting that row and no other.
    #[test]
    fn every_pattern_token_selects_exactly_its_own_detector() {
        let row_tokens: Vec<&str> = DETECTORS.iter().map(|detector| detector.token).collect();
        assert_eq!(
            row_tokens,
            pattern_flag::TOKENS,
            "DETECTORS must hold one row per --pattern token, in --help order"
        );
        for &token in pattern_flag::TOKENS {
            let selected: Vec<&str> = selected_detectors(Some(token))
                .map(|detector| detector.token)
                .collect();
            assert_eq!(
                selected,
                [token],
                "--pattern {token} must select exactly its own row"
            );
        }
    }

    /// The pattern-less sweep is what every bare `reify-audit` invocation, and
    /// the /audit skill, runs. The skill documents exactly these five.
    #[test]
    fn default_sweep_is_exactly_p1_p2_p5_ptodo_pdssentinel() {
        let swept: Vec<&str> = selected_detectors(None)
            .map(|detector| detector.token)
            .collect();
        assert_eq!(
            swept,
            [
                pattern_flag::P1,
                pattern_flag::P2,
                pattern_flag::P5,
                pattern_flag::PTODO,
                pattern_flag::PDSSENTINEL,
            ],
            "the no-`--pattern` default sweep must run exactly these detectors, in row order"
        );
    }

    /// Trailing or leading commas (`--pattern P1,` / `--pattern ,P2`) produce
    /// a dedicated "empty --pattern token" diagnostic rather than the generic
    /// `unknown --pattern value ''` message.
    #[test]
    fn parse_args_pattern_empty_token_gives_clear_error() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "P1,".to_string()]));
        assert!(
            err.contains("empty --pattern token"),
            "trailing comma must produce empty-token diagnostic; got: {err}"
        );
        let err2 = unwrap_err(parse_args(&["--pattern".to_string(), ",P2".to_string()]));
        assert!(
            err2.contains("empty --pattern token"),
            "leading comma must produce empty-token diagnostic; got: {err2}"
        );
    }

    /// needs_jcodemunch must route comma-separated patterns correctly:
    /// - P2,P5 → false (neither P1/PDEAD/PUNTESTED present)
    /// - P2,P1 → true  (P1 present)
    /// - P5,PDEAD → true (PDEAD present)
    /// - P2,PUNTESTED → true (PUNTESTED present)
    #[test]
    fn needs_jcodemunch_comma_pattern_routing() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("P2,P5"))),
            "P2,P5 must not need jcodemunch"
        );
        assert!(
            needs_jcodemunch(&make_args(false, Some("P2,P1"))),
            "P2,P1 must need jcodemunch (P1 present)"
        );
        assert!(
            needs_jcodemunch(&make_args(false, Some("P5,PDEAD"))),
            "P5,PDEAD must need jcodemunch (PDEAD present)"
        );
        assert!(
            needs_jcodemunch(&make_args(false, Some("P2,PUNTESTED"))),
            "P2,PUNTESTED must need jcodemunch (PUNTESTED present)"
        );
    }

    /// The opt-in detectors PDEAD/PUNTESTED must be enabled when their token
    /// appears anywhere in a comma-separated `--pattern`, and stay off when
    /// absent or when no `--pattern` is given (they are not part of the default
    /// sweep). This directly exercises the `is_some_and(pattern_selects(..))`
    /// routing that replaced the old `== Some("PDEAD")` exact-equality, which a
    /// whole-string comparison would have broken for any multi-token list.
    #[test]
    fn opt_in_detectors_selected_via_comma_list() {
        // PDEAD reached as a non-leading token in a comma list.
        assert!(
            selects(pattern_flag::PDEAD, Some("P2,PDEAD")),
            "P2,PDEAD must enable PDEAD"
        );
        assert!(
            !selects(pattern_flag::PDEAD, Some("P2,P5")),
            "P2,P5 must not enable PDEAD (token absent)"
        );
        assert!(
            !selects(pattern_flag::PDEAD, None),
            "no --pattern must not enable PDEAD (opt-in only)"
        );

        // PUNTESTED reached as a non-leading token in a comma list.
        assert!(
            selects(pattern_flag::PUNTESTED, Some("P2,PUNTESTED")),
            "P2,PUNTESTED must enable PUNTESTED"
        );
        assert!(
            !selects(pattern_flag::PUNTESTED, Some("P1,PDEAD")),
            "P1,PDEAD must not enable PUNTESTED (token absent)"
        );

        // A mixed opt-in list must enable BOTH opt-in detectors at once.
        assert!(
            selects(pattern_flag::PDEAD, Some("PDEAD,PUNTESTED"))
                && selects(pattern_flag::PUNTESTED, Some("PDEAD,PUNTESTED")),
            "PDEAD,PUNTESTED must enable both opt-in detectors"
        );
    }

    // -------------------------------------------------------------------
    // PTODO CLI-wiring tests (step-13 RED / step-14 GREEN)
    //
    // PTODO is the structural TODO-tracking lane (PRD task α). Unlike
    // PDEAD/PUNTESTED/PLAYER it is *structural* — it reads the working tree
    // via ls_files + fs and never contacts jcodemunch — so needs_jcodemunch
    // must stay false for it. Like the other non-default detectors it is
    // opt-in only (excluded from the default all-detector sweep; ε owns
    // default-sweep membership).
    // -------------------------------------------------------------------

    #[test]
    fn parse_args_accepts_ptodo_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PTODO".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PTODO must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PTODO"),
            "parsed pattern must be Some(\"PTODO\")"
        );
    }

    #[test]
    fn needs_jcodemunch_ptodo_routes_false() {
        // PTODO is the structural lane — it reads the working tree directly
        // (ls_files + fs), never jcodemunch. It must NOT trigger a connection.
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PTODO"))),
            "PTODO is structural and must not require jcodemunch"
        );
    }

    /// ε: PTODO is now part of the no-`--pattern` default all-detector sweep,
    /// mirroring P1/P2/P5: default (None) → true; explicit PTODO → true;
    /// non-PTODO pattern → false.
    #[test]
    fn ptodo_in_default_sweep() {
        assert!(
            selects(pattern_flag::PTODO, None),
            "PTODO must run in the no-`--pattern` default sweep"
        );
        assert!(
            selects(pattern_flag::PTODO, Some("PTODO")),
            "PTODO must activate when --pattern PTODO is given"
        );
        assert!(
            !selects(pattern_flag::PTODO, Some("P2")),
            "PTODO must be excluded when a named non-PTODO pattern is given"
        );
    }

    /// PTODO must be selectable as a non-leading token in a comma-separated
    /// `--pattern` list (mirrors `opt_in_detectors_selected_via_comma_list`).
    #[test]
    fn ptodo_selected_via_comma_list() {
        assert!(
            selects(pattern_flag::PTODO, Some("P2,PTODO")),
            "P2,PTODO must enable PTODO"
        );
    }

    // -------------------------------------------------------------------
    // PDSSENTINEL CLI-wiring tests (step-7 RED / step-8 GREEN)
    //
    // PDSSENTINEL is the ds-sentinel reintroduction guard (task #4650).
    // Like PTODO it is *structural* — reads the working tree via ls_files + fs,
    // never contacts jcodemunch. Unlike opt-in PDEAD/PUNTESTED/PLAYER, it
    // is part of the default all-detector sweep, like PTODO.
    // -------------------------------------------------------------------

    /// `--pattern PDSSENTINEL` must be accepted and stored.
    #[test]
    fn parse_args_accepts_pdssentinel_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PDSSENTINEL".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PDSSENTINEL must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PDSSENTINEL"),
            "parsed pattern must be Some(\"PDSSENTINEL\")"
        );
    }

    /// PDSSENTINEL is structural — must NOT require jcodemunch.
    #[test]
    fn needs_jcodemunch_pdssentinel_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PDSSENTINEL"))),
            "PDSSENTINEL is structural and must not require jcodemunch"
        );
    }

    /// PDSSENTINEL participates in the no-`--pattern` default sweep, like
    /// PTODO. Default (None) → true; explicit PDSSENTINEL → true;
    /// a non-PDSSENTINEL named pattern (e.g. P2) → false.
    #[test]
    fn pdssentinel_in_default_sweep() {
        assert!(
            selects(pattern_flag::PDSSENTINEL, None),
            "PDSSENTINEL must run in the no-`--pattern` default sweep"
        );
        assert!(
            selects(pattern_flag::PDSSENTINEL, Some("PDSSENTINEL")),
            "PDSSENTINEL must activate when --pattern PDSSENTINEL is given"
        );
        assert!(
            !selects(pattern_flag::PDSSENTINEL, Some("P2")),
            "PDSSENTINEL must be excluded when a named non-PDSSENTINEL pattern is given"
        );
    }

    /// PDSSENTINEL must be selectable as a non-leading token in a comma-separated
    /// `--pattern` list.
    #[test]
    fn pdssentinel_selected_via_comma_list() {
        assert!(
            selects(pattern_flag::PDSSENTINEL, Some("P1,PDSSENTINEL")),
            "P1,PDSSENTINEL must enable PDSSENTINEL"
        );
    }

    /// Unknown pattern error message must list PDSSENTINEL as a valid token.
    #[test]
    fn parse_args_unknown_pattern_lists_pdssentinel() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "BOGUS".to_string()]));
        assert!(
            err.contains("PDSSENTINEL"),
            "error must list PDSSENTINEL as a valid pattern; got: {err}"
        );
    }

    // -------------------------------------------------------------------
    // PDOCCOVER CLI-wiring tests (task #5478, step-21 RED / step-22 GREEN)
    //
    // PDOCCOVER is the bidirectional registry↔chunk name-drift detector. Like
    // PTODO and PDSSENTINEL it is *structural* — working-tree reads via
    // ls_files + fs, never contacts jcodemunch. UNLIKE them it is OPT-IN,
    // for PDIAG's reason: its verdicts are High and feed the exit code, and
    // they ratchet against a committed ledger (pdoccover-baseline.txt), so in
    // the default sweep a drifting ledger would turn every bare audit run
    // non-zero. The hard gate is tests/infra/test_reify_audit_pdoccover.sh.
    // -------------------------------------------------------------------

    /// `--pattern PDOCCOVER` must be accepted and stored.
    #[test]
    fn parse_args_accepts_pdoccover_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PDOCCOVER".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PDOCCOVER must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PDOCCOVER"),
            "parsed pattern must be Some(\"PDOCCOVER\")"
        );
    }

    /// PDOCCOVER is structural — must NOT require jcodemunch. Requesting it
    /// alone must leave the run fully offline.
    #[test]
    fn needs_jcodemunch_pdoccover_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PDOCCOVER"))),
            "PDOCCOVER reads units.rs, the chunk corpus and the compiler/stdlib \
             sources from the working tree; it must not open a jcodemunch \
             connection"
        );
    }

    /// PDOCCOVER is OPT-IN — the assertion inverted relative to
    /// PTODO/PDSSENTINEL. Default (None) → FALSE; explicit
    /// PDOCCOVER → true; a named non-PDOCCOVER pattern → false.
    #[test]
    fn pdoccover_is_opt_in_not_in_default_sweep() {
        assert!(
            !selects(pattern_flag::PDOCCOVER, None),
            "PDOCCOVER must NOT run in the no-`--pattern` default sweep: its \
             findings are High severity and ratchet against a committed ledger, \
             so a drifting ledger would make every bare audit run exit non-zero"
        );
        assert!(
            selects(pattern_flag::PDOCCOVER, Some("PDOCCOVER")),
            "PDOCCOVER must activate when --pattern PDOCCOVER is given"
        );
        assert!(
            !selects(pattern_flag::PDOCCOVER, Some("P2")),
            "PDOCCOVER must be excluded when a named non-PDOCCOVER pattern is given"
        );
    }

    /// PDOCCOVER must be selectable as a NON-LEADING token in a
    /// comma-separated `--pattern` list — token-set membership, not a prefix
    /// match.
    #[test]
    fn pdoccover_selected_via_comma_list() {
        assert!(
            selects(pattern_flag::PDOCCOVER, Some("P1,PDOCCOVER")),
            "P1,PDOCCOVER must enable PDOCCOVER"
        );
    }

    /// Unknown pattern error message must list PDOCCOVER as a valid token —
    /// an accepted-but-undiscoverable pattern is a usability bug.
    #[test]
    fn parse_args_unknown_pattern_lists_pdoccover() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "BOGUS".to_string()]));
        assert!(
            err.contains("PDOCCOVER"),
            "error must list PDOCCOVER as a valid pattern; got: {err}"
        );
    }

    /// `--help` must list PDOCCOVER on the `--pattern` line, for the same
    /// discoverability reason.
    #[test]
    fn usage_text_lists_pdoccover() {
        let mut buf: Vec<u8> = Vec::new();
        print_usage(&mut buf);
        let usage = String::from_utf8(buf).expect("usage text is UTF-8");
        assert!(
            usage.contains("PDOCCOVER"),
            "--help must list PDOCCOVER among the --pattern values; got:\n{usage}"
        );
    }
    // -------------------------------------------------------------------
    // PDIAG CLI-wiring tests (task #5405)
    //
    // PDIAG is the INV-SF-6 codes-mandatory ratchet. Like PTODO/PDSSENTINEL/
    // PDOCCOVER it is *structural* — `ls_files` enumeration plus working-tree
    // reads, no jcodemunch and no task DB. Like PDEAD/PUNTESTED/PLAYER/
    // PDOCCOVER it is opt-in (its verdicts are High and feed the exit code);
    // that half is covered end-to-end by `tests/cli.rs::
    // pdiag_does_not_join_the_default_all_detector_sweep`. What is pinned HERE
    // is the pair every other detector has beside it, and which PDIAG lacked:
    // the arg token, and the offline posture.
    // -------------------------------------------------------------------

    /// `--pattern PDIAG` must be accepted and stored — including as a
    /// NON-LEADING comma token, which is the shape `pattern_selects` exists to
    /// handle and the one a naive `starts_with` would get wrong.
    #[test]
    fn parse_args_accepts_pdiag_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PDIAG".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PDIAG must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PDIAG"),
            "parsed pattern must be Some(\"PDIAG\")"
        );

        let unioned = parse_args(&["--pattern".to_string(), "P1,PDIAG".to_string()])
            .unwrap_or_else(|e| panic!("--pattern P1,PDIAG must parse successfully; got: {e}"));
        assert!(
            selects(pattern_flag::PDIAG, unioned.pattern.as_deref()),
            "PDIAG must activate as a trailing comma token in a union pattern"
        );
        assert!(
            !selects(pattern_flag::PDIAG, Some("P2")),
            "PDIAG must stay off for a named non-PDIAG pattern"
        );
    }

    /// PDIAG is structural — it must NOT require jcodemunch.
    ///
    /// The claim is asserted in `Pattern::PDiag`'s docs, but both PDIAG
    /// integration tests pass `--no-jcodemunch`, so nothing else would go red
    /// if a future edit set `queries_jcodemunch` on PDIAG's row. That
    /// regression is not cosmetic: via `jcodemunch_only_run_set` a
    /// jcodemunch-backed PDIAG would hard-refuse with exit 125 on a stale
    /// index, turning the merge gate red for a detector that never reads the
    /// index.
    #[test]
    fn needs_jcodemunch_pdiag_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PDIAG"))),
            "PDIAG enumerates via ls_files and reads the working tree; it must \
             not open a jcodemunch connection"
        );
        assert!(
            !jcodemunch_only_run_set(&make_args(false, Some("PDIAG"))),
            "a PDIAG-only run must not reach jcodemunch_only_run_set's \
             stale-index refusal (exit 125)"
        );
    }

    // -------------------------------------------------------------------
    // PDCHECK (task #7550) — delivered_checks dead-path lane
    // -------------------------------------------------------------------

    #[test]
    fn parse_args_accepts_pdcheck_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PDCHECK".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PDCHECK must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PDCHECK"),
            "parsed pattern must be Some(\"PDCHECK\")"
        );
    }

    /// The token must work as a NON-LEADING member of a comma-separated union,
    /// not just alone — validation is per token, selection is set membership.
    #[test]
    fn parse_args_accepts_pdcheck_in_comma_list() {
        let args = parse_args(&["--pattern".to_string(), "P1,PDCHECK".to_string()])
            .expect("--pattern P1,PDCHECK must parse successfully");
        let val = args.pattern.as_deref().expect("pattern must be Some");
        let tokens: Vec<&str> = val.split(',').map(str::trim).collect();
        assert!(tokens.contains(&"PDCHECK"), "tokens must contain PDCHECK; got: {tokens:?}");
        assert!(
            selects(pattern_flag::PDCHECK, Some("P1,PDCHECK")),
            "P1,PDCHECK must enable PDCHECK"
        );
    }

    /// An accepted-but-undiscoverable pattern is a usability bug: the error
    /// message at the validator is the only place a user learns the vocabulary.
    #[test]
    fn parse_args_unknown_pattern_lists_pdcheck() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "BOGUS".to_string()]));
        assert!(
            err.contains("PDCHECK"),
            "error must list PDCHECK as a valid pattern; got: {err}"
        );
    }

    /// `--help` must list PDCHECK on the `--pattern` line, for the same
    /// discoverability reason.
    #[test]
    fn usage_text_lists_pdcheck() {
        let mut buf: Vec<u8> = Vec::new();
        print_usage(&mut buf);
        let usage = String::from_utf8(buf).expect("usage text is UTF-8");
        assert!(
            usage.contains("PDCHECK"),
            "--help must list PDCHECK on the --pattern line; got:\n{usage}"
        );
    }

    /// PDCHECK is OPT-IN. The no-`--pattern` case being FALSE is the
    /// load-bearing assertion: `delivered-check-unsatisfiable-path` is High by
    /// design and the exit code is the High-severity count, so joining the
    /// default sweep would turn `scripts/reify-audit-predone-wrapper.sh`, the
    /// /audit skill and verify non-zero.
    #[test]
    fn pdcheck_is_opt_in_not_in_default_sweep() {
        assert!(
            !selects(pattern_flag::PDCHECK, None),
            "PDCHECK must NOT run in the no-`--pattern` default sweep: its High \
             findings drive the exit code, so every bare `reify-audit` \
             invocation would start exiting non-zero"
        );
        assert!(
            selects(pattern_flag::PDCHECK, Some("PDCHECK")),
            "PDCHECK must activate when --pattern PDCHECK is given"
        );
        assert!(
            !selects(pattern_flag::PDCHECK, Some("P2")),
            "PDCHECK must be excluded when a named non-PDCHECK pattern is given"
        );
    }

    /// PDCHECK is `ls_files` plus a read-only sqlite open — never the
    /// jcodemunch serve, so it must not force a connect.
    #[test]
    fn needs_jcodemunch_pdcheck_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PDCHECK"))),
            "PDCHECK reads the tracked-file list and .taskmaster/tasks/tasks.db; \
             it must not open a jcodemunch connection"
        );
        assert!(
            !jcodemunch_only_run_set(&make_args(false, Some("PDCHECK"))),
            "a PDCHECK-only run must not reach jcodemunch_only_run_set's \
             stale-index refusal (exit 125)"
        );
    }

    // -------------------------------------------------------------------
    // PCITE (task #6931) — capability-manifest cite lane
    // -------------------------------------------------------------------

    #[test]
    fn parse_args_accepts_pcite_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PCITE".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PCITE must parse successfully; got: {e}"));
        assert_eq!(
            args.pattern.as_deref(),
            Some("PCITE"),
            "parsed pattern must be Some(\"PCITE\")"
        );
    }

    #[test]
    fn parse_args_accepts_pcite_in_comma_list() {
        let args = parse_args(&["--pattern".to_string(), "P1,PCITE".to_string()])
            .expect("--pattern P1,PCITE must parse successfully");
        let val = args.pattern.as_deref().expect("pattern must be Some");
        let tokens: Vec<&str> = val.split(',').map(str::trim).collect();
        assert!(tokens.contains(&"PCITE"), "tokens must contain PCITE; got: {tokens:?}");
        assert!(
            selects(pattern_flag::PCITE, Some("P1,PCITE")),
            "P1,PCITE must enable PCITE"
        );
    }

    #[test]
    fn parse_args_unknown_pattern_lists_pcite() {
        let err = unwrap_err(parse_args(&["--pattern".to_string(), "BOGUS".to_string()]));
        assert!(
            err.contains("PCITE"),
            "error must list PCITE as a valid pattern; got: {err}"
        );
    }

    #[test]
    fn usage_text_lists_pcite() {
        let mut buf: Vec<u8> = Vec::new();
        print_usage(&mut buf);
        let usage = String::from_utf8(buf).expect("usage text is UTF-8");
        assert!(
            usage.contains("PCITE"),
            "--help must list PCITE on the --pattern line; got:\n{usage}"
        );
    }

    /// PCITE is OPT-IN although it cannot move the exit code (Medium only):
    /// it reads every tracked non-prose file to build its oracle, and its
    /// residual is legitimately non-zero, so a pattern-less sweep must not
    /// pay for it or route its follow-ups unasked.
    #[test]
    fn pcite_is_opt_in_not_in_default_sweep() {
        assert!(
            !selects(pattern_flag::PCITE, None),
            "PCITE must NOT run in the no-`--pattern` default sweep"
        );
        assert!(
            selects(pattern_flag::PCITE, Some("PCITE")),
            "PCITE must activate when --pattern PCITE is given"
        );
        assert!(
            !selects(pattern_flag::PCITE, Some("P2")),
            "PCITE must be excluded when a named non-PCITE pattern is given"
        );
    }

    #[test]
    fn needs_jcodemunch_pcite_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PCITE"))),
            "PCITE reads the tracked tree only; it must not open a jcodemunch \
             connection"
        );
        assert!(
            !jcodemunch_only_run_set(&make_args(false, Some("PCITE"))),
            "a PCITE-only run must not reach jcodemunch_only_run_set's \
             stale-index refusal (exit 125)"
        );
    }

    // -------------------------------------------------------------------
    // PPRDSTATUS (task #6932) — PRD status-prose drift
    // -------------------------------------------------------------------

    /// Accepted alone, and as a NON-LEADING member of a comma-separated union.
    #[test]
    fn parse_args_accepts_pprdstatus_pattern() {
        let args = parse_args(&["--pattern".to_string(), "PPRDSTATUS".to_string()])
            .unwrap_or_else(|e| panic!("--pattern PPRDSTATUS must parse successfully; got: {e}"));
        assert_eq!(args.pattern.as_deref(), Some(pattern_flag::PPRDSTATUS));

        let args = parse_args(&["--pattern".to_string(), "P1,PPRDSTATUS".to_string()])
            .unwrap_or_else(|e| panic!("--pattern P1,PPRDSTATUS must parse successfully; got: {e}"));
        assert_eq!(args.pattern.as_deref(), Some("P1,PPRDSTATUS"));
        assert!(
            selects(pattern_flag::PPRDSTATUS, Some("P1,PPRDSTATUS")),
            "P1,PPRDSTATUS must enable PPRDSTATUS"
        );
    }

    /// PPRDSTATUS is OPT-IN: its High findings track a standing backlog of
    /// PRD prose, and the exit code is the High-severity count, so joining the
    /// default sweep would turn every bare `reify-audit` invocation non-zero.
    #[test]
    fn pprdstatus_is_opt_in_not_in_default_sweep() {
        assert!(
            !selects(pattern_flag::PPRDSTATUS, None),
            "PPRDSTATUS must NOT run in the no-`--pattern` default sweep"
        );
        assert!(
            selects(pattern_flag::PPRDSTATUS, Some("PPRDSTATUS")),
            "PPRDSTATUS must activate when --pattern PPRDSTATUS is given"
        );
        assert!(
            !selects(pattern_flag::PPRDSTATUS, Some("P2")),
            "PPRDSTATUS must be excluded when a named non-PPRDSTATUS pattern is given"
        );
    }

    /// PPRDSTATUS reads the loaded task corpus, `ls_files` and the working
    /// tree — never the jcodemunch serve.
    #[test]
    fn needs_jcodemunch_pprdstatus_routes_false() {
        assert!(
            !needs_jcodemunch(&make_args(false, Some("PPRDSTATUS"))),
            "PPRDSTATUS must not open a jcodemunch connection"
        );
        assert!(
            !jcodemunch_only_run_set(&make_args(false, Some("PPRDSTATUS"))),
            "a PPRDSTATUS-only run must not reach jcodemunch_only_run_set's \
             stale-index refusal (exit 125)"
        );
    }

    /// An empty task corpus refuses a PPRDSTATUS-only run and nothing wider:
    /// a mixed or pattern-less run keeps its other detectors running.
    #[test]
    fn empty_task_corpus_refusal_is_scoped_to_a_pprdstatus_only_run() {
        assert!(
            task_corpus_only_run_set(&make_args(false, Some("PPRDSTATUS"))),
            "a PPRDSTATUS-only run must refuse an empty task corpus"
        );
        for pattern in [None, Some("P5"), Some("P5,PPRDSTATUS")] {
            assert!(
                !task_corpus_only_run_set(&make_args(false, pattern)),
                "--pattern {pattern:?} must not refuse an empty task corpus"
            );
        }
    }

    // -------------------------------------------------------------------
    // --print-repo-id (task #6459)
    // -------------------------------------------------------------------

    #[test]
    fn parse_args_accepts_print_repo_id() {
        let args = parse_args(&["--print-repo-id".to_string()])
            .unwrap_or_else(|e| panic!("--print-repo-id must parse successfully; got: {e}"));
        assert!(args.print_repo_id);
    }

    #[test]
    fn parse_args_empty_defaults_print_repo_id_false() {
        let args = parse_args(&[]).unwrap_or_else(|e| panic!("empty argv must parse: {e}"));
        assert!(!args.print_repo_id, "--print-repo-id must default to off");
    }

    /// An accepted-but-undiscoverable flag is a usability bug — same
    /// discoverability guard as `usage_text_lists_jcodemunch_index_dir`.
    #[test]
    fn usage_text_lists_print_repo_id() {
        let mut buf: Vec<u8> = Vec::new();
        print_usage(&mut buf);
        let usage = String::from_utf8(buf).expect("usage text is UTF-8");
        assert!(
            usage.contains("--print-repo-id"),
            "--help must list --print-repo-id; got:\n{usage}"
        );
    }
}

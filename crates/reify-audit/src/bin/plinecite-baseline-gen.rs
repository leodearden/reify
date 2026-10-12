//! `plinecite-baseline-gen` — the one regenerator of
//! `crates/reify-audit/plinecite-baseline.txt`, PLINECITE's ratchet ledger.
//!
//! ```text
//! cargo run -p reify-audit --bin plinecite-baseline-gen -- --project-root . \
//!   > crates/reify-audit/plinecite-baseline.txt
//! ```
//!
//! SHRINK-ONLY by default: it writes `Ledger::kept()` — the committed rows
//! that still match a live cite — so stale rows drop out and nothing is added.
//! `--admit-new` writes ALL live debt instead; it is the only way the ledger
//! grows, and every row it adds must be justified in review.
//!
//! This binary owns only that flag. The rows are `plinecite::baseline_ledger`,
//! the same derivation `plinecite::check` settles, so a regenerated ledger is
//! by construction one the ratchet accepts; the bytes are
//! `plinecite_baseline::render_baseline`.
//!
//! Stdout is the ledger and nothing else; diagnostics go to stderr. Exit codes:
//! `0` after rendering a ledger; `2` on `-h`/`--help` or a bad argument, which
//! render none (the shell has already truncated the redirect target, so no
//! such path may report success); `3` when `baseline_ledger` refuses a tree
//! with no tracked file in scope — which is also what any git failure
//! degrades to — with NOTHING on stdout, because a shrink-only render over it
//! would wipe the ledger.

use std::path::PathBuf;

use reify_audit::plinecite::baseline_ledger;
use reify_audit::plinecite_baseline::render_baseline;
use reify_audit::{AuditContext, NoopJCodemunchOps, RealGitOps};

const USAGE: &str = "\
Usage: plinecite-baseline-gen [--project-root <path>] [--admit-new]
Emits the PLINECITE ratchet ledger to stdout. Redirect into
crates/reify-audit/plinecite-baseline.txt.
  default       keep only committed rows that still match a live cite
  --admit-new   ledger ALL live debt (justify every added row in review)";

struct Args {
    project_root: PathBuf,
    admit_new: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        project_root: PathBuf::from("."),
        admit_new: false,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--project-root" => match argv.next() {
                Some(root) => args.project_root = PathBuf::from(root),
                None => refuse("--project-root requires a value"),
            },
            "--admit-new" => args.admit_new = true,
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
            other => refuse(&format!("unknown argument {other:?}")),
        }
    }
    args
}

fn refuse(reason: &str) -> ! {
    eprintln!("plinecite-baseline-gen: {reason}\n{USAGE}");
    std::process::exit(2);
}

fn main() {
    let args = parse_args();
    let git = RealGitOps::new(args.project_root.clone());
    // PLINECITE is structural: the task DB and jcodemunch are never read, but
    // `AuditContext` requires both seams.
    let conn = rusqlite::Connection::open_in_memory()
        .expect("in-memory sqlite connection for AuditContext placeholder");
    let ctx = AuditContext {
        project_root: args.project_root.clone(),
        conn: &conn,
        git: &git,
        jcodemunch: &NoopJCodemunchOps,
        task_metadata: std::collections::HashMap::new(),
        target_task_id: None,
        window: None,
        now: None,
        producer_branch: None,
    };

    let ledger = match baseline_ledger(&ctx) {
        Ok(ledger) => ledger,
        Err(empty) => {
            eprintln!("plinecite-baseline-gen: {empty}");
            std::process::exit(3);
        }
    };

    let rows = if args.admit_new {
        ledger.live.clone()
    } else {
        ledger.kept()
    };
    print!("{}", render_baseline(&rows));

    let not_admitted = if args.admit_new {
        0
    } else {
        ledger.new_debt().len()
    };
    eprintln!(
        "plinecite-baseline-gen: wrote {} row(s); dropped {} stale row(s); \
         {not_admitted} new-debt row(s) NOT admitted — cite the enclosing symbol, \
         or mark a deliberate dated cite `plinecite:allow — <reason>`; \
         `--admit-new` ledgers new debt, and every added row must be justified \
         in review",
        rows.len(),
        ledger.stale().len(),
    );
}

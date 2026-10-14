//! The driver every ratchet ledger's `*-baseline-gen` binary shares. A lane
//! supplies a [`Regenerator`] — its name, usage, ledger derivation, renderer
//! and remedy — and this module owns the rest: the argv grammar, the
//! shrink-only default against `--admit-new`, and the exit-code contract.
//!
//! Stdout is the ledger and nothing else; diagnostics go to stderr. Exit
//! codes: `0` after rendering a ledger; `2` on `-h`/`--help` or a bad
//! argument, which render none (the shell has already truncated the redirect
//! target, so no such path may report success); `3` when the lane's
//! derivation refuses its tree, with NOTHING on stdout, because a shrink-only
//! render over a tree that was never read would wipe the ledger.

use super::Ledger;
use crate::{AuditContext, NoopJCodemunchOps, RealGitOps};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

/// What one lane's regenerator binary does not share with the others.
pub struct Regenerator<R: Ord + Clone, E: fmt::Display> {
    /// The binary's name; it prefixes every stderr line.
    pub name: &'static str,
    /// Printed on stderr for `--help` and after a refused argument.
    pub usage: &'static str,
    /// The lane's live and committed debt — the same derivation its `check`
    /// settles, so a regenerated ledger is by construction one the ratchet
    /// accepts. An `Err` refuses the tree, and is reported on stderr.
    pub derive: fn(&AuditContext<'_>) -> Result<Ledger<R>, E>,
    /// The ledger file's bytes for a set of rows.
    pub render: fn(&BTreeSet<R>) -> String,
    /// What the stderr summary says was written, e.g. `3 row(s)`.
    pub describe_rows: fn(&BTreeSet<R>) -> String,
    /// How to clear new debt instead of admitting it.
    pub remedy: &'static str,
}

impl<R: Ord + Clone, E: fmt::Display> Regenerator<R, E> {
    /// The binary's whole `main`, over the process's own argv.
    pub fn main(&self) -> ExitCode {
        match parse_args(std::env::args().skip(1)) {
            Ok(args) => self.regenerate(&args),
            Err(NoRun::Help) => {
                eprintln!("{}", self.usage);
                ExitCode::from(2)
            }
            Err(NoRun::Refused(reason)) => {
                eprintln!("{}: {reason}\n{}", self.name, self.usage);
                ExitCode::from(2)
            }
        }
    }

    fn regenerate(&self, args: &Args) -> ExitCode {
        let git = RealGitOps::new(args.project_root.clone());
        // A ratchet lane is structural: the task DB and jcodemunch are never
        // read, but `AuditContext` requires both seams.
        let conn = rusqlite::Connection::open_in_memory()
            .expect("in-memory sqlite connection for AuditContext placeholder");
        let ctx = AuditContext {
            project_root: args.project_root.clone(),
            conn: &conn,
            git: &git,
            jcodemunch: &NoopJCodemunchOps,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        let ledger = match (self.derive)(&ctx) {
            Ok(ledger) => ledger,
            Err(refusal) => {
                eprintln!("{}: {refusal}", self.name);
                return ExitCode::from(3);
            }
        };

        let (rows, not_admitted) = if args.admit_new {
            (ledger.live.clone(), 0)
        } else {
            (ledger.kept(), ledger.new_debt().len())
        };
        print!("{}", (self.render)(&rows));
        eprintln!(
            "{}: wrote {}; dropped {} stale row(s); {not_admitted} new-debt row(s) NOT \
             admitted — {}; `--admit-new` ledgers new debt, and every added row must \
             be justified in review",
            self.name,
            (self.describe_rows)(&rows),
            ledger.stale().len(),
            self.remedy,
        );
        ExitCode::SUCCESS
    }
}

struct Args {
    project_root: PathBuf,
    admit_new: bool,
}

/// Why argv names no regeneration.
enum NoRun {
    Help,
    Refused(String),
}

/// Only `--project-root <path>` names the root: a stray positional is refused
/// like an unknown flag, never silently taken as a different tree.
fn parse_args(mut argv: impl Iterator<Item = String>) -> Result<Args, NoRun> {
    let mut args = Args {
        project_root: PathBuf::from("."),
        admit_new: false,
    };
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--project-root" => match argv.next() {
                Some(root) => args.project_root = PathBuf::from(root),
                None => return Err(NoRun::Refused("--project-root requires a value".into())),
            },
            "--admit-new" => args.admit_new = true,
            "-h" | "--help" => return Err(NoRun::Help),
            other => return Err(NoRun::Refused(format!("unknown argument {other:?}"))),
        }
    }
    Ok(args)
}

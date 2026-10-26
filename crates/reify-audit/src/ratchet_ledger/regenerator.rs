//! The driver every ratchet ledger's `*-baseline-gen` binary shares. A lane
//! supplies a [`Regenerator`] — its name, usage, ledger derivation, renderer
//! and remedy — and this module owns the rest: the argv grammar, the
//! shrink-only default against `--admit-new`, `--write`, and the exit-code
//! contract.
//!
//! Without `--write`, stdout is the ledger and nothing else; diagnostics go
//! to stderr. With `--write`, the ledger replaces the lane's file in place,
//! through a sibling temp file renamed over it. That is the way to regenerate:
//! the committed rows are read from the working tree, so redirecting stdout
//! into the ledger itself hands the run a file the shell has already
//! truncated, and a shrink-only render of it keeps nothing.
//!
//! Exit codes: `0` after rendering a ledger; `2` on `-h`/`--help` or a bad
//! argument, which render none (under a redirect the shell has already
//! truncated the target, so no such path may report success); `3`, with
//! NOTHING rendered, when the lane's derivation refuses its tree, when a
//! shrink-only run finds the ledger file zero bytes long — a rendered ledger
//! always carries its header, so zero bytes is a truncation, never debt — or
//! when `--write` cannot replace the file.

use super::Ledger;
use crate::{AuditContext, NoopJCodemunchOps, RealGitOps};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What one lane's regenerator binary does not share with the others.
pub struct Regenerator<R: Ord + Clone, E: fmt::Display> {
    /// The binary's name; it prefixes every stderr line.
    pub name: &'static str,
    /// Printed on stderr for `--help` and after a refused argument.
    pub usage: &'static str,
    /// The ledger file, relative to the project root: what `--write`
    /// replaces, and what a shrink-only run refuses to find truncated.
    pub ledger: &'static str,
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
        let ledger_file = args.project_root.join(self.ledger);
        if !args.admit_new && is_truncated(&ledger_file) {
            eprintln!(
                "{}: {} is zero bytes long — truncated, most likely by redirecting this \
                 run's stdout into it — so a shrink-only render would keep no row. \
                 Restore it with `git checkout -- {}` and regenerate with `--write`",
                self.name, self.ledger, self.ledger,
            );
            return ExitCode::from(3);
        }
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
        let rendered = (self.render)(&rows);
        if args.write {
            if let Err(error) = replace_file(&ledger_file, &rendered) {
                eprintln!("{}: cannot write {}: {error}", self.name, self.ledger);
                return ExitCode::from(3);
            }
        } else {
            print!("{rendered}");
        }
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

/// A ledger that exists but holds no byte, not even its header.
fn is_truncated(ledger_file: &Path) -> bool {
    std::fs::metadata(ledger_file).is_ok_and(|meta| meta.is_file() && meta.len() == 0)
}

/// Replaces `path` with `content` so that no reader, and no failure, ever
/// sees a partial ledger.
fn replace_file(path: &Path, content: &str) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, content)
        .and_then(|()| std::fs::rename(&temp, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
}

struct Args {
    project_root: PathBuf,
    admit_new: bool,
    write: bool,
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
        write: false,
    };
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--project-root" => match argv.next() {
                Some(root) => args.project_root = PathBuf::from(root),
                None => return Err(NoRun::Refused("--project-root requires a value".into())),
            },
            "--admit-new" => args.admit_new = true,
            "--write" => args.write = true,
            "-h" | "--help" => return Err(NoRun::Help),
            other => return Err(NoRun::Refused(format!("unknown argument {other:?}"))),
        }
    }
    Ok(args)
}

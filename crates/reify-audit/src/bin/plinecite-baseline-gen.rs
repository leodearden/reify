//! `plinecite-baseline-gen` — the one regenerator of
//! `crates/reify-audit/plinecite-baseline.txt`, PLINECITE's ratchet ledger.
//!
//! ```text
//! cargo run -p reify-audit --bin plinecite-baseline-gen -- --project-root . --write
//! ```
//!
//! `--write` replaces the ledger in place. Never redirect stdout into the
//! ledger instead: the shell truncates it before the run reads its rows.
//!
//! SHRINK-ONLY by default: it writes `Ledger::kept()` — the committed rows
//! that still match a live cite — so stale rows drop out and nothing is added.
//! `--admit-new` writes ALL live debt instead; it is the only way the ledger
//! grows, and every row it adds must be justified in review.
//!
//! The rows are `plinecite::baseline_ledger`, the same derivation
//! `plinecite::check` settles; the bytes are
//! `plinecite_baseline::render_baseline`. Argv, the stdout/stderr split and
//! the exit-code contract are the shared `ratchet_ledger::regenerator`
//! driver's. Its refusal (exit `3`, nothing on stdout) here means no tracked
//! file is in scope, which is also what any git failure degrades to.

use std::process::ExitCode;

use reify_audit::plinecite::baseline_ledger;
use reify_audit::plinecite_baseline::{BASELINE_PATH, render_baseline};
use reify_audit::ratchet_ledger::regenerator::Regenerator;

const USAGE: &str = "\
Usage: plinecite-baseline-gen [--project-root <path>] [--admit-new] [--write]
Emits the PLINECITE ratchet ledger to stdout, or with --write replaces
crates/reify-audit/plinecite-baseline.txt in place. Never redirect stdout
into that file: the shell truncates it before its rows are read.
  default       keep only committed rows that still match a live cite
  --admit-new   ledger ALL live debt (justify every added row in review)
  --write       replace the ledger file instead of printing it";

fn main() -> ExitCode {
    Regenerator {
        name: "plinecite-baseline-gen",
        usage: USAGE,
        ledger: BASELINE_PATH,
        derive: baseline_ledger,
        render: render_baseline,
        describe_rows: |rows| format!("{} row(s)", rows.len()),
        remedy: "cite the enclosing symbol, or mark a deliberate dated cite \
                 `plinecite:allow — <reason>`",
    }
    .main()
}

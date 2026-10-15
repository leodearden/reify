//! `pdoccover-baseline-gen` — the one regenerator of
//! `crates/reify-audit/pdoccover-baseline.txt`, PDOCCOVER's ratchet ledger.
//!
//! ```text
//! cargo run -p reify-audit --bin pdoccover-baseline-gen -- --project-root . --write
//! ```
//!
//! `--write` replaces the ledger in place. Never redirect stdout into the
//! ledger instead: the shell truncates it before the run reads its rows.
//!
//! SHRINK-ONLY by default: it writes `Ledger::kept()` — the committed rows
//! that still match live debt — so stale rows drop out and nothing is added.
//! `--admit-new` writes ALL live debt instead; it is the only way the ledger
//! grows, and every row it adds must be justified in review.
//!
//! The rows are `pdoccover::baseline_ledger`, the same derivation
//! `pdoccover::check` settles; the bytes are
//! `pdoccover_baseline::render_baseline`. Argv, the stdout/stderr split and
//! the exit-code contract are the shared `ratchet_ledger::regenerator`
//! driver's. Its refusal (exit `3`, nothing rendered) here also covers a
//! degenerate tree — an empty registry census or no readable chunk, which is
//! also what any git failure degrades to.

use std::collections::BTreeSet;
use std::process::ExitCode;

use reify_audit::pdoccover::baseline_ledger;
use reify_audit::pdoccover_baseline::{BASELINE_PATH, BaselineRow, render_baseline};
use reify_audit::ratchet_ledger::regenerator::Regenerator;

const USAGE: &str = "\
Usage: pdoccover-baseline-gen [--project-root <path>] [--admit-new] [--write]
Emits the PDOCCOVER ratchet ledger to stdout, or with --write replaces
crates/reify-audit/pdoccover-baseline.txt in place. Never redirect stdout
into that file: the shell truncates it before its rows are read.
  default       keep only committed rows that still match live debt
  --admit-new   ledger ALL live debt (justify every added row in review)
  --write       replace the ledger file instead of printing it";

fn describe_rows(rows: &BTreeSet<BaselineRow>) -> String {
    let omissions = rows
        .iter()
        .filter(|row| matches!(row, BaselineRow::Undocumented(_)))
        .count();
    format!(
        "{omissions} omission row(s) and {} fabrication row(s)",
        rows.len() - omissions
    )
}

fn main() -> ExitCode {
    Regenerator {
        name: "pdoccover-baseline-gen",
        usage: USAGE,
        ledger: BASELINE_PATH,
        derive: baseline_ledger,
        render: render_baseline,
        describe_rows,
        remedy: "document the name, fix the chunk, or mark the line \
                 `pdoccover:allow — <reason>`",
    }
    .main()
}

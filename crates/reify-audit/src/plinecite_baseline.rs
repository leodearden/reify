//! The PLINECITE ratchet ledger — the grammar of the committed
//! `crates/reify-audit/plinecite-baseline.txt`.
//!
//! The file lists PLINECITE's accepted line-cite debt, one [`CiteRow`] per
//! line. The ratchet and its generator settle these rows with
//! [`crate::ratchet_ledger::Ledger`]'s set algebra.
//!
//! This module knows nothing about how debt is found — `plinecite.rs` derives
//! the live rows; this is only the format of one committed file.

use std::collections::BTreeSet;
use std::fmt;

/// The committed ledger, read only when git-tracked (an untracked copy is
/// inert).
pub const BASELINE_PATH: &str = "crates/reify-audit/plinecite-baseline.txt";

/// The preamble [`render_baseline`] writes above the rows. Every line is a
/// `#` comment, so [`parse_baseline`] reads none of it as a row.
pub const BASELINE_HEADER: &str = "\
# PLINECITE baseline — accepted line-cite debt.
#
# One row per line, `<path> :: <cite>`: a tracked in-scope file, and a hard
# line cite one of its comment lines carries, quoted from its stem through its
# line number (`cache.rs:617`, `entity.rs ~2145`, `lib.rs line 444`; grammar:
# docs/prds/reify-audit-stale-line-cite-detector.md §4). The citing line's own
# number is deliberately absent, so a row survives code motion.
#
# GENERATED — do not hand-edit. Regenerate with:
#   cargo run -p reify-audit --bin plinecite-baseline-gen -- --project-root . --write
# Never redirect its stdout into this file: the shell truncates it first.
#
# The default regeneration only DROPS stale rows. `--admit-new` is the only way
# to add debt, and every row it adds must be justified in review; the real fix
# is to cite the enclosing symbol, or to mark a deliberate dated cite
# `plinecite:allow — <reason>`. Re-numbering a cite is not a fix: the new
# number is new debt.
#
# A row that matches no live cite is STALE, and `reify-audit --pattern
# PLINECITE` reports it as `stale-baseline-entry`: delete it, or rerun the
# default generator.
#
# A regeneration is valid for EXACTLY the tree it ran against. Any later rebase,
# merge, amend or cherry-pick is a different tree and invalidates it. Re-verify
# on the COMMITTED tree:
#   git status --porcelain    # must be empty
#   cargo run -p reify-audit --bin plinecite-baseline-gen -- --project-root . \\
#     | diff -u crates/reify-audit/plinecite-baseline.txt -
";

/// Between a row's path and its cite. A path holds no space and a cite can
/// (` esc-<id>`, ` ~`, ` line `), so a row splits at the first occurrence.
const SEPARATOR: &str = " :: ";

/// One line of the ledger: a citing file and one cite it carries.
///
/// The derived `Ord` sorts by path, then cite — the order [`render_baseline`]
/// writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CiteRow {
    pub path: String,
    pub cite: String,
}

impl CiteRow {
    /// Parse one non-comment line. Never fails: a line with no separator
    /// becomes a row with an empty cite, which no live cite matches, so the
    /// ratchet reports it stale.
    pub fn parse(line: &str) -> Self {
        let line = line.trim();
        let (path, cite) = line.split_once(SEPARATOR).unwrap_or((line, ""));
        Self {
            path: path.to_string(),
            cite: cite.to_string(),
        }
    }
}

impl fmt::Display for CiteRow {
    /// The row's line, so a stale row is named by the text to delete.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.cite.is_empty() {
            f.write_str(&self.path)
        } else {
            write!(f, "{}{SEPARATOR}{}", self.path, self.cite)
        }
    }
}

/// Every row in a ledger file. Blank and `#` lines are skipped; every other
/// line becomes a row, so no line is ever silently discarded.
pub fn parse_baseline(content: &str) -> BTreeSet<CiteRow> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(CiteRow::parse)
        .collect()
}

/// The ledger file's bytes: [`BASELINE_HEADER`], then one row per line in
/// [`CiteRow`] order. An empty set renders the header alone.
pub fn render_baseline(rows: &BTreeSet<CiteRow>) -> String {
    let mut out = String::from(BASELINE_HEADER);
    for row in rows {
        out.push_str(&format!("{row}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CACHE: &str = "crates/reify-eval/src/cache.rs";
    const STDLIB_FEA: &str = "crates/reify-compiler/stdlib/materials_fea.ri";

    fn row(path: &str, cite: &str) -> CiteRow {
        CiteRow {
            path: path.to_string(),
            cite: cite.to_string(),
        }
    }

    fn rows<const N: usize>(items: [CiteRow; N]) -> BTreeSet<CiteRow> {
        items.into_iter().collect()
    }

    #[test]
    fn a_row_is_a_path_and_a_cite() {
        assert_eq!(
            CiteRow::parse("crates/reify-eval/src/cache.rs :: engine_eval.rs:3884"),
            row(CACHE, "engine_eval.rs:3884"),
        );
    }

    #[test]
    fn a_row_splits_at_the_first_separator_so_an_esc_cite_keeps_its_space() {
        assert_eq!(
            CiteRow::parse(&format!("{STDLIB_FEA} :: units.ri esc-4026-121:80")),
            row(STDLIB_FEA, "units.ri esc-4026-121:80"),
        );
    }

    #[test]
    fn display_is_the_row_grammar_parse_accepts() {
        let plain = row(CACHE, "engine_eval.rs:3884");
        let esc = row(STDLIB_FEA, "units.ri esc-4026-121:80");
        assert_eq!(
            plain.to_string(),
            "crates/reify-eval/src/cache.rs :: engine_eval.rs:3884"
        );
        for r in [plain, esc] {
            assert_eq!(CiteRow::parse(&r.to_string()), r);
        }
    }

    #[test]
    fn parse_trims_surrounding_whitespace() {
        assert_eq!(
            CiteRow::parse("  crates/reify-eval/src/cache.rs :: engine_eval.rs:3884\t"),
            row(CACHE, "engine_eval.rs:3884"),
        );
    }

    #[test]
    fn parse_baseline_skips_blank_and_comment_lines_and_dedups() {
        let content = "# header\n\ncrates/reify-eval/src/cache.rs :: a.rs:1\n   \n  \
                       # indented comment\ncrates/reify-eval/src/cache.rs :: a.rs:1\n\
                       crates/reify-eval/src/cache.rs :: b.rs:2\n";
        assert_eq!(
            parse_baseline(content),
            rows([row(CACHE, "a.rs:1"), row(CACHE, "b.rs:2")]),
        );
    }

    /// No line is ever silently discarded: a row that matches no live cite
    /// is reported stale by the ratchet, so garbage must survive parsing to
    /// be reported at all.
    #[test]
    fn parse_baseline_turns_every_non_comment_line_into_a_row() {
        assert_eq!(
            parse_baseline("no separator here\n"),
            rows([row("no separator here", "")]),
        );
    }

    #[test]
    fn render_baseline_writes_the_header_then_rows_sorted_by_path_then_cite() {
        let ledger = rows([
            row(STDLIB_FEA, "units.ri:116"),
            row(CACHE, "lib.rs:9"),
            row(CACHE, "engine_eval.rs:3884"),
        ]);
        let expected_rows = "crates/reify-compiler/stdlib/materials_fea.ri :: units.ri:116\n\
                             crates/reify-eval/src/cache.rs :: engine_eval.rs:3884\n\
                             crates/reify-eval/src/cache.rs :: lib.rs:9\n";
        assert_eq!(
            render_baseline(&ledger),
            format!("{BASELINE_HEADER}{expected_rows}")
        );
    }

    #[test]
    fn render_baseline_round_trips_through_parse_baseline() {
        let ledger = rows([
            row(CACHE, "engine_eval.rs:3884"),
            row(STDLIB_FEA, "units.ri esc-4026-121:80"),
        ]);
        assert_eq!(parse_baseline(&render_baseline(&ledger)), ledger);
    }

    #[test]
    fn an_empty_ledger_renders_the_header_alone() {
        assert_eq!(render_baseline(&BTreeSet::new()), BASELINE_HEADER);
        assert!(parse_baseline(BASELINE_HEADER).is_empty());
    }
}

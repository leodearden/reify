//! The PDOCCOVER ratchet ledger — the grammar of the committed
//! `crates/reify-audit/pdoccover-baseline.txt`.
//!
//! The file lists PDOCCOVER's accepted debt, one [`BaselineRow`] per line, in
//! two kinds: a bare `<name>` is omission debt (a registry name no chunk
//! documents) and `<chunk path>:<name>` is fabrication debt (a call-shaped name
//! that chunk claims but no source declares). [`Ledger`] is the set algebra
//! the ratchet and its generator share: live debt the ledger already holds is
//! kept, live debt it lacks is new, and a committed row no live debt matches
//! is stale.
//!
//! This module knows nothing about how debt is found — `pdoccover.rs` derives
//! the live rows; this is only the format of one committed file.

use std::collections::BTreeSet;
use std::fmt;

/// The committed ledger, read only when git-tracked (an untracked copy is
/// inert, like every other PDOCCOVER input).
pub const BASELINE_PATH: &str = "crates/reify-audit/pdoccover-baseline.txt";

/// The preamble [`render_baseline`] writes above the rows. Every line is a
/// `#` comment, so [`parse_baseline`] reads none of it as a row — which is
/// also why stripping it would go unnoticed without a byte comparison against
/// this constant.
pub const BASELINE_HEADER: &str = "\
# PDOCCOVER baseline — accepted registry<->chunk name-drift debt.
#
# Two row kinds, one per line:
#   <name>               a registry name no chunk documents (undocumented-name)
#   <chunk path>:<name>  a call-shaped name that chunk claims but no compiler or
#                        stdlib source declares (fabricated-name)
#
# GENERATED — do not hand-edit. Regenerate with:
#   cargo run -p reify-audit --bin pdoccover-baseline-gen -- --project-root . \\
#     > crates/reify-audit/pdoccover-baseline.txt
#
# The default regeneration only DROPS stale rows. `--admit-new` is the only way
# to add debt, and every row it adds must be justified in review; the real fix
# is to document the name, correct the chunk, or mark the line
# `pdoccover:allow — <reason>`.
#
# A row that matches no live debt is STALE and hard-FAILs
# `reify-audit --pattern PDOCCOVER`: delete it, or rerun the default generator.
#
# A regeneration is valid for EXACTLY the tree it ran against. Any later rebase,
# merge, amend or cherry-pick is a different tree and invalidates it. Re-verify
# on the COMMITTED tree:
#   git status --porcelain    # must be empty
#   cargo run -p reify-audit --bin pdoccover-baseline-gen -- --project-root . \\
#     | diff -u crates/reify-audit/pdoccover-baseline.txt -
";

/// One line of the ledger.
///
/// The derived `Ord` puts every omission row before every fabrication row
/// (variant order), each group sorted by its fields — the order
/// [`render_baseline`] writes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaselineRow {
    /// A bare `<name>` row.
    Undocumented(String),
    /// A `<chunk path>:<name>` row.
    Fabricated { chunk: String, name: String },
}

impl BaselineRow {
    /// Parse one non-comment line. Never fails: text that names no live debt
    /// still becomes a row, and the ratchet reports it stale.
    ///
    /// Splits at the LAST `:` — identifier-shaped names never contain one, so
    /// every row a lane can produce round-trips through [`fmt::Display`].
    pub fn parse(line: &str) -> Self {
        match line.trim().rsplit_once(':') {
            Some((chunk, name)) => Self::Fabricated {
                chunk: chunk.to_string(),
                name: name.to_string(),
            },
            None => Self::Undocumented(line.trim().to_string()),
        }
    }
}

impl fmt::Display for BaselineRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undocumented(name) => f.write_str(name),
            Self::Fabricated { chunk, name } => write!(f, "{chunk}:{name}"),
        }
    }
}

/// Every row in a ledger file. Blank and `#` lines are skipped; every other
/// line becomes a row, so no line is ever silently discarded.
pub fn parse_baseline(content: &str) -> BTreeSet<BaselineRow> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(BaselineRow::parse)
        .collect()
}

/// The ledger file's bytes: [`BASELINE_HEADER`], then one row per line in
/// [`BaselineRow`] order. An empty set renders the header alone.
pub fn render_baseline(rows: &BTreeSet<BaselineRow>) -> String {
    let mut out = String::from(BASELINE_HEADER);
    for row in rows {
        out.push_str(&format!("{row}\n"));
    }
    out
}

/// Live debt (what the lanes find today) against committed debt (the ledger
/// file). The three views are disjoint: `kept ∪ new_debt == live` and
/// `kept ∪ stale == committed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    pub live: BTreeSet<BaselineRow>,
    pub committed: BTreeSet<BaselineRow>,
}

impl Ledger {
    /// Live debt the ledger already accounts for — what a shrink-only
    /// regeneration writes.
    pub fn kept(&self) -> BTreeSet<BaselineRow> {
        self.live.intersection(&self.committed).cloned().collect()
    }

    /// Live debt the ledger lacks — reported by the ratchet.
    pub fn new_debt(&self) -> BTreeSet<BaselineRow> {
        self.live.difference(&self.committed).cloned().collect()
    }

    /// Committed rows no live debt matches — removing one would change no
    /// other finding, so each is reported as dead weight.
    pub fn stale(&self) -> BTreeSet<BaselineRow> {
        self.committed.difference(&self.live).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOMETRY_CHUNK: &str = "crates/reify-mcp/src/tools/chunks/geometry.md";

    fn undocumented(name: &str) -> BaselineRow {
        BaselineRow::Undocumented(name.to_string())
    }

    fn fabricated(chunk: &str, name: &str) -> BaselineRow {
        BaselineRow::Fabricated {
            chunk: chunk.to_string(),
            name: name.to_string(),
        }
    }

    fn rows<const N: usize>(items: [BaselineRow; N]) -> BTreeSet<BaselineRow> {
        items.into_iter().collect()
    }

    #[test]
    fn a_bare_row_is_omission_debt() {
        assert_eq!(BaselineRow::parse("midplane"), undocumented("midplane"));
    }

    #[test]
    fn a_path_name_row_is_fabrication_debt_split_at_the_last_colon() {
        assert_eq!(
            BaselineRow::parse("crates/reify-mcp/src/tools/chunks/geometry.md:Value"),
            fabricated(GEOMETRY_CHUNK, "Value"),
        );
        assert_eq!(BaselineRow::parse("a:b:c"), fabricated("a:b", "c"));
    }

    #[test]
    fn parse_trims_surrounding_whitespace() {
        assert_eq!(BaselineRow::parse("  midplane\t"), undocumented("midplane"));
        assert_eq!(
            BaselineRow::parse(" crates/reify-mcp/src/tools/chunks/geometry.md:new "),
            fabricated(GEOMETRY_CHUNK, "new"),
        );
    }

    #[test]
    fn display_is_the_row_grammar_parse_accepts() {
        let bare = undocumented("midplane");
        let path_name = fabricated(GEOMETRY_CHUNK, "InnerSolution");
        assert_eq!(bare.to_string(), "midplane");
        assert_eq!(
            path_name.to_string(),
            "crates/reify-mcp/src/tools/chunks/geometry.md:InnerSolution",
        );
        for row in [bare, path_name] {
            assert_eq!(BaselineRow::parse(&row.to_string()), row);
        }
    }

    #[test]
    fn parse_baseline_skips_blank_and_comment_lines_and_dedups() {
        let content = "# header\n\nmidplane\n   \n  # indented comment\nmidplane\n\
                       crates/reify-mcp/src/tools/chunks/geometry.md:Value\n";
        assert_eq!(
            parse_baseline(content),
            rows([
                undocumented("midplane"),
                fabricated(GEOMETRY_CHUNK, "Value")
            ]),
        );
    }

    /// No line is ever silently discarded: a row that matches no live debt is
    /// reported stale by the ratchet, so garbage must survive parsing to be
    /// reported at all.
    #[test]
    fn parse_baseline_turns_every_non_comment_line_into_a_row() {
        assert_eq!(
            parse_baseline("foo bar\na:b:c\n"),
            rows([undocumented("foo bar"), fabricated("a:b", "c")]),
        );
    }

    #[test]
    fn render_baseline_lists_omission_rows_before_path_name_rows_each_sorted() {
        let ledger = rows([
            fabricated(GEOMETRY_CHUNK, "new"),
            undocumented("midplane"),
            fabricated(
                "crates/reify-mcp/src/tools/chunks/constraints.md",
                "predicate",
            ),
            fabricated(GEOMETRY_CHUNK, "Value"),
            undocumented("angle_between"),
        ]);
        let expected_rows = "angle_between\n\
                             midplane\n\
                             crates/reify-mcp/src/tools/chunks/constraints.md:predicate\n\
                             crates/reify-mcp/src/tools/chunks/geometry.md:Value\n\
                             crates/reify-mcp/src/tools/chunks/geometry.md:new\n";
        assert_eq!(
            render_baseline(&ledger),
            format!("{BASELINE_HEADER}{expected_rows}")
        );
    }

    #[test]
    fn render_baseline_round_trips_through_parse_baseline() {
        let ledger = rows([
            undocumented("midplane"),
            undocumented("angle_between"),
            fabricated(GEOMETRY_CHUNK, "Value"),
            fabricated(GEOMETRY_CHUNK, "new"),
        ]);
        assert_eq!(parse_baseline(&render_baseline(&ledger)), ledger);
    }

    #[test]
    fn an_empty_ledger_renders_the_header_alone() {
        assert_eq!(render_baseline(&BTreeSet::new()), BASELINE_HEADER);
        assert!(parse_baseline(BASELINE_HEADER).is_empty());
    }

    #[test]
    fn the_ledger_partitions_live_and_committed_debt() {
        let kept = undocumented("kept_op");
        let new = fabricated(GEOMETRY_CHUNK, "ghost_op");
        let stale = undocumented("vanished_op");
        let ledger = Ledger {
            live: rows([kept.clone(), new.clone()]),
            committed: rows([kept.clone(), stale.clone()]),
        };

        assert_eq!(ledger.kept(), rows([kept]));
        assert_eq!(ledger.new_debt(), rows([new]));
        assert_eq!(ledger.stale(), rows([stale]));

        let (kept, new_debt, stale) = (ledger.kept(), ledger.new_debt(), ledger.stale());
        assert!(kept.is_disjoint(&new_debt) && kept.is_disjoint(&stale));
        assert!(new_debt.is_disjoint(&stale));
        assert_eq!(&kept | &new_debt, ledger.live);
        assert_eq!(&kept | &stale, ledger.committed);
    }
}

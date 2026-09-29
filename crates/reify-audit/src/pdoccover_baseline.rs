//! The PDOCCOVER ratchet ledger — the grammar of the committed
//! `crates/reify-audit/pdoccover-baseline.txt`.

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
            rows([undocumented("midplane"), fabricated(GEOMETRY_CHUNK, "Value")]),
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
            fabricated("crates/reify-mcp/src/tools/chunks/constraints.md", "predicate"),
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

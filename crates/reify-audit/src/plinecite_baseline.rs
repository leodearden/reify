#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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

    #[test]
    fn the_ledger_lives_beside_the_other_reify_audit_baselines() {
        assert_eq!(BASELINE_PATH, "crates/reify-audit/plinecite-baseline.txt");
    }
}

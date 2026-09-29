//! PCITE — capability-manifest cite detector.

use std::collections::HashSet;

/// The symbols one capability-manifest line cites as grep evidence.
pub fn cited_symbols(_line: &str) -> Vec<&str> {
    Vec::new()
}

/// Every identifier-shaped word in `sources`.
pub fn symbol_index(_sources: &[(String, String)]) -> HashSet<&str> {
    HashSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_without_grep_evidence_cites_nothing() {
        assert_eq!(
            cited_symbols("| cap | `check_expr_struct_ctor_args` in `lib.rs` | PASS |"),
            Vec::<&str>::new(),
            "backticked identifiers are cites only on a line carrying `grep:`"
        );
    }

    #[test]
    fn a_grep_cell_cites_its_identifier_span_and_not_its_path_span() {
        let line = "| cap | grep: `check_expr_struct_ctor_args` at \
                    `crates/x/src/y.rs:1344` | PASS |";
        assert_eq!(cited_symbols(line), vec!["check_expr_struct_ctor_args"]);
    }

    #[test]
    fn backticks_pair_from_line_start_not_from_the_grep_offset() {
        assert_eq!(
            cited_symbols("| `grep:crates/a.rs:12 wired` (`real_fn`, confirmed) |"),
            vec!["real_fn"],
            "slicing at `grep:` first would pair the span's CLOSING backtick \
             with `real_fn`'s opening one"
        );
        assert_eq!(
            cited_symbols("| `foo` | grep: `bar` |"),
            vec!["bar"],
            "a span opening before the first `grep:` is never cited"
        );
    }

    #[test]
    fn path_spans_yield_each_segment_and_call_parens_are_stripped() {
        assert_eq!(
            cited_symbols("grep: `PendingBoundCheck::TraitArgConformance`"),
            vec!["PendingBoundCheck", "TraitArgConformance"]
        );
        assert_eq!(cited_symbols("grep: `live_counts()`"), vec!["live_counts"]);
    }

    #[test]
    fn a_span_with_any_non_identifier_segment_yields_nothing() {
        for span in ["impl DiagnosticCode", "::COUNT", "a b", "Foo::", "x.y", ""] {
            let line = format!("grep: `{span}`");
            assert_eq!(
                cited_symbols(&line),
                Vec::<&str>::new(),
                "`{span}` is not a symbol path"
            );
        }
    }

    #[test]
    fn commit_sha_shaped_segments_are_dropped() {
        assert_eq!(cited_symbols("grep: `cdc501a3f1`"), Vec::<&str>::new());
        assert_eq!(
            cited_symbols("grep: `abcdef1` `deadbeefcafe0123456789abcdef0123456789ab` `real_fn`"),
            vec!["real_fn"],
            "7- and 40-hex-digit spans are both SHA-shaped"
        );
    }

    #[test]
    fn hex_words_that_are_not_sha_shaped_are_kept() {
        let too_long = "deadbeefcafe0123456789abcdef0123456789abc";
        assert_eq!(too_long.len(), 41);
        let line = format!("grep: `abcdef` `deadbeef` `abc123` `cdc501A3f1` `{too_long}`");
        assert_eq!(
            cited_symbols(&line),
            vec!["abcdef", "deadbeef", "abc123", "cdc501A3f1", too_long],
            "no digit, under 7 or over 40 hex digits, or an uppercase letter: \
             none is SHA-shaped"
        );
    }

    #[test]
    fn the_grammar_is_pure_an_allow_marker_does_not_suppress_here() {
        assert_eq!(
            cited_symbols("| grep: `ghost_symbol` | <!-- pcite:allow — dark-factory symbol -->"),
            vec!["ghost_symbol"],
            "suppression is check()'s job, so the marker report stays \
             independent of this grammar"
        );
    }

    #[test]
    fn symbol_index_holds_every_maximal_identifier_run() {
        let sources = vec![
            ("a.rs".to_string(), "fn x1() { let v = 1x; héllo_w }".to_string()),
            ("b.ri".to_string(), "structure Bracket_2".to_string()),
        ];
        let index = symbol_index(&sources);
        for present in ["fn", "x1", "let", "v", "h", "llo_w", "structure", "Bracket_2"] {
            assert!(index.contains(present), "`{present}` must be indexed; got {index:?}");
        }
        for absent in ["1x", "x", "héllo_w", "llo", "Bracket"] {
            assert!(!index.contains(absent), "`{absent}` must not be indexed; got {index:?}");
        }
    }
}

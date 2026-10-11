#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Vec<&str> = Vec::new();

    #[test]
    fn a_cite_token_stops_at_its_first_digit_run() {
        assert_eq!(line_cites("// see cache.rs:617-621"), vec!["cache.rs:617"]);
    }

    #[test]
    fn a_ri_target_is_a_cite_too() {
        assert_eq!(
            line_cites("// (units.ri:116) composes"),
            vec!["units.ri:116"]
        );
    }

    #[test]
    fn a_path_qualified_cite_is_delimited_by_backticks() {
        assert_eq!(
            line_cites("/// `crates/reify-eval/src/unfold.rs:292-358` populates"),
            vec!["crates/reify-eval/src/unfold.rs:292"]
        );
    }

    #[test]
    fn several_cites_on_one_line_come_back_in_line_order() {
        assert_eq!(
            line_cites("// orientation.rs:264, orientation.rs:440"),
            vec!["orientation.rs:264", "orientation.rs:440"]
        );
    }

    #[test]
    fn doc_comment_lines_and_indented_comment_lines_are_comment_lines() {
        assert_eq!(line_cites("    /// see a.rs:1"), vec!["a.rs:1"]);
        assert_eq!(line_cites("//! see b.ri:2"), vec!["b.ri:2"]);
        assert_eq!(line_cites("\t// see c.rs:3"), vec!["c.rs:3"]);
    }

    #[test]
    fn a_line_that_is_not_a_comment_line_cites_nothing() {
        for line in [
            "let at = lookup(\"cache.rs:561\");",
            "assert!(ok, \"sanity (cache.rs:561)\");",
            "let x = 1; // cache.rs:561",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} is not a comment line");
        }
    }

    #[test]
    fn the_esc_anchored_form_reports_through_its_line_number() {
        assert_eq!(
            line_cites("// (units.ri esc-4026-121:80-91 precedent)"),
            vec!["units.ri esc-4026-121:80"]
        );
    }

    #[test]
    fn legal_spellings_without_a_line_number_cite_nothing() {
        for line in [
            "// units.ri esc-4026-121",
            "// units.ri",
            "// foo.rs::bar",
            "// foo.rs: see",
            "// foo.rsx:1",
            "// foo.rib:2",
            "// .rs:1",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} carries no hard cite");
        }
    }

    #[test]
    fn documented_out_of_grammar_forms_cite_nothing() {
        for line in [
            "// lines 12-34 of foo.rs",
            "// documented in (structural_physical.ri:",
            "// see widget.js:12",
            "// see occt.h:30",
        ] {
            assert_eq!(line_cites(line), NONE, "{line:?} is out of grammar");
        }
    }

    #[test]
    fn non_ascii_text_neither_panics_nor_hides_a_cite() {
        assert_eq!(line_cites("// µm °C — foo.rs:12"), vec!["foo.rs:12"]);
        assert_eq!(line_cites("// é.rs:1 ü"), NONE);
    }

    #[test]
    fn the_grammar_is_pure_an_allow_marker_does_not_suppress_here() {
        assert_eq!(
            line_cites("// foo.rs:12 plinecite:allow — dated"),
            vec!["foo.rs:12"],
            "suppression is check()'s job, so the marker report stays \
             independent of this grammar"
        );
    }
}

pub fn has_liveness_marker(_source: &str, _cite: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sole_cite_marker_line_is_a_liveness_marker() {
        let source = "fn f() {}\n    // TODO(#5404): delete this entry\nconst A: u8 = 0;\n"; // ptodo:allow — fixture, not a marker
        assert!(has_liveness_marker(source, "#5404"));
    }

    #[test]
    fn a_cite_held_only_in_a_string_literal_has_no_marker() {
        let source = "    cite: \"#5404\",\n";
        assert!(!has_liveness_marker(source, "#5404"));
    }

    #[test]
    fn a_marker_sharing_its_line_with_another_cite_does_not_count() {
        let co_cited = "// TODO(#5404): retire once #6048 widens"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(co_cited, "#5404"));

        let with_sole_cite_line = format!("{co_cited}\n// TODO(#5404): demote it"); // ptodo:allow — fixture, not a marker
        assert!(has_liveness_marker(&with_sole_cite_line, "#5404"));
    }

    #[test]
    fn an_escaped_marker_does_not_count() {
        let source = "// TODO(#5404): x // ptodo:allow — reason"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(source, "#5404"));
    }

    #[test]
    fn the_closing_paren_bounds_the_id() {
        let longer_id = "// TODO(#54040): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(longer_id, "#5404"));

        let shorter_cite = "// TODO(#5404): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(shorter_cite, "#540"));
    }

    #[test]
    fn a_hash_not_followed_by_a_digit_is_not_a_second_cite() {
        let source = "// TODO(#5404): drop the #[allow] and the C# shim"; // ptodo:allow — fixture, not a marker
        assert!(has_liveness_marker(source, "#5404"));
    }

    #[test]
    fn a_non_canonical_cite_never_has_a_marker() {
        let source = "// TODO(task 5404): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(source, "task 5404"));
    }

    #[test]
    fn a_marker_for_a_different_task_does_not_count() {
        let source = "// TODO(#6693): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(source, "#5404"));
    }
}

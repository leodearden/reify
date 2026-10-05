//! Whether a task cite is liveness-checked by the PTODO detector.
//!
//! PTODO's liveness lane resolves cites only where a marker anchors them
//! (comments, `#[ignore]` reasons, stub macros), so a task cite held in a Rust
//! string literal, such as a struct field or a table row, is invisible to it.
//! Such a cite is liveness-checked only through a marker comment in the same
//! file citing the same task: when that task goes terminal, the marker is
//! reported `orphaned` and the PTODO ratchet turns red.
//!
//! [`has_liveness_marker`] accepts a TODO-family marker comment whose
//! parenthesised cite is exactly `cite`, but only when that cite is the line's
//! sole `#`-then-digit occurrence and the line carries no `ptodo:allow` escape.
//! Both rules come from `docs/prds/reify-audit-ptodo-detector.md`. Under §8.2
//! one live cite tracks the whole line, so a co-cited live task would mask a
//! closed one. Under §6.8 an escaped line is skipped entirely.
//!
//! The predicate is a conservative SUFFICIENT condition for PTODO to orphan the
//! marker once the task closes. Every canonical cite begins with `#` and a
//! digit, so it can demand more than PTODO needs but never less. A false
//! refusal (a PRD-relative `§`-index on the marker line, say) is fixed by moving
//! the other `#`-then-digit text off the marker line.
//!
//! A table whose rows carry task cites pins each distinct cite through this
//! predicate over its own source.

/// `true` iff some line of `source` is a marker comment that keeps `cite`
/// liveness-checked by PTODO, per the module-level rules.
pub fn has_liveness_marker(source: &str, cite: &str) -> bool {
    let needle = format!("// TODO({cite})"); // ptodo:allow — the needle, not a marker
    source
        .lines()
        .any(|line| is_sole_cite_marker(line, &needle))
}

fn is_sole_cite_marker(line: &str, needle: &str) -> bool {
    line.contains(needle)
        && !line.contains("ptodo:allow")
        && hash_digit_occurrences(needle) == 1
        && hash_digit_occurrences(line) == 1
}

fn hash_digit_occurrences(text: &str) -> usize {
    text.as_bytes()
        .windows(2)
        .filter(|pair| pair[0] == b'#' && pair[1].is_ascii_digit())
        .count()
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

        let other_cite_on_the_line = "// TODO(task 5404): see #6048"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(other_cite_on_the_line, "task 5404"));
    }

    #[test]
    fn a_marker_for_a_different_task_does_not_count() {
        let source = "// TODO(#6693): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(source, "#5404"));
    }
}

//! Whether a task cite is liveness-checked by the PTODO detector.
//!
//! PTODO's liveness lane resolves cites only where a marker anchors them
//! (comments, `#[ignore]` reasons, stub macros), so a task cite held in a Rust
//! string literal, such as a struct field or a table row, is invisible to it.
//! Such a cite is liveness-checked only through a marker comment in the same
//! file citing the same task: when that task goes terminal, the marker is
//! reported `orphaned` and the PTODO ratchet turns red.
//!
//! [`has_liveness_marker`] accepts one form only. The cite is canonical: `#`
//! then 1 to 5 digits, value at least 1, as reify-audit's `cite_occurrences`
//! reads it. Some comment-only line (trimmed, it starts with `//`) contains
//! `// TODO` immediately followed by the parenthesised cite. That cite is the
//! line's sole `#`-then-digit occurrence, and the line carries no `ptodo:allow`
//! escape. The last two rules come from `docs/prds/reify-audit-ptodo-detector.md`.
//! Under §8.2 one live cite tracks the whole line, so a co-cited live task
//! would mask a closed one. Under §6.8 an escaped line is skipped entirely.
//!
//! PTODO tracks every accepted line by that cite alone, so the marker is
//! orphaned once the task closes. The converse does not hold. PTODO also tracks
//! `FIXME`/`HACK` markers, markers trailing code, and lines whose other
//! `#`-then-digit text is not a cite (a PRD-relative `§`-index, say), and this
//! predicate refuses all of them. The comment-only rule is what keeps out an
//! `#[ignore = "..."]` line, which PTODO judges by its reason string alone. To
//! fix a refusal, write the marker in the accepted form on a line of its own.
//!
//! A table whose rows carry task cites pins each distinct cite through this
//! predicate over its own source.

/// `true` iff some line of `source` is a marker comment that keeps `cite`
/// liveness-checked by PTODO, per the module-level rules.
pub fn has_liveness_marker(source: &str, cite: &str) -> bool {
    if !is_canonical_cite(cite) {
        return false;
    }
    let needle = format!("// TODO({cite})"); // ptodo:allow — the needle, not a marker
    source
        .lines()
        .any(|line| is_sole_cite_marker(line, &needle))
}

fn is_canonical_cite(cite: &str) -> bool {
    let Some(digits) = cite.strip_prefix('#') else {
        return false;
    };
    (1..=5).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.parse::<u32>().is_ok_and(|id| id >= 1)
}

fn is_sole_cite_marker(line: &str, needle: &str) -> bool {
    line.trim_start().starts_with("//")
        && line.contains(needle)
        && !line.contains("ptodo:allow")
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

        let zero_id = "// TODO(#0): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(zero_id, "#0"));

        let six_digits = "// TODO(#123456): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(six_digits, "#123456"));
    }

    #[test]
    fn a_marker_must_be_a_comment_only_line() {
        let after_code = "const A: u8 = 0; // TODO(#5404): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(after_code, "#5404"));

        let after_ignore = "#[ignore = \"slow\"] // TODO(#5404): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(after_ignore, "#5404"));

        let doc_comment = "    /// TODO(#5404): x"; // ptodo:allow — fixture, not a marker
        assert!(has_liveness_marker(doc_comment, "#5404"));
    }

    #[test]
    fn only_the_todo_keyword_is_accepted() {
        for keyword in ["FIXME", "HACK"] {
            let source = format!("// {keyword}(#5404): x");
            assert!(!has_liveness_marker(&source, "#5404"), "{source}");
        }
    }

    #[test]
    fn a_marker_for_a_different_task_does_not_count() {
        let source = "// TODO(#6693): x"; // ptodo:allow — fixture, not a marker
        assert!(!has_liveness_marker(source, "#5404"));
    }
}

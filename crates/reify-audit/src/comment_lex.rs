//! Classify a source line's bytes as code or comment, carrying nested
//! `/* */` depth across lines; Rust and Reify `.ri` share the syntax. PDIAG
//! consumes only the depth, because its comment mask is line-granular;
//! PDOCCOVER consumes the code spans, to strip comments out of a line.

use std::ops::Range;

/// Block-comment nesting depth at a line boundary. [`Default`] is closed:
/// outside every comment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BlockDepth(usize);

/// Report each code byte-range of `line` to `on_code`, lexing from `entry`,
/// and return the depth the line exits at.
pub(crate) fn lex_line(
    line: &str,
    entry: BlockDepth,
    mut on_code: impl FnMut(Range<usize>),
) -> BlockDepth {
    on_code(0..line.len());
    entry
}

/// Byte length of the char literal starting at `bytes[at]`, or `None` when
/// none starts there.
pub(crate) fn char_literal_len(bytes: &[u8], at: usize) -> Option<usize> {
    let _ = (bytes, at);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code spans `lex_line` reports for `line`, as text, and its exit
    /// depth.
    fn code(line: &str, entry: BlockDepth) -> (Vec<&str>, BlockDepth) {
        let mut spans = Vec::new();
        let exit = lex_line(line, entry, |r| spans.push(&line[r]));
        (spans, exit)
    }

    fn closed() -> BlockDepth {
        BlockDepth::default()
    }

    /// The depth `line` exits at from a closed entry: how these tests build an
    /// open state without reaching the counter.
    fn after(line: &str) -> BlockDepth {
        code(line, closed()).1
    }

    #[test]
    fn an_all_code_line_is_one_span_and_stays_closed() {
        let line = "let x = 1;";
        assert_eq!(code(line, closed()), (vec![line], closed()));
    }

    #[test]
    fn a_line_comment_tail_is_not_code() {
        assert_eq!(
            code("let x = 1; // note", closed()),
            (vec!["let x = 1; "], closed())
        );
    }

    #[test]
    fn an_inline_block_comment_splits_the_code() {
        assert_eq!(code("a /* c */ b", closed()), (vec!["a ", " b"], closed()));
    }

    #[test]
    fn a_nested_block_closes_only_at_the_outer_close() {
        assert_eq!(
            code("/* a /* b */ c */ d", closed()),
            (vec![" d"], closed())
        );
    }

    #[test]
    fn an_open_block_carries_into_the_next_line() {
        let (spans, open) = code("/* a /* b */ c", closed());
        assert!(spans.is_empty(), "the whole line is comment, got {spans:?}");
        assert_ne!(open, closed());
        assert_eq!(code("x */ y", open), (vec![" y"], closed()));
    }

    #[test]
    fn the_carried_depth_counts_every_unclosed_opener() {
        let two = after("/* a /* b");
        let (spans, one) = code("*/ z", two);
        assert!(
            spans.is_empty(),
            "one close of two leaves it open, got {spans:?}"
        );
        assert_ne!(one, closed());
        assert_eq!(code("*/ z", one), (vec![" z"], closed()));
        assert_eq!(code("*/ m */ z", two), (vec![" z"], closed()));
    }

    #[test]
    fn comment_delimiters_inside_a_string_are_code() {
        for line in [
            "let s = \"/*\"; let t = 1;",
            "let u = \"a//b\"; x",
            "let s = \"*/\"; y",
        ] {
            assert_eq!(code(line, closed()), (vec![line], closed()), "{line:?}");
        }
        assert_eq!(
            code("let u = \"a//b\"; // c", closed()),
            (vec!["let u = \"a//b\"; "], closed())
        );
    }

    #[test]
    fn an_escaped_quote_keeps_the_string_open() {
        let line = "let s = \"a\\\"/*\"; y";
        assert_eq!(code(line, closed()), (vec![line], closed()));
        assert_eq!(
            code("let s = \"a\\\"/*\"; /* c */ y", closed()),
            (vec!["let s = \"a\\\"/*\"; ", " y"], closed())
        );
    }

    #[test]
    fn a_char_literal_quote_does_not_open_a_string() {
        assert_eq!(
            code("let q = '\"'; /* c */ z", closed()),
            (vec!["let q = '\"'; ", " z"], closed())
        );
        assert_eq!(
            code("let q = b'\"'; /* c */ z", closed()),
            (vec!["let q = b'\"'; ", " z"], closed())
        );
        assert_eq!(
            code("let q = '\\\"'; // c", closed()),
            (vec!["let q = '\\\"'; "], closed())
        );
    }

    #[test]
    fn a_lifetime_is_not_a_char_literal() {
        assert_eq!(
            code("fn f<'a>(x: &'a str) /* c */ {}", closed()),
            (vec!["fn f<'a>(x: &'a str) ", " {}"], closed())
        );
    }

    #[test]
    fn quotes_are_inert_inside_a_comment() {
        assert_eq!(code("/* \" */ x", closed()), (vec![" x"], closed()));
        assert_eq!(code("\" */ x", after("/* open")), (vec![" x"], closed()));
    }

    #[test]
    fn a_stray_close_outside_a_comment_is_code() {
        let line = "a */ b";
        assert_eq!(code(line, closed()), (vec![line], closed()));
        assert_eq!(
            code("a */ b /* c */ d", closed()),
            (vec!["a */ b ", " d"], closed())
        );
    }

    /// Only depth carries: an unterminated string desyncs the rest of its own
    /// line at most, never the lines after it.
    #[test]
    fn string_state_does_not_carry_across_lines() {
        let line = "let s = \"abc";
        let (spans, exit) = code(line, closed());
        assert_eq!((spans, exit), (vec![line], closed()));
        assert_eq!(code("/* c */ d", exit), (vec![" d"], closed()));
    }

    #[test]
    fn spans_split_only_on_char_boundaries() {
        assert_eq!(
            code("\"§\" /* — */ é", closed()),
            (vec!["\"§\" ", " é"], closed())
        );
    }

    #[test]
    fn char_literal_len_measures_only_the_char_literal_shapes() {
        assert_eq!(char_literal_len(b"'x'", 0), Some(3));
        assert_eq!(char_literal_len(b"'\\n'", 0), Some(4));
        assert_eq!(char_literal_len(b"'\"'", 0), Some(3));
        assert_eq!(char_literal_len(b"'\\\"'", 0), Some(4));
        assert_eq!(char_literal_len(b"q = 'x';", 4), Some(3));
        assert_eq!(char_literal_len(b"'a str", 0), None, "a lifetime");
        assert_eq!(char_literal_len(b"x'y'", 0), None, "no quote at `at`");
    }
}

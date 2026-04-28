//! Internal helpers shared across the HTML and Markdown formatters.
//!
//! Nothing in this module is part of the public API of `reify-doc`; all items
//! are `pub(crate)` only.

/// Strip surrounding `"`s from a rendered string-literal annotation argument.
///
/// Annotations source-render `@deprecated("msg")` as the arg `"\"msg\""` —
/// the literal quote characters are *part of* the rendered representation.
/// Formatter output should display the message text without those wrapping
/// quotes; non-string-literal args (calls, identifiers, numbers) are returned
/// unchanged.
///
/// # Escape-aware stripping
///
/// Strips the outer `"…"` only when the inner content contains **no
/// unescaped** `"` characters.  A `\` preceding a `"` marks that `"` as
/// escaped; any other character (including a second `\`) clears the escape
/// flag.  If the inner content contains an unescaped `"` the input is
/// returned unchanged — this guards against callers passing a non-string-
/// literal arg such as `"first" + "second"` whose outer `"` chars are *not*
/// matching delimiters of a single string literal.
pub(crate) fn unquote(s: &str) -> &str {
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return s;
    }
    let inner = &s[1..s.len() - 1];
    let mut escaped = false;
    for b in inner.bytes() {
        match b {
            b'\\' => escaped = !escaped,
            b'"' if !escaped => return s, // unescaped inner quote — not a single literal
            _ => escaped = false,
        }
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract tests for `unquote`.
    ///
    /// The upstream source-printer represents string-literal annotation args
    /// with their wrapping `"` characters as part of the rendered value — e.g.
    /// `@deprecated("msg")` yields the arg string `"\"msg\""`.  `unquote`
    /// should strip the outer quotes for well-formed single string-literals
    /// but leave non-string args (calls, identifiers, concatenations whose
    /// outer `"` is not a matching pair bounding a single literal) unchanged.
    #[test]
    fn unquote_strips_only_well_formed_string_literals() {
        // Basic single string-literal: strip outer quotes.
        assert_eq!(
            unquote("\"foo\""),
            "foo",
            "basic single string-literal should have outer quotes stripped"
        );

        // Empty quoted string: strip to empty string.
        assert_eq!(
            unquote("\"\""),
            "",
            "empty quoted string should strip to empty string"
        );

        // Inner escaped quote: `"foo\"bar"` — the inner `\"` is escaped, so
        // there is only one string-literal; strip outer quotes.
        assert_eq!(
            unquote("\"foo\\\"bar\""),
            "foo\\\"bar",
            "inner escaped quote should not block stripping"
        );

        // Pathological multi-quote with escapes: `"\"first\" + \"second\""` —
        // all inner `"` chars are preceded by `\`, so this is still a
        // well-formed single string-literal; outer quotes should be stripped.
        assert_eq!(
            unquote("\"\\\"first\\\" + \\\"second\\\"\""),
            "\\\"first\\\" + \\\"second\\\"",
            "all-escaped inner quotes should still yield a strip"
        );

        // Unescaped inner quote: `"first" + "second"` — inner `"` after
        // `first` is unescaped.  This is NOT a single string-literal;
        // return unchanged.
        assert_eq!(
            unquote("\"first\" + \"second\""),
            "\"first\" + \"second\"",
            "concatenation with unescaped inner quote must be returned unchanged"
        );

        // No wrapping quotes: return unchanged.
        assert_eq!(
            unquote("not a string"),
            "not a string",
            "input without wrapping quotes must be returned unchanged"
        );

        // Single character (len < 2 after removing both ends would be 0 or
        // negative): return unchanged.
        assert_eq!(
            unquote("\""),
            "\"",
            "single-char input that is just a quote must be returned unchanged"
        );

        // Asymmetric: opening quote but no closing quote.
        assert_eq!(
            unquote("\"a"),
            "\"a",
            "asymmetric input (open quote, no close) must be returned unchanged"
        );

        // Edge case: input ending with an escaped trailing quote — `"foo\"`
        // (raw 6-char string: `"`, `f`, `o`, `o`, `\`, `"`).  The
        // `ends_with('"')` check is escape-blind and matches the trailing `"`;
        // the inner walk (`foo\`) contains no `"` byte, so the implementation
        // strips outer quotes and returns `foo\`.  Pinned here to prevent
        // silent behavioural drift if `unquote` is later rewritten.
        assert_eq!(
            unquote("\"foo\\\""),
            "foo\\",
            "escape-blind trailing-quote: outer quotes stripped, yielding foo\\"
        );

        // Extra trailing-side concatenation case: `"a""` (raw 4-char string:
        // `"`, `a`, `"`, `"`).  The inner walk finds an unescaped `"` at
        // position 1 and returns the input unchanged — exercises detection of
        // unescaped inner quotes near the trailing delimiter.
        assert_eq!(
            unquote("\"a\"\""),
            "\"a\"\"",
            "trailing-side unescaped inner quote must be returned unchanged"
        );
    }
}

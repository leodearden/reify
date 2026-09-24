//! The `check_and_lower!` diagnostic's source excerpt is length-bounded.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
//! a diagnostic that reprints a block of source is unreadable, and on a recovered parse the
//! offending node can span an entire declaration. These tests pin that the `invalid <label>: `
//! diagnostics emitted by `check_and_lower!` carry a BOUNDED, single-line excerpt, while
//! leaving the prefix — and any excerpt already inside the bound — untouched.
//!
//! SCOPE is `check_and_lower!` alone. The sibling `syntax error in <context>: ` arms in
//! `ts_parser.rs` no longer interpolate raw `node_text` (task #6156), but `lower_connect_body`'s
//! parameter and port-mapping arms still do (task #7756), so nothing here says the
//! diagnostic-excerpt class is closed across the parser.
//!
//! Asserted through the public `reify_syntax::parse` API on the messages it actually emits,
//! not against the private `snippet` helper: the contract is the user-visible diagnostic.
//! That is also why the newline case is pinned here as well as by `snippet`'s own unit tests
//! — only the rendered message shows that no raw newline survives into a diagnostic, and both
//! renderings are line-structured: `report_parse_errors` (crates/reify-cli/src/main.rs) writes
//! one `Parse error: {error}` line per error, and `mcp_context` joins messages with "; ".

use reify_ast::ParseError;

use crate::parse_error_lookup::{only_error_starting_with, parse_errors};

/// The bound a `check_and_lower!` excerpt must honour: at most this many characters, plus the
/// ellipsis that marks a cut.
const MAX_EXCERPT_CHARS: usize = 40;

/// The excerpt body of the one `invalid <label>: ` diagnostic, or a failure naming what was
/// emitted.
#[track_caller]
fn diagnostic_body<'a>(errors: &'a [ParseError], prefix: &str) -> &'a str {
    &only_error_starting_with(errors, prefix).message[prefix.len()..]
}

/// Assert a cut excerpt: the BOUND first, then the exact text.
///
/// The order is load-bearing. A grammar change to the node's text then reds the exact-match
/// only, with the bound already shown to hold — so the failure says "the cut point moved",
/// not "the diagnostic may be unbounded again".
#[track_caller]
fn assert_truncated(body: &str, expected: &str, errors: &[ParseError]) {
    assert!(
        body.ends_with('…'),
        "expected a cut excerpt to end in an ellipsis, got: {errors:?}"
    );
    assert!(
        body.chars().count() <= MAX_EXCERPT_CHARS + 1,
        "expected at most {} excerpt chars plus an ellipsis, got {}: {errors:?}",
        MAX_EXCERPT_CHARS,
        body.chars().count()
    );
    assert!(
        !body.contains('\n'),
        "expected a single-line excerpt, got: {errors:?}"
    );
    assert_eq!(body, expected, "unexpected cut point, got: {errors:?}");
}

/// Assert an excerpt inside the bound is reproduced verbatim: the properties first, then the
/// exact text, for the same reason as [`assert_truncated`].
#[track_caller]
fn assert_verbatim(body: &str, expected: &str, errors: &[ParseError]) {
    assert!(
        !body.contains('…'),
        "expected no ellipsis on an excerpt inside the bound, got: {errors:?}"
    );
    assert!(
        !body.contains('\n'),
        "expected a single-line excerpt, got: {errors:?}"
    );
    assert_eq!(body, expected, "unexpected excerpt, got: {errors:?}");
}

// ── the bound applies: long excerpts are cut ──────────────────────

#[test]
fn long_malformed_connect_is_truncated_with_an_ellipsis() {
    // `{ <member> -> }` has a "from" but no "to", so tree-sitter error recovery sets
    // has_error() on the connect_statement while keeping its kind — the proven vehicle for
    // reaching `check_and_lower!` with an `invalid connect` label.
    let errors = parse_errors(
        "structure S { port a : out T  port b : in T  connect a -> b { shaft_alignment_reference_member -> } }",
    );
    assert_truncated(
        diagnostic_body(&errors, "invalid connect: "),
        "connect a -> b { shaft_alignment_referen…",
        &errors,
    );
}

#[test]
fn multiline_connect_excerpt_is_cut_at_the_first_newline() {
    // The motivating harm, and the one user-visible FORMAT regression the bound fixes: on a
    // recovered parse the offending node spans the whole block, so this message previously
    // carried EMBEDDED RAW NEWLINES — measured before the fix as
    // `invalid connect: connect a -> b {\n    shaft ->\n  }` — which breaks the
    // one-record-per-diagnostic structure of both renderings named in the module doc.
    let errors = parse_errors(
        "structure S {\n  port a : out T\n  port b : in T\n  connect a -> b {\n    shaft ->\n  }\n}\n",
    );
    assert_truncated(
        diagnostic_body(&errors, "invalid connect: "),
        "connect a -> b {…",
        &errors,
    );
}

#[test]
fn truncation_of_a_multibyte_snippet_lands_on_a_character_boundary() {
    // A string literal where a port member name belongs: malformed by GRAMMAR, like the
    // fixtures above, so repairing any open parse gap cannot silently retire this test.
    //
    // WHY THESE EXACT BYTES: the node text `connect a -> b { "abcdefghijklmnopqrstu·vwxyz"`
    // is 46 chars but 47 bytes, and byte offset 40 is NOT a char boundary — the U+00B7 MIDDLE
    // DOT occupies bytes 39-40. A regression from `char_indices` to a byte slice
    // `&first_line[..40]` therefore PANICS here rather than merely mis-asserting. The 40th
    // character is the `·` itself, so a correct cut keeps it intact and puts the ellipsis
    // after it.
    let errors = parse_errors(
        "structure S { port a : out T  port b : in T  connect a -> b { \"abcdefghijklmnopqrstu·vwxyz\" -> } }",
    );
    assert_truncated(
        diagnostic_body(&errors, "invalid connect: "),
        "connect a -> b { \"abcdefghijklmnopqrstu·…",
        &errors,
    );
}

// ── controls: excerpts already inside the bound are unchanged ─────

#[test]
fn short_malformed_connect_is_unchanged() {
    // Same malformed-mapping shape as above with a short member name, so the excerpt is
    // inside the bound. Green before and after the fix: its job is to prove the bound leaves
    // short diagnostics — and the `invalid connect: ` prefix — exactly as they were.
    let errors =
        parse_errors("structure S { port a : out T  port b : in T  connect a -> b { shaft -> } }");
    assert_verbatim(
        diagnostic_body(&errors, "invalid connect: "),
        "connect a -> b { shaft -> }",
        &errors,
    );
}

#[test]
fn short_multibyte_diagnostic_is_unchanged() {
    // The same malformed string-literal member, short enough that no cut happens at all, so a
    // multi-byte excerpt inside the bound is reproduced byte for byte. Green before and after.
    let errors = parse_errors(
        "structure S { port a : out T  port b : in T  connect a -> b { \"kg·m^-3\" -> } }",
    );
    assert_verbatim(
        diagnostic_body(&errors, "invalid connect: "),
        "connect a -> b { \"kg·m^-3\"",
        &errors,
    );
}

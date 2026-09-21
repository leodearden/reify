//! The `check_and_lower!` diagnostic's source excerpt is length-bounded.
//!
//! INV-SF-7 `parse-is-value-faithful` (docs/legibility/design-invariants.md), task #5392:
//! a diagnostic that reprints a block of source is unreadable, and on a recovered parse the
//! offending node can span an entire declaration. These tests pin that the `invalid <label>: `
//! diagnostics emitted by `check_and_lower!` carry a BOUNDED excerpt, while leaving the
//! prefix — and any excerpt already inside the bound — untouched.
//!
//! Asserted through the public `reify_syntax::parse` API on the messages it actually emits,
//! not against the private `snippet` helper: the contract is the user-visible diagnostic.
//! `snippet`'s own mechanics (newline cut, char-boundary cut, short-text passthrough) are
//! unit-tested once in `ts_parser`'s `mod tests` and are deliberately not restated here.

use reify_ast::ParseError;

/// Helper: parse source and return only the parse errors.
fn parse_errors(source: &str) -> Vec<ParseError> {
    reify_syntax::parse(source, reify_core::ModulePath::single("snippet_bound_test")).errors
}

/// The excerpt body of an `invalid <label>: ` diagnostic, or a failure naming what was emitted.
fn diagnostic_body<'a>(errors: &'a [ParseError], prefix: &str) -> &'a str {
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one parse error, got: {:?}",
        errors
    );
    errors[0]
        .message
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("expected message to start with {prefix:?}, got: {errors:?}"))
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
    let body = diagnostic_body(&errors, "invalid connect: ");
    assert_eq!(
        body, "connect a -> b { shaft_alignment_referen…",
        "expected the excerpt cut at 40 chars, got: {:?}",
        errors
    );
    // Pinned independently of the exact cut point, so a legitimate grammar change to the
    // node's text cannot silently unbound the diagnostic.
    assert!(
        body.ends_with('…'),
        "expected a truncated excerpt to end in an ellipsis, got: {:?}",
        errors
    );
    assert!(
        body.chars().count() <= 41,
        "expected the excerpt bounded to 40 chars + ellipsis, got {} chars: {:?}",
        body.chars().count(),
        errors
    );
}

#[test]
fn truncation_of_a_multibyte_snippet_lands_on_a_character_boundary() {
    // `let a = 7850 kg·m^-3` (space between magnitude and unit) is a known, still-open parse
    // gap — see crates/reify-compiler/tests/harness_units/unit_middot_mul_tests.rs:13 — that
    // keeps the `let_declaration` kind with has_error() set, so it reaches `check_and_lower!`.
    //
    // WHY THIS FIXTURE: the node text `let abcdefghijklmnopqrstuvwxy = 7850 kg·m^-3` is 44
    // chars but 45 bytes, and byte offset 40 is NOT a char boundary — the U+00B7 MIDDLE DOT
    // occupies bytes 39-40. A regression from `char_indices` to a byte slice `&first_line[..40]`
    // therefore PANICS here rather than merely mis-asserting. The 40th character is the `·`
    // itself, so a correct cut keeps it intact and appends the ellipsis after it.
    let errors = parse_errors("structure S { let abcdefghijklmnopqrstuvwxy = 7850 kg·m^-3 }");
    let body = diagnostic_body(&errors, "invalid let: ");
    assert_eq!(
        body, "let abcdefghijklmnopqrstuvwxy = 7850 kg·…",
        "expected the excerpt cut on a char boundary with the `·` intact, got: {:?}",
        errors
    );
    assert!(
        body.ends_with('…'),
        "expected a truncated excerpt to end in an ellipsis, got: {:?}",
        errors
    );
    assert!(
        body.chars().count() <= 41,
        "expected the excerpt bounded to 40 chars + ellipsis, got {} chars: {:?}",
        body.chars().count(),
        errors
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
    let body = diagnostic_body(&errors, "invalid connect: ");
    assert_eq!(
        body, "connect a -> b { shaft -> }",
        "expected a short excerpt reproduced verbatim, got: {:?}",
        errors
    );
    assert!(
        !body.contains('…'),
        "expected no ellipsis on an excerpt inside the bound, got: {:?}",
        errors
    );
}

#[test]
fn short_multibyte_diagnostic_is_unchanged() {
    // The same `kg·m^-3` parse gap with a short binding name: multi-byte, but inside the
    // bound, so no cut happens at all. Green before and after the fix.
    let errors = parse_errors("structure S { let a = 7850 kg·m^-3 }");
    let body = diagnostic_body(&errors, "invalid let: ");
    assert_eq!(
        body, "let a = 7850 kg·m^-3",
        "expected a short multi-byte excerpt reproduced verbatim, got: {:?}",
        errors
    );
    assert!(
        !body.contains('…'),
        "expected no ellipsis on an excerpt inside the bound, got: {:?}",
        errors
    );
}

//! Behavioural pins for `reify_test_support::assert_single_declaration_error`,
//! the shared declaration-site diagnostic assertion.
//!
//! A copy that quietly stopped panicking would make every call site vacuous
//! (same rationale as `dimensioned_assertions.rs`), so each invariant it
//! enforces is pinned by a source that violates it. Panic pins match a SHORT
//! payload substring naming the failed invariant, never full message prose.

use reify_core::DiagnosticCode;
use reify_test_support::assert_single_declaration_error;

const LET_MASS_AT_LENGTH: &str = "structure S { let a : Length = 5kg }";

#[test]
fn accepts_a_single_let_mismatch_and_returns_its_message() {
    let message = assert_single_declaration_error(
        LET_MASS_AT_LENGTH,
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
    assert!(
        message.contains("'a'"),
        "the returned value must be the diagnostic's own message; got: {message:?}"
    );
}

#[test]
fn accepts_a_single_param_mismatch() {
    assert_single_declaration_error(
        "structure S { param m : Length = 5kg }",
        DiagnosticCode::ParamDefaultTypeMismatch,
        "param",
        "m",
    );
}

#[test]
#[should_panic(expected = "expected exactly one error")]
fn rejects_a_clean_source() {
    assert_single_declaration_error(
        "structure S { let a : Length = 5mm }",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
}

#[test]
#[should_panic(expected = "expected exactly one error")]
fn rejects_two_errors() {
    assert_single_declaration_error(
        "structure S {\n let a : Length = 5kg\n let b : Length = 5kg\n}",
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "a",
    );
}

#[test]
#[should_panic(expected = "expected ParamDefaultTypeMismatch")]
fn rejects_the_wrong_code() {
    assert_single_declaration_error(
        LET_MASS_AT_LENGTH,
        DiagnosticCode::ParamDefaultTypeMismatch,
        "let",
        "a",
    );
}

#[test]
#[should_panic(expected = "must quote 'b'")]
fn rejects_a_message_that_does_not_quote_the_name() {
    assert_single_declaration_error(
        LET_MASS_AT_LENGTH,
        DiagnosticCode::LetAnnotationTypeMismatch,
        "let",
        "b",
    );
}

/// The message quotes `'a'`, but the label `let a : Length = 5kg` does not
/// contain `param a`: only the anchor check can reject this.
#[test]
#[should_panic(expected = "label must cover 'param a'")]
fn rejects_a_label_not_anchored_at_the_declaration() {
    assert_single_declaration_error(
        LET_MASS_AT_LENGTH,
        DiagnosticCode::LetAnnotationTypeMismatch,
        "param",
        "a",
    );
}

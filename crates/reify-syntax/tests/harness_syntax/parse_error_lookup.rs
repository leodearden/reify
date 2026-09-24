//! Parse-error lookup shared by this harness's diagnostic tests: run the public
//! `reify_syntax::parse` and pick out the one diagnostic a test is about.
//!
//! One copy, so every consumer selects its diagnostic by the same rule and fails with the same
//! full error list.

use reify_ast::ParseError;

/// Parse `source` and return only its parse errors.
pub fn parse_errors(source: &str) -> Vec<ParseError> {
    reify_syntax::parse(source, reify_core::ModulePath::single("parse_error_lookup")).errors
}

/// The one error whose message starts with `prefix`, or a failure naming every error emitted.
///
/// Selects by prefix rather than requiring a lone error, so a second diagnostic elsewhere in the
/// fixture cannot red a test for an unrelated reason.
#[track_caller]
pub fn only_error_starting_with<'a>(errors: &'a [ParseError], prefix: &str) -> &'a ParseError {
    let mut matching = errors.iter().filter(|e| e.message.starts_with(prefix));
    let error = matching
        .next()
        .unwrap_or_else(|| panic!("expected an error starting with {prefix:?}, got: {errors:?}"));
    assert!(
        matching.next().is_none(),
        "expected exactly one {prefix:?} diagnostic, got: {errors:?}"
    );
    error
}

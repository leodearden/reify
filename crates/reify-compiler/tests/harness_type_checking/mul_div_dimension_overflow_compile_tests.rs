//! Compile-time dimension-overflow guard for value-level `*`/`/` (task 9236,
//! `E_DIMENSION_EXPONENT_OVERFLOW`): a product or quotient whose combined
//! dimension has an exponent that does not fit `Rational` (i16/i16) is one
//! located, coded error, never a compiler panic.

use reify_core::{Diagnostic, DiagnosticCode};
use reify_test_support::{compile_source, errors_only};

/// Compile `structure def P { <body> }`; return the source and its errors.
fn compile_errors(body: &str) -> (String, Vec<Diagnostic>) {
    let source = format!("structure def P {{ {body} }}");
    let module = compile_source(&source);
    let errors = errors_only(&module).into_iter().cloned().collect();
    (source, errors)
}

/// Assert `body` compiles to exactly one error, coded
/// `DimensionExponentOverflow`, whose first label covers `labelled`.
#[track_caller]
fn assert_one_overflow_labelled(body: &str, labelled: &str) {
    let (source, errors) = compile_errors(body);
    assert_eq!(errors.len(), 1, "`{body}`: want one error, got {errors:?}");
    let error = &errors[0];
    assert_eq!(
        error.code,
        Some(DiagnosticCode::DimensionExponentOverflow),
        "`{body}`: {error:?}"
    );
    let span = error
        .labels
        .first()
        .unwrap_or_else(|| panic!("`{body}`: unlabelled {error:?}"))
        .span;
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        labelled,
        "`{body}`: the label must cover the overflowing operator node"
    );
}

#[test]
fn product_whose_dimension_overflows_is_one_located_error() {
    assert_one_overflow_labelled(
        "let a = 1m^1/31 * 1m^1/37 * 1m^1/41",
        "1m^1/31 * 1m^1/37 * 1m^1/41",
    );
}

#[test]
fn quotient_whose_dimension_overflows_is_one_located_error() {
    assert_one_overflow_labelled(
        "let a = 1m^1/31 * 1m^1/37 / 1m^-1/41",
        "1m^1/31 * 1m^1/37 / 1m^-1/41",
    );
}

#[test]
fn overflow_through_bindings_is_labelled_at_the_combining_operator() {
    assert_one_overflow_labelled(
        "let x = 1m^1/31 * 1m^1/37  let y = 1m^1/41  let z = x * y",
        "x * y",
    );
}

#[test]
fn an_overflowed_binding_does_not_cascade() {
    let (_, errors) = compile_errors("let a = 1m^1/31 * 1m^1/37 * 1m^1/41  let w = a * 2");
    assert_eq!(errors.len(), 1, "want one error, got {errors:?}");
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::DimensionExponentOverflow)
    );
}

#[test]
fn products_whose_dimension_fits_compile_cleanly() {
    for body in ["let a = 1m^1/31 * 1m^1/37", "let a = 1m^1/127 * 1m^1/126"] {
        let (_, errors) = compile_errors(body);
        assert!(errors.is_empty(), "`{body}`: {errors:?}");
    }
}

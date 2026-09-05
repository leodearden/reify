//! Placeholder — module doc and test content are written in task 6143 step-3/step-4/step-5.

use reify_core::DiagnosticCode;
use reify_test_support::{
    assert_error_code_present, assert_no_error_diagnostics, compile_source_with_stdlib,
};

// ══════════════════════════════════════════════════════════════════════════════
// AXIS 1 — DIMENSION MISMATCH (task 6143 step-3)
//
// A three-test set, not three independent tests. (a) pins the carve-out — the
// trap itself. (b) is (a)'s NON-VACUITY GUARD: it proves the same trait body
// IS checkable, so (a)'s zero-error result reports a real deferral rather than
// a compiler that has stopped looking. (c) proves (b)'s error is caused by the
// MISMATCH and not merely by the presence of a conformer.
//
// Together they establish that the axis is trait-WITHOUT-CONFORMER vs
// conformed — NOT trait-vs-structure, as task 6143's description hypothesised.
// ══════════════════════════════════════════════════════════════════════════════

/// A trait body whose constraint compares `Mass` against a `Length` literal
/// emits ZERO error diagnostics when NO structure conforms to the trait.
///
/// This is the carve-out, and the trap: an absence-of-diagnostic assertion
/// written against this shape passes no matter what the compiler does.
///
/// Measured on main @ 6927f3c0db: 0 errors. Its non-vacuity guard is
/// [`conformed_trait_body_dimension_mismatch_errors`] below — the SAME trait
/// body, plus a conformer, does error.
#[test]
fn trait_body_without_conformer_is_not_dimension_checked() {
    let source = r#"
trait Probe {
    param mass : Mass
    constraint mass > 1m
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_no_error_diagnostics(
        &module.diagnostics,
        "trait body with no conformer (dimension mismatch deferred to conformance)",
    );
}

/// NON-VACUITY GUARD for [`trait_body_without_conformer_is_not_dimension_checked`].
///
/// The SAME trait body, plus a conforming structure, DOES emit
/// `DiagnosticCode::DimensionMismatch`. This is what makes the zero-error
/// result above a measurement of deferral rather than of absence.
///
/// Measured on main @ 6927f3c0db: 1 error, `DimensionMismatch`, message
/// "dimension mismatch in comparison: Scalar[kg] vs Scalar[m]". The message
/// text is recorded here (not asserted) so a future reader can tell a
/// behaviour change from a mere rewording.
#[test]
fn conformed_trait_body_dimension_mismatch_errors() {
    let source = r#"
trait Probe {
    param mass : Mass
    constraint mass > 1m
}

structure def Conformer : Probe {
    param mass : Mass = 1kg
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_error_code_present(
        &module.diagnostics,
        DiagnosticCode::DimensionMismatch,
        "conformed trait body with a Mass-vs-Length constraint",
    );
}

/// Proves [`conformed_trait_body_dimension_mismatch_errors`] fires on the
/// MISMATCH, not merely on the presence of a conformer: the same conformer
/// shape with a dimensionally-consistent constraint is clean.
///
/// Measured on main @ 6927f3c0db: 0 errors.
#[test]
fn conformed_trait_body_matching_dimensions_is_clean() {
    let source = r#"
trait Probe {
    param mass : Mass
    constraint mass > 1kg
}

structure def Conformer : Probe {
    param mass : Mass = 1kg
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_no_error_diagnostics(
        &module.diagnostics,
        "conformed trait body with a dimensionally-consistent constraint",
    );
}

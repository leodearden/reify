//! Trait-body checking is DEFERRED to conformance — and the vacuity trap that
//! follows from it (task 6143).
//!
//! # The measured rule
//!
//! Trait-body member expressions are dimension- and member-checked at
//! **conformance**, not at trait declaration. A trait compiled with **no**
//! conforming structure emits **zero** error diagnostics no matter how broken
//! its body is: a dimension-mismatched constraint, a reference to an undefined
//! member field, and a wrong `let` type annotation all pass silently.
//!
//! # Consequence — the trap
//!
//! **Never write an absence-of-diagnostic assertion against a bare trait
//! body.** It passes regardless of what the code under test does, so it pins
//! nothing and reports green forever. Any such probe must either declare a
//! conformer (`structure def C : T { ... }`) or be rewritten as a `structure`.
//!
//! The vacuity condition is precisely **"trait body with no conformer"** — NOT
//! "trait body". A conformed trait body is fully checkable, so a guard that
//! regressed there would be LOUD, not silent. That narrowness is the whole
//! reason each pin below is written as a PAIR: the conformed half is what
//! makes the unconformed half's zero-error result a measurement of *deferral*
//! rather than of *absence*.
//!
//! # Measurement provenance
//!
//! Measured on `main` at commit 6927f3c0db via
//! `reify_test_support::compile_source_with_stdlib`, counting
//! `Severity::Error` diagnostics. Every fixture below reproduces that
//! measurement through the same entry point. Conformance was found to catch
//! all three axes — `DimensionMismatch`, `StructureMemberNotFound` and
//! `TypeMismatchForTraitMember` — so no follow-up task was warranted for the
//! undefined-field half; these pins are what keep that true.
//!
//! # Fixture hazard for future authors
//!
//! These tests assert by diagnostic **code**, never by error count. That is
//! forced, not stylistic:
//!
//! - A struct-literal param default (`param bearer : Bearer = Bearer { .. }`)
//!   emits an unrelated `unknown variant 'Bearer': no enum in scope declares
//!   it` error (code `None`). Do not try to eliminate it — it is pre-existing
//!   behaviour outside task 6143's scope, and the code-keyed assertions are
//!   immune to it.
//! - An EMPTY struct literal (`Bearer {}`) is a **parse** error, so it is not
//!   an escape from the above.
//! - The stdlib alternative (`param material : Material = Steel`) emits
//!   `UnresolvedName`, so it is not an escape either.
//!
//! So no count-based assertion is even *expressible* on the axis-2 fixtures.
//! Code-keyed assertion is also the strictly stronger pin in general: an
//! unrelated future diagnostic keeps a bare non-empty check green while the
//! guard it protects rots away. See
//! `reify_test_support::assert_error_code_present`.
//!
//! Polymorphic-zero coercion (`mass > 0` acquiring the operand's dimension) is
//! a *separate* rule that interacts with these fixtures but is not re-derived
//! here; it belongs to `polymorphic_zero_tests.rs`.

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

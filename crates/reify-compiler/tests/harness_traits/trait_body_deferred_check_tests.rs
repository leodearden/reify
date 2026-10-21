//! Trait-body checking is DEFERRED to conformance — and the vacuity trap that
//! follows from it (task 6143).
//!
//! Trait-body member expressions are dimension- and member-checked at
//! **conformance**, not at trait declaration: a trait with no conforming
//! structure emits zero error diagnostics however broken its body is. So never
//! write an absence-of-diagnostic assertion against a bare trait body — it
//! passes regardless of what the code under test does. Declare a conformer
//! (`structure def C : T { ... }`), or write the probe as a `structure`.
//!
//! The vacuity condition is precisely "trait body with NO conformer", not
//! "trait body": a conformed body is fully checkable, so a guard that regressed
//! there would be loud rather than silent. Hence the pairing invariant every
//! axis below obeys — a carve-out pin, the conformed non-vacuity guard that
//! makes its zero-error result a measurement of *deferral* rather than of
//! *absence*, and a control showing the guard fires on the defect rather than
//! on the conformer.
//!
//! Assertions are keyed on `DiagnosticCode`, never on error count: the
//! conformed fixtures carry an unrelated codeless `unknown variant` error from
//! their struct-literal param default, which no count-based assertion tolerates.

use reify_core::DiagnosticCode;
use reify_test_support::{
    assert_error_code_absent, assert_error_code_present, assert_no_error_diagnostics,
    compile_source_with_stdlib,
};

// ── AXIS 1: dimension mismatch ──────────────────────────────────────────────

/// Carve-out pin — guarded by [`conformed_trait_body_dimension_mismatch_errors`].
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

/// Non-vacuity guard for [`trait_body_without_conformer_is_not_dimension_checked`].
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

/// Control for [`conformed_trait_body_dimension_mismatch_errors`]: the same
/// conformed shape, dimensionally consistent, is clean.
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

// ── AXIS 2: undefined member field ──────────────────────────────────────────
//
// Fixtures use a user-defined `Bearer` rather than the stdlib `Material`:
// `param material : Material = Steel` emits an unrelated `UnresolvedName`,
// which buys nothing over the `unknown variant` noise it would replace.

/// Carve-out pin — guarded by [`conformed_trait_body_undefined_member_errors`].
#[test]
fn trait_body_without_conformer_ignores_undefined_member() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    constraint bearer.no_such_field > 0
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_no_error_diagnostics(
        &module.diagnostics,
        "trait body with no conformer (undefined member deferred to conformance)",
    );
}

/// Non-vacuity guard for [`trait_body_without_conformer_ignores_undefined_member`].
#[test]
fn conformed_trait_body_undefined_member_errors() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    constraint bearer.no_such_field > 0
}

structure def Conformer : Probe {
    param bearer : Bearer = Bearer { density: 1kg/m^3 }
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_error_code_present(
        &module.diagnostics,
        DiagnosticCode::StructureMemberNotFound,
        "conformed trait body referencing an undefined member field",
    );
}

/// Control for [`conformed_trait_body_undefined_member_errors`]: the same
/// conformer over an EXISTING member emits no `StructureMemberNotFound`.
#[test]
fn conformed_trait_body_existing_member_has_no_member_not_found() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    constraint bearer.density > 0kg/m^3
}

structure def Conformer : Probe {
    param bearer : Bearer = Bearer { density: 1kg/m^3 }
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_error_code_absent(
        &module.diagnostics,
        DiagnosticCode::StructureMemberNotFound,
        "conformed trait body referencing an EXISTING member field",
    );
}

// ── AXIS 3: wrong `let` type annotation ─────────────────────────────────────

/// Carve-out pin — guarded by [`conformed_trait_body_wrong_let_annotation_errors`].
#[test]
fn trait_body_without_conformer_ignores_wrong_let_annotation() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    let bad : Length = bearer.density
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_no_error_diagnostics(
        &module.diagnostics,
        "trait body with no conformer (wrong `let` annotation deferred to conformance)",
    );
}

/// Non-vacuity guard for [`trait_body_without_conformer_ignores_wrong_let_annotation`].
#[test]
fn conformed_trait_body_wrong_let_annotation_errors() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    let bad : Length = bearer.density
}

structure def Conformer : Probe {
    param bearer : Bearer = Bearer { density: 1kg/m^3 }
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_error_code_present(
        &module.diagnostics,
        DiagnosticCode::TypeMismatchForTraitMember,
        "conformed trait body with a wrong `let` type annotation",
    );
}

/// Control for [`conformed_trait_body_wrong_let_annotation_errors`]: the same
/// conformed `let` with a MATCHING annotation emits no
/// `TypeMismatchForTraitMember`.
#[test]
fn conformed_trait_body_matching_let_annotation_has_no_type_mismatch() {
    let source = r#"
structure Bearer {
    param density : Density = 1kg/m^3
}

trait Probe {
    param bearer : Bearer
    let ok : Density = bearer.density
}

structure def Conformer : Probe {
    param bearer : Bearer = Bearer { density: 1kg/m^3 }
}
"#;

    let module = compile_source_with_stdlib(source);

    assert_error_code_absent(
        &module.diagnostics,
        DiagnosticCode::TypeMismatchForTraitMember,
        "conformed trait body with a matching `let` type annotation",
    );
}

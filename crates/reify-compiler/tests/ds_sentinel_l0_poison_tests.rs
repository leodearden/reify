//! L0 ds-sentinel poison tests (task #4645).
//!
//! Pins the surface-reachable producer sites where an unresolved type NAME in a
//! structure- or port-param position must resolve the producer's RESOLVED TYPE to
//! `Type::Error` (poison), NOT `Type::dimensionless_scalar()`.
//!
//! Rationale (PRD docs/prds/dimensionless-scalar-sentinel-stampout.md §9 L0): a
//! `dimensionless_scalar()` fallback at a resolution-failure site leaks a silent
//! `Real` into downstream scope/body/overload/conformance resolution and spawns a
//! secondary mis-typed cascade. Returning `Type::Error` engages the existing
//! anti-cascade guards (`check_param_default_type`'s `declared.is_error()` early
//! return; `implicitly_converts_to(Error, _) => true` in type_compat.rs) so the
//! root-cause diagnostic stands alone.
//!
//! DISCRIMINATOR: each producer-side test asserts `.is_error()` on the
//! compiled value-cell's `cell_type` — the precise effect of the L0 fix
//! (dimensionless == not-error pre-fix -> Error == is-error post-fix). A
//! diagnostic-count test on a site whose cascade is already suppressed by other
//! guards would be GREEN pre-fix and thus a doomed RED; `.is_error()` is
//! genuinely RED pre-fix.
//!
//! The anti-cascade closure tests confirm the end-to-end headline: exactly ONE
//! error (UnresolvedType only) when an unresolved type name appears in a param
//! declaration with a dimensioned default value — no secondary mismatch.

use reify_compiler::find_template;
use reify_core::DiagnosticCode;
use reify_test_support::{compile_source, errors_only};

/// entity.rs Tier-1 top-level structure param position (site :1014): an unresolved
/// NAME `Bogus` in a structure param type annotation must make the compiled
/// value-cell's `cell_type` poison (`Type::Error`), not a silent dimensionless `Real`.
///
/// Covers the `else` arm after the `UnresolvedType` diagnostic is pushed at entity.rs:1003,
/// which currently falls back to `Type::dimensionless_scalar()` — genuinely RED pre-fix.
#[test]
fn structure_param_unresolved_name_cell_type_is_error() {
    let module = compile_source("structure S { param p : Bogus }");
    let tmpl = find_template(&module.templates, "S")
        .expect("template S should be compiled despite the unresolved type");
    let cell = tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "p")
        .expect("value cell for param 'p' must be present");
    assert!(
        cell.cell_type.is_error(),
        "unresolved param type `Bogus` must make the value-cell cell_type \
         Type::Error (poison), not a silent dimensionless Real; got: {:?}",
        cell.cell_type
    );
}

/// Anti-cascade closure for the top-level structure param (Tier-1 :1014 + the
/// `check_param_default_type` guard at entity.rs:430):
/// `structure S { param p : Bogus = 5kg }` must produce exactly ONE error
/// (the root-cause `UnresolvedType`) with NO secondary `ParamDefaultTypeMismatch`.
///
/// Pre-fix: `Bogus` resolves to `Type::Real`, the anti-cascade guard does not fire,
/// and a spurious `ParamDefaultTypeMismatch` is emitted alongside `UnresolvedType`.
/// This test is genuinely RED on current main (headline regression).
///
/// Note: this is the structure-param counterpart of the test already flipped in
/// param_default_type_mismatch_tests.rs (step-1 of this task); keeping it here as
/// part of the L0 sentinel file for completeness with the port-param coverage below.
#[test]
fn structure_param_unresolved_type_anti_cascade_no_secondary_error() {
    let source = "structure S { param p : Bogus = 5kg }";
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected an UnresolvedType error for unresolved type 'Bogus'; got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_none(),
        "expected NO ParamDefaultTypeMismatch (anti-cascade: unknown-name -> Type::Error \
         -> declared.is_error() guard fires -> mismatch check suppressed); \
         got unexpected secondary mismatch: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// entity.rs Tier-1 port-member param position (site :1282): an unresolved NAME
/// `Bogus` in a port-member param type annotation must produce exactly ONE error
/// (the root-cause `UnresolvedType`) with NO secondary `ParamDefaultTypeMismatch`.
///
/// Mechanism: the port-param pass-1 registration falls back to
/// `Type::dimensionless_scalar()` at :1282 (pre-fix), which leaks a silent `Real`
/// into `check_param_default_type`'s `cell_type` readback — the anti-cascade guard
/// (`declared.is_error()`) does not fire, and a spurious mismatch is emitted.
/// After the fix, :1282 returns `Type::Error`, the guard fires, and only the
/// root-cause `UnresolvedType` survives.
#[test]
fn port_param_unresolved_type_anti_cascade_no_secondary_error() {
    let source = r#"
structure S {
    port p : out MockPort {
        param x : Bogus = 5kg
    }
}
trait MockPort {
    param x : Real
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.iter().any(|d| d.code == Some(DiagnosticCode::UnresolvedType)),
        "expected an UnresolvedType error for unresolved port-param type 'Bogus'; got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_none(),
        "expected NO ParamDefaultTypeMismatch for port-param 'x : Bogus = 5kg' \
         (anti-cascade: unknown-name -> Type::Error -> guard fires -> mismatch suppressed); \
         got unexpected secondary mismatch: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

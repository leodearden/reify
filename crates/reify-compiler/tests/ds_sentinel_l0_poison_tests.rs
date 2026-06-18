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
use reify_core::{DiagnosticCode, Type};
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

/// entity.rs Tier-2 non-arm Sub type-arg `_ =>` arm (site :1865): an invalid
/// type argument (IntegerLiteral `5`) in a sub instantiation must lower the
/// sub-component's `type_args[0]` to `Type::Error` (poison), NOT a silent
/// dimensionless `Real`.
///
/// `5` as a type arg hits `TypeExprKind::IntegerLiteral(5)` which is the `_ =>`
/// arm in entity.rs (only `Named` and `Auto` have dedicated match arms). Pre-fix:
/// `Type::dimensionless_scalar()` is returned — `.is_error()` is false. Post-fix:
/// `Type::Error` is returned — `.is_error()` is true. Genuinely RED pre-fix.
#[test]
fn sub_invalid_type_arg_resolves_to_error() {
    let source = r#"
structure def Foo<T> { param x : Real = 1.0 }
structure def Asm { sub b = Foo<5>() }
"#;
    let module = compile_source(source);
    let tmpl = find_template(&module.templates, "Asm")
        .expect("template Asm should compile despite the invalid type arg");
    let sub_b = tmpl
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("sub 'b' must be present");
    let first_type_arg = sub_b
        .type_args
        .first()
        .expect("sub 'b' must carry the integer type arg as type_args[0]");
    assert!(
        first_type_arg.is_error(),
        "invalid IntegerLiteral type arg `5` must lower to Type::Error (poison sentinel), \
         not a silent dimensionless Real; got: {:?}",
        first_type_arg
    );
    assert_ne!(
        *first_type_arg,
        Type::dimensionless_scalar(),
        "type arg must NOT be Type::dimensionless_scalar() after the fix; got: {:?}",
        first_type_arg
    );
}

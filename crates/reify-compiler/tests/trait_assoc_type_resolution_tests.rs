//! Integration tests for task 3973 (trait associated types ιγ): verifies that
//! bound associated-type names resolve to their concrete types in member
//! `param` annotations.
//!
//! PRD: docs/prds/v0_6/trait-associated-functions.md §4.5, §5.3a, §7.3

use reify_core::{DiagnosticCode, Type};
use reify_test_support::{compile_source, errors_only};

// ─── Step-1 RED: structure-own binding ────────────────────────────────────────

/// A structure that provides its own `type Material = Steel` binding must
/// resolve `param mass : Material` to `Type::StructureRef("Steel")`.
///
/// Fails today: `Material` is not a declared structure name, so
/// `resolve_type_with_aliases` returns `None`, the param arm emits
/// `UnresolvedType` and falls back to `Type::dimensionless_scalar()`.
#[test]
fn bound_assoc_type_resolves_in_param_annotation() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
    type Material = Steel
    param mass : Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("Beam template should be compiled");

    let mass_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "mass")
        .expect("value cell 'mass' should exist");

    assert_eq!(
        mass_cell.cell_type,
        Type::StructureRef("Steel".to_string()),
        "param typed with assoc-type name 'Material' should resolve to \
         Type::StructureRef(\"Steel\"); got: {:?}",
        mass_cell.cell_type
    );
}

// ─── Step-3 RED: inherited trait default ──────────────────────────────────────

/// A structure that provides NO own `type Material = ...` binding but inherits
/// the trait default `type Material = Steel` must also resolve
/// `param mass : Material` to `Type::StructureRef("Steel")`.
///
/// Fails after step-2: the own-binding scope is empty for `Bar`, so `Material`
/// resolves to `None` → `UnresolvedType` → `Type::dimensionless_scalar()`.
#[test]
fn inherited_default_assoc_type_resolves_in_param_annotation() {
    let source = r#"
structure Steel {}
trait HasMaterial { type Material = Steel }
structure def Bar : HasMaterial {
    param mass : Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for inherited default; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bar")
        .expect("Bar template should be compiled");

    let mass_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "mass")
        .expect("value cell 'mass' should exist");

    assert_eq!(
        mass_cell.cell_type,
        Type::StructureRef("Steel".to_string()),
        "param typed with inherited-default assoc-type 'Material' should resolve to \
         Type::StructureRef(\"Steel\"); got: {:?}",
        mass_cell.cell_type
    );
}

// ─── Step-5 RED: anti-cascade poison ──────────────────────────────────────────

/// A required assoc type with NO structure binding and NO trait default must
/// produce exactly one `TraitAssocTypeNotBound` error and ZERO `UnresolvedType`
/// errors (anti-cascade).
///
/// Fails after step-4: `Material` is unbound/undefaulted, so `resolve_assoc_type_name`
/// returns `None` → the param arm emits `UnresolvedType`, producing a second
/// (spurious) cascade error alongside the `TraitAssocTypeNotBound` from conformance.
#[test]
fn unbound_required_assoc_type_no_unresolved_type_cascade() {
    let source = r#"
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
    param mass : Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let not_bound: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitAssocTypeNotBound))
        .collect();
    assert_eq!(
        not_bound.len(),
        1,
        "expected exactly one TraitAssocTypeNotBound diagnostic; all errors: {:?}",
        errors
    );

    let unresolved: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
        .collect();
    assert!(
        unresolved.is_empty(),
        "expected zero UnresolvedType diagnostics (anti-cascade); got: {:?}",
        unresolved
    );
}

// ─── Amendment: own-binding-wins-over-trait-default precedence ────────────────

/// When the structure provides its own `type Material = Steel` while the trait
/// defaults `type Material = Iron`, the structure's own binding must win.
///
/// This verifies the `entry().or_insert_with` ownership rule: the own-binding
/// scope is inserted first; the trait-default insertion only fills *absent*
/// names, so `Material → Steel` is never overwritten by `Material → Iron`.
#[test]
fn own_binding_wins_over_trait_default() {
    let source = r#"
structure Steel {}
structure Iron {}
trait HasMaterial { type Material = Iron }
structure def Beam : HasMaterial {
    type Material = Steel
    param mass : Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors when own binding overrides trait default; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("Beam template should be compiled");

    let mass_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "mass")
        .expect("value cell 'mass' should exist");

    assert_eq!(
        mass_cell.cell_type,
        Type::StructureRef("Steel".to_string()),
        "own binding (Steel) must win over trait default (Iron); got: {:?}",
        mass_cell.cell_type
    );
}

// ─── Amendment: bad-RHS single-diagnostic, no param cascade ──────────────────

/// When the structure binds `type Material = NoSuchType` (bad RHS), exactly one
/// `UnresolvedType` diagnostic must be emitted — the authoritative one from the
/// conformance checker's `collect_structure_assoc_type_bindings` call.  The
/// `param mass : Material` annotation must NOT emit a second `UnresolvedType`:
/// entity.rs's throwaway-sink walk resolves the bad RHS to `Type::Error` and
/// inserts `Material → Type::Error` into the assoc-type scope, so the param arm
/// gets `Some(Type::Error)` back from `resolve_assoc_type_name` and stays silent.
#[test]
fn bad_rhs_single_unresolved_type_no_param_cascade() {
    let source = r#"
trait HasMaterial { type Material }
structure def Beam : HasMaterial {
    type Material = NoSuchType
    param mass : Material
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let unresolved: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "expected exactly one UnresolvedType (for the bad RHS 'NoSuchType'); \
         param arm must stay silent; all errors: {:?}",
        errors
    );
}

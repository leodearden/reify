//! Integration tests for task 3972 (trait associated types): the
//! producer-end signal driven through the FULL compile pipeline via
//! `reify_test_support::compile_source` (grammar + lowering landed by 3971).
//!
//! Step-7 (RED): the unbound required-assoc-type test fails until step-8 wires
//! the AssocType satisfaction arm in checker.rs (today the inert arm does
//! `continue` so no `TraitAssocTypeNotBound` is emitted).
//!
//! Step-9 (RED): the resolved-table tests fail until step-10 wires entity.rs
//! to store the resolved assoc-type table onto each conformer's
//! `TopologyTemplate.assoc_types` (today entity.rs stores an empty Vec).

use reify_core::{DiagnosticCode, Type};
use reify_test_support::{compile_source, errors_only};

/// A trait with a bodyless required associated type `type Material` plus a
/// structure that declares conformance but provides NO `type Material = ...`
/// binding must surface `TraitAssocTypeNotBound` naming the type and the
/// declaring trait — through the full pipeline.
///
/// RED (step-7): fails today because checker.rs has an inert
/// `RequirementKind::AssocType(_) => continue` arm that skips the
/// satisfaction check, so no diagnostic is emitted.
#[test]
fn required_assoc_type_unbound_emits_diagnostic_end_to_end() {
    let source = r#"
trait HasMaterial {
    type Material
}
structure def Beam : HasMaterial {
    param w : Length = 1
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
        "expected exactly one TraitAssocTypeNotBound error for the unbound \
         associated type 'Material'; all diagnostics: {:?}",
        module.diagnostics
    );
    let msg = &not_bound[0].message;
    assert!(
        msg.contains("Material"),
        "diagnostic should name the missing type 'Material'; got: {}",
        msg
    );
    assert!(
        msg.contains("HasMaterial"),
        "diagnostic should name the declaring trait 'HasMaterial'; got: {}",
        msg
    );
}

/// A trait with a default `type Material = Steel` satisfies the requirement
/// automatically — a conforming structure with no explicit `type Material = ...`
/// binding should compile with zero errors.
///
/// GREEN already after step-6 (the default satisfies the requirement via the
/// inert Continue arm — no diagnostic is emitted). Pinned here so step-8
/// doesn't accidentally regress the default-satisfies case.
#[test]
fn required_assoc_type_satisfied_by_default_no_diagnostic() {
    let source = r#"
structure Steel {}
trait HasMaterial {
    type Material = Steel
}
structure def Bar : HasMaterial {
    param w : Length = 1
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let not_bound: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TraitAssocTypeNotBound))
        .collect();
    assert!(
        not_bound.is_empty(),
        "default-provided assoc type should satisfy the requirement with no \
         TraitAssocTypeNotBound error; all diagnostics: {:?}",
        module.diagnostics
    );
}

/// (a) A structure that explicitly overrides the trait default populates
/// `TopologyTemplate.assoc_types` with `is_override = true` and the
/// structure-provided type.
///
/// RED (step-9): fails because entity.rs stores `assoc_types: Vec::new()` per
/// pre-2 (no resolve phase wired yet).
#[test]
fn structure_override_assoc_type_populates_template_table() {
    let source = r#"
structure Steel {}
structure Aluminum {}
trait HasMaterial {
    type Material = Steel
}
structure def Beam : HasMaterial {
    type Material = Aluminum
    param w : Length = 1
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "override should compile cleanly; diagnostics: {:?}",
        module.diagnostics
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("compiled module should contain a template for structure 'Beam'");

    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.trait_name == "HasMaterial" && a.type_name == "Material")
        .unwrap_or_else(|| {
            panic!(
                "Beam template should carry an assoc_types entry for \
                 (HasMaterial, Material); assoc_types = {:?}",
                template.assoc_types
            )
        });

    assert_eq!(
        entry.resolved,
        Type::StructureRef("Aluminum".to_string()),
        "override should resolve to Aluminum; got: {:?}",
        entry.resolved
    );
    assert!(
        entry.is_override,
        "structure provided an explicit binding, so is_override must be true; got: {:?}",
        entry
    );
}

/// (b) A structure that inherits the trait default (no explicit binding)
/// populates `TopologyTemplate.assoc_types` with `is_override = false` and the
/// default-provided type.
///
/// RED (step-9): fails because entity.rs stores `assoc_types: Vec::new()` per
/// pre-2 (no resolve phase wired yet).
#[test]
fn inherited_default_assoc_type_populates_template_table() {
    let source = r#"
structure Steel {}
trait HasMaterial {
    type Material = Steel
}
structure def Bar : HasMaterial {
    param w : Length = 1
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "inherited default should compile cleanly; diagnostics: {:?}",
        module.diagnostics
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bar")
        .expect("compiled module should contain a template for structure 'Bar'");

    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.trait_name == "HasMaterial" && a.type_name == "Material")
        .unwrap_or_else(|| {
            panic!(
                "Bar template should carry an assoc_types entry for \
                 (HasMaterial, Material); assoc_types = {:?}",
                template.assoc_types
            )
        });

    assert_eq!(
        entry.resolved,
        Type::StructureRef("Steel".to_string()),
        "inherited default should resolve to Steel; got: {:?}",
        entry.resolved
    );
    assert!(
        !entry.is_override,
        "structure did not override the default, so is_override must be false; got: {:?}",
        entry
    );
}

/// A structure binding `type X = Typo` where `Typo` is not declared must
/// surface an `UnresolvedType` diagnostic rather than silently compiling.
///
/// Previously `collect_structure_assoc_type_bindings` discarded resolution
/// diagnostics via a throwaway sink, so the unresolvable binding quietly
/// became `Type::Error` (treated as "bound"), suppressed
/// `TraitAssocTypeNotBound`, and produced NO diagnostic at all — the typo
/// went undetected. (task 3972 amendment)
#[test]
fn structure_binding_to_nonexistent_type_emits_diagnostic() {
    let source = r#"
trait HasMaterial {
    type Material
}
structure def Beam : HasMaterial {
    type Material = Typo
    param w : Length = 1
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // The binding to the undeclared type "Typo" must produce at least one error.
    // Acceptable codes: UnresolvedType for the bad annotation, or
    // TraitAssocTypeNotBound if the binding was discarded entirely — either
    // signals the user that something is wrong.  Silently compiling is the bug.
    assert!(
        !errors.is_empty(),
        "binding to undeclared type 'Typo' must produce at least one diagnostic; \
         got none — silent compilation is a UX regression; \
         all diagnostics: {:?}",
        module.diagnostics
    );

    // Specifically we expect an UnresolvedType diagnostic for the bad annotation.
    let unresolved: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "expected exactly one UnresolvedType diagnostic for the undeclared \
         binding target 'Typo'; all diagnostics: {:?}",
        module.diagnostics
    );
}

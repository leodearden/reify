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

use reify_core::DiagnosticCode;
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
    param w : Scalar = 1
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
    param w : Scalar = 1
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

//! Trait-typed params — compilation tests for task 1874.
//!
//! Verify that `param m : Material` and related forms resolve to
//! `Type::TraitObject("Material")` across structures, ports, guards,
//! traits, and conformance checks. Uses the stdlib-enabled helpers so
//! the `Material` trait (from `stdlib/materials_mechanical.ri`) is in
//! scope.

use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};
use reify_types::{Severity, Type};

/// Structure member: `param m : Material` should resolve to `Type::TraitObject("Material")`.
#[test]
fn structure_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        structure def HasMaterial { param m : Material }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "HasMaterial")
        .expect("HasMaterial template should be compiled");

    let m_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "m")
        .expect("value cell 'm' should exist");

    assert_eq!(
        m_cell.cell_type,
        Type::TraitObject("Material".to_string()),
        "param typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
    );
}

/// Port member: `port p : in PortType { param m : Material }` should resolve the
/// port member's param type to `Type::TraitObject("Material")`.
#[test]
fn port_member_param_with_trait_type_resolves_to_trait_object() {
    let source = r#"
        trait PortType {}
        structure def HasPortMember {
            port p : in PortType { param m : Material }
        }
    "#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "HasPortMember")
        .expect("HasPortMember template should be compiled");

    let port = template
        .ports
        .iter()
        .find(|p| p.name == "p")
        .expect("port 'p' should exist");

    let m_member = port
        .members
        .iter()
        .find(|vc| vc.id.member == "p.m")
        .expect("port member 'p.m' should exist");

    assert_eq!(
        m_member.cell_type,
        Type::TraitObject("Material".to_string()),
        "port member typed with trait name Material should resolve to Type::TraitObject(\"Material\")"
    );
}

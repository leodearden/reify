//! Trait conformance compilation tests.
//!
//! Tests for compiling trait declarations, conformance checking,
//! default merging, and composition conflict detection.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected 1 template");
    (template, module.diagnostics)
}

/// Step 1: Compile a trait declaration produces CompiledTrait in CompiledModule.trait_defs.
#[test]
fn compile_trait_produces_compiled_trait() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}
"#;

    let module = compile_module(source);

    // Should have 1 trait def
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait def");
    let trait_def = &module.trait_defs[0];

    // Name should be "Fastener"
    assert_eq!(trait_def.name, "Fastener");

    // Should have 1 required member named "thread_pitch"
    assert_eq!(trait_def.required_members.len(), 1, "expected 1 required member");
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "thread_pitch");

    // Requirement kind should be Param with type Scalar{LENGTH}
    match &req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(*ty, Type::Scalar { dimension: DimensionVector::LENGTH });
        }
        other => panic!("expected RequirementKind::Param, got {:?}", other),
    }
}

/// Step 3: Simple conformance — structure satisfies trait requirement.
#[test]
fn simple_conformance_no_errors() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param thread_pitch : Length = 20mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// Step 5: Missing member — error diagnostic about missing required member.
#[test]
fn missing_member_error() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param length : Length = 10mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error diagnostic for missing member");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("thread_pitch"),
        "error should mention 'missing required member' and 'thread_pitch', got: {}",
        error_msg
    );
}

/// Step 7: Type mismatch — member has wrong type.
#[test]
fn type_mismatch_error() {
    let source = r#"
trait Weighted {
    param mass : Mass
}

structure def S : Weighted {
    param mass : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error diagnostic for type mismatch");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("type mismatch"),
        "error should mention 'type mismatch', got: {}",
        error_msg
    );
}

/// Step 9: Default merging — trait provides default, structure doesn't override.
#[test]
fn default_merging_injects_value_cell() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The template should contain a value cell for 'size' injected from the trait default.
    let size_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "size");
    assert!(
        size_cell.is_some(),
        "expected 'size' value cell from trait default, got cells: {:?}",
        template.value_cells.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    let size_cell = size_cell.unwrap();
    assert_eq!(size_cell.kind, ValueCellKind::Param);
    assert_eq!(
        size_cell.cell_type,
        Type::Scalar { dimension: DimensionVector::LENGTH }
    );
    assert!(size_cell.default_expr.is_some(), "expected default expression for 'size'");
}

/// Step 11: Default override — structure provides its own value, no error, only one cell.
#[test]
fn default_override_uses_structure_value() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
    param size : Length = 20mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Only one 'size' value cell should exist (the structure's, not the trait default).
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(
        size_cells.len(),
        1,
        "expected exactly 1 'size' value cell, got {}",
        size_cells.len()
    );
}

/// Step 13: Multiple trait bounds — structure satisfies both traits.
#[test]
fn multiple_trait_bounds_satisfied() {
    let source = r#"
trait A {
    param a : Length
}

trait B {
    param b : Length
}

structure def X : A + B {
    param a : Length = 1mm
    param b : Length = 2mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

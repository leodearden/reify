//! Occurrence compilation tests.
//!
//! Tests for compiling occurrence definitions into TopologyTemplates with EntityKind::Occurrence.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("occ_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected 1 template");
    (template, module.diagnostics)
}

// ── step-9: compile basic occurrence ─────────────────────────────────

#[test]
fn compile_occurrence_basic() {
    let source = "occurrence def Welding { param method : Length = 10mm }";
    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(template.name, "Welding");
    assert_eq!(template.entity_kind, EntityKind::Occurrence);
    assert!(!template.value_cells.is_empty(), "expected at least 1 value cell");
}

// ── step-11: compile occurrence with ports ────────────────────────────

#[test]
fn compile_occurrence_with_ports() {
    let source = r#"
occurrence def Welding {
    port workpiece : in StructurePort {
        param d : Length = 5mm
    }
    port result : out StructurePort {
        param d : Length = 5mm
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(template.entity_kind, EntityKind::Occurrence);
    assert_eq!(template.ports.len(), 2, "expected 2 ports");

    let workpiece = &template.ports[0];
    assert_eq!(workpiece.name, "workpiece");
    assert_eq!(workpiece.direction, PortDirection::In);
    assert_eq!(workpiece.type_name, "StructurePort");
    assert!(!workpiece.members.is_empty(), "expected port members");

    let result_port = &template.ports[1];
    assert_eq!(result_port.name, "result");
    assert_eq!(result_port.direction, PortDirection::Out);
    assert_eq!(result_port.type_name, "StructurePort");
    assert!(!result_port.members.is_empty(), "expected port members");
}

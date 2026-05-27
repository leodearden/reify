//! Occurrence compilation tests.
//!
//! Tests for compiling occurrence definitions into TopologyTemplates with EntityKind::Occurrence.

use reify_compiler::*;
use reify_test_support::{compile_first_template, compile_source};
use reify_core::*;

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
    assert!(
        !template.value_cells.is_empty(),
        "expected at least 1 value cell"
    );
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

// ── step-13: compile occurrence with constraints ──────────────────────

#[test]
fn compile_occurrence_with_constraints() {
    let source = r#"
occurrence def Welding {
    param speed : Length = 100mm
    constraint speed > 0mm
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
    assert_eq!(template.constraints.len(), 1, "expected 1 constraint");
    assert_eq!(template.constraints[0].expr.result_type, Type::Bool);
}

// ── step-15: port direction validation warnings ───────────────────────

#[test]
fn compile_occurrence_missing_in_port_warning() {
    let source = r#"
occurrence def Welding {
    port result : out StructurePort {
        param d : Length = 5mm
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert_eq!(template.entity_kind, EntityKind::Occurrence);

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("no input port"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected warning about missing input port, got diagnostics: {:?}",
        diagnostics
    );
}

#[test]
fn compile_occurrence_missing_out_port_warning() {
    let source = r#"
occurrence def Welding {
    port workpiece : in StructurePort {
        param d : Length = 5mm
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    assert_eq!(template.entity_kind, EntityKind::Occurrence);

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("no output port"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected warning about missing output port, got diagnostics: {:?}",
        diagnostics
    );
}

#[test]
fn compile_occurrence_with_both_ports_no_warning() {
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
    let (_, diagnostics) = compile_first_template(source);

    let port_direction_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("no input port") || d.message.contains("no output port"))
        .collect();
    assert!(
        port_direction_warnings.is_empty(),
        "expected no port direction warnings, got: {:?}",
        port_direction_warnings
    );
}

// ── step-17: sub declaration referencing an occurrence ─────────────────

#[test]
fn compile_occurrence_sub_instantiation() {
    let source = r#"
occurrence def Welding {
    param method : Length = 10mm
}

structure def Assembly {
    sub step = Welding(method: 10mm)
}
"#;
    let module = compile_source(source);

    // Should have 2 templates: Welding (occurrence) and Assembly (structure)
    assert_eq!(module.templates.len(), 2, "expected 2 templates");

    let assembly = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("expected Assembly template");
    assert_eq!(assembly.entity_kind, EntityKind::Structure);
    assert_eq!(assembly.sub_components.len(), 1, "expected 1 sub-component");
    assert_eq!(assembly.sub_components[0].structure_name, "Welding");
}

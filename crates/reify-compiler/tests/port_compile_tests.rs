//! Port compilation tests.
//!
//! Tests for compiling port declarations into CompiledPort entries in TopologyTemplate.

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

// ── Step 11: compile_port_creates_value_cells ───────────────────────

#[test]
fn compile_port_creates_value_cells() {
    let source = r#"
trait MechPort {
    param diameter : Length
}

structure def S {
    port mount : MechPort {
        param diameter : Length = 5mm
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

    // Should have 1 port
    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];
    assert_eq!(port.name, "mount");
    assert_eq!(port.direction, PortDirection::Bidi); // default
    assert_eq!(port.type_name, "MechPort");

    // Port should have 1 member with id containing 'mount.diameter'
    assert_eq!(port.members.len(), 1, "expected 1 port member");
    assert!(
        port.members[0].id.member.contains("mount.diameter"),
        "expected member id to contain 'mount.diameter', got '{}'",
        port.members[0].id.member
    );
}

// ── Step 13: port type checking ────────────────────────────────────

#[test]
fn compile_port_type_check_known() {
    let source = r#"
trait MechPort {
    param diameter : Length
}

structure def S {
    port mount : MechPort {
        param diameter : Length = 5mm
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No warning about unknown port type since MechPort is defined
    let type_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown port type"))
        .collect();
    assert!(type_warnings.is_empty(), "unexpected type warnings: {:?}", type_warnings);
}

#[test]
fn compile_port_type_check_unknown() {
    let source = r#"
structure def S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Should have a warning about unknown port type
    let type_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown port type"))
        .collect();
    assert_eq!(type_warnings.len(), 1, "expected 1 unknown port type warning, got: {:?}", diagnostics);
}

// ── Step 15: port member access via dot notation ────────────────────

#[test]
fn compile_port_member_access() {
    let source = r#"
trait MechPort {
    param diameter : Length
}

structure def S {
    port mount : MechPort {
        param diameter : Length = 5mm
    }
    let d = mount.diameter
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Should have a let 'd' in value_cells that references the port member
    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("expected value cell 'd'");

    assert_eq!(d_cell.kind, ValueCellKind::Let);
    // The expression should be a ValueRef to the composite ValueCellId
    match &d_cell.default_expr.as_ref().unwrap().kind {
        CompiledExprKind::ValueRef(id) => {
            assert!(
                id.member.contains("mount.diameter"),
                "expected ValueRef to 'mount.diameter', got '{}'",
                id.member
            );
        }
        other => panic!("expected ValueRef, got {:?}", other),
    }
}

// ── Step 17: multiple ports ─────────────────────────────────────────

#[test]
fn compile_multiple_ports() {
    let source = r#"
trait MechPort {
    param d : Length
}
trait RotaryPort {
    param rpm : Length
}

structure def S {
    port mount : MechPort {
        param d : Length = 5mm
    }
    port shaft : RotaryPort {
        param rpm : Length = 100mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Should have 2 ports
    assert_eq!(template.ports.len(), 2, "expected 2 ports");

    let mount = &template.ports[0];
    assert_eq!(mount.name, "mount");
    assert_eq!(mount.members.len(), 1);
    assert!(mount.members[0].id.member.contains("mount.d"));

    let shaft = &template.ports[1];
    assert_eq!(shaft.name, "shaft");
    assert_eq!(shaft.members.len(), 1);
    assert!(shaft.members[0].id.member.contains("shaft.rpm"));

    // ValueCellIds should be distinct
    assert_ne!(mount.members[0].id, shaft.members[0].id);
}

// ── Step 19: port direction preserved ──────────────────────────────

#[test]
fn compile_port_direction_preserved() {
    let source = r#"
trait MechPort {
    param d : Length
}

structure def S {
    port a : in MechPort {
        param d : Length = 1mm
    }
    port b : out MechPort {
        param d : Length = 2mm
    }
    port c : bidi MechPort {
        param d : Length = 3mm
    }
    port d : MechPort {
        param d : Length = 4mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(template.ports.len(), 4, "expected 4 ports");
    assert_eq!(template.ports[0].direction, PortDirection::In);
    assert_eq!(template.ports[1].direction, PortDirection::Out);
    assert_eq!(template.ports[2].direction, PortDirection::Bidi);
    assert_eq!(template.ports[3].direction, PortDirection::Bidi); // default
}

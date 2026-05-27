//! Port compilation tests.
//!
//! Tests for compiling port declarations into CompiledPort entries in TopologyTemplate.

use reify_compiler::*;
use reify_test_support::compile_first_template;
use reify_core::*;
use reify_ir::*;

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
    assert!(
        type_warnings.is_empty(),
        "unexpected type warnings: {:?}",
        type_warnings
    );
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
    assert_eq!(
        type_warnings.len(),
        1,
        "expected 1 unknown port type warning, got: {:?}",
        diagnostics
    );
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

// ── Step 21: content hash includes port identity fields ─────────────

#[test]
fn compile_port_content_hash_includes_identity() {
    // Two structures identical except for port name ('a' vs 'b')
    let source_a = r#"
trait MechPort { param d : Length }
structure def S1 {
    port a : MechPort { param d : Length = 5mm }
}
"#;
    let source_b = r#"
trait MechPort { param d : Length }
structure def S1 {
    port b : MechPort { param d : Length = 5mm }
}
"#;
    let (tmpl_a, _) = compile_first_template(source_a);
    let (tmpl_b, _) = compile_first_template(source_b);
    assert_ne!(
        tmpl_a.content_hash, tmpl_b.content_hash,
        "renaming a port must change content_hash"
    );
}

#[test]
fn compile_port_content_hash_includes_direction() {
    // Two structures identical except for port direction (in vs out)
    let source_in = r#"
trait MechPort { param d : Length }
structure def S1 {
    port x : in MechPort { param d : Length = 5mm }
}
"#;
    let source_out = r#"
trait MechPort { param d : Length }
structure def S1 {
    port x : out MechPort { param d : Length = 5mm }
}
"#;
    let (tmpl_in, _) = compile_first_template(source_in);
    let (tmpl_out, _) = compile_first_template(source_out);
    assert_ne!(
        tmpl_in.content_hash, tmpl_out.content_hash,
        "changing port direction must change content_hash"
    );
}

#[test]
fn compile_port_content_hash_includes_type_name() {
    // Two structures with different port type_name (MechPort vs RotaryPort)
    let source_mech = r#"
trait MechPort { param d : Length }
trait RotaryPort { param d : Length }
structure def S1 {
    port x : MechPort { param d : Length = 5mm }
}
"#;
    let source_rotary = r#"
trait MechPort { param d : Length }
trait RotaryPort { param d : Length }
structure def S1 {
    port x : RotaryPort { param d : Length = 5mm }
}
"#;
    let (tmpl_mech, _) = compile_first_template(source_mech);
    let (tmpl_rotary, _) = compile_first_template(source_rotary);
    assert_ne!(
        tmpl_mech.content_hash, tmpl_rotary.content_hash,
        "changing port type_name must change content_hash"
    );
}

// ── Step 23: content hash includes frame_expr ───────────────────────

#[test]
fn compile_port_content_hash_includes_frame_expr() {
    // Two structures: one port has frame = origin, the other has frame = offset
    let source_origin = r#"
trait MechPort { param d : Length }
structure def S1 {
    let origin = 0mm
    let offset = 1mm
    port x : MechPort {
        param d : Length = 5mm
        frame = origin
    }
}
"#;
    let source_offset = r#"
trait MechPort { param d : Length }
structure def S1 {
    let origin = 0mm
    let offset = 1mm
    port x : MechPort {
        param d : Length = 5mm
        frame = offset
    }
}
"#;
    let (tmpl_origin, _) = compile_first_template(source_origin);
    let (tmpl_offset, _) = compile_first_template(source_offset);
    assert_ne!(
        tmpl_origin.content_hash, tmpl_offset.content_hash,
        "changing port frame_expr must change content_hash"
    );
}

#[test]
fn compile_port_content_hash_frame_vs_no_frame() {
    // One port has a frame expression, the other doesn't
    let source_with_frame = r#"
trait MechPort { param d : Length }
structure def S1 {
    let origin = 0mm
    port x : MechPort {
        param d : Length = 5mm
        frame = origin
    }
}
"#;
    let source_no_frame = r#"
trait MechPort { param d : Length }
structure def S1 {
    let origin = 0mm
    port x : MechPort {
        param d : Length = 5mm
    }
}
"#;
    let (tmpl_with, _) = compile_first_template(source_with_frame);
    let (tmpl_without, _) = compile_first_template(source_no_frame);
    assert_ne!(
        tmpl_with.content_hash, tmpl_without.content_hash,
        "adding/removing frame_expr must change content_hash"
    );
}

// ── Step 25: duplicate port name error ──────────────────────────────

#[test]
fn compile_duplicate_port_name_error() {
    let source = r#"
trait MechPort { param d : Length }
trait RotaryPort { param r : Length }

structure def S {
    port mount : MechPort {
        param d : Length = 5mm
    }
    port mount : RotaryPort {
        param r : Length = 3mm
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    // Should have at least one error diagnostic about duplicate port name
    let dup_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("duplicate port"))
        .collect();
    assert!(
        !dup_errors.is_empty(),
        "expected error about duplicate port name, got diagnostics: {:?}",
        diagnostics
    );

    // Should only have 1 CompiledPort (the first one), not 2
    assert_eq!(
        template.ports.len(),
        1,
        "expected only 1 port (first occurrence), got {}",
        template.ports.len()
    );
    assert_eq!(template.ports[0].name, "mount");
    assert_eq!(template.ports[0].type_name, "MechPort");
}

//! Connect/chain compilation tests.
//!
//! Tests for compiling connect and chain declarations into CompiledConnection entries.

use reify_test_support::{
    assert_has_diagnostic, assert_no_diagnostic, compile_first_template, compile_source,
};
use reify_core::*;
use reify_ir::*;

// ── Step 13: compile_connect_generates_connection ────────────────────

#[test]
fn compile_connect_generates_connection() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Should have 1 connection
    assert_eq!(
        template.connections.len(),
        1,
        "expected 1 connection, got {}",
        template.connections.len()
    );
    assert_eq!(template.connections[0].left_port, "a");
    assert_eq!(template.connections[0].right_port, "b");
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Forward
    );

    // Should have a compatibility constraint
    let compat_id = &template.connections[0].compatibility_constraint;
    let has_compat = template.constraints.iter().any(|c| c.id == *compat_id);
    assert!(
        has_compat,
        "expected compatibility constraint for connection"
    );
}

// ── Step 15: compile_connect_with_connector ──────────────────────────

#[test]
fn compile_connect_with_connector() {
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 10.9 }
}
"#;

    let module = compile_source(source);
    // Get the S template (second one, after BoltSet)
    let s_template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let diagnostics = &module.diagnostics;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Should have 1 connection with connector_sub
    assert_eq!(s_template.connections.len(), 1);
    let conn = &s_template.connections[0];
    assert!(conn.connector_sub.is_some(), "expected connector_sub");
    let connector_name = conn.connector_sub.as_ref().unwrap();
    assert!(
        connector_name.starts_with("__connector_"),
        "expected __connector_ prefix, got {}",
        connector_name
    );

    // Should have a sub_component for the connector
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name);
    assert!(
        connector_sub.is_some(),
        "expected sub_component for connector"
    );
    let connector_sub = connector_sub.unwrap();
    assert_eq!(connector_sub.structure_name, "BoltSet");
}

// ── Step 17: compile_chain_desugars ─────────────────────────────────

#[test]
fn compile_chain_desugars() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 2mm }
    port c : in T { param d : Length = 3mm }
    chain a -> b -> c
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Chain a -> b -> c should desugar to 2 connections: a->b and b->c
    assert_eq!(
        template.connections.len(),
        2,
        "expected 2 connections, got {}",
        template.connections.len()
    );

    assert_eq!(template.connections[0].left_port, "a");
    assert_eq!(template.connections[0].right_port, "b");
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Forward
    );

    assert_eq!(template.connections[1].left_port, "b");
    assert_eq!(template.connections[1].right_port, "c");
    assert_eq!(
        template.connections[1].operator,
        reify_ast::ConnectOp::Forward
    );
}

// ── Step 19: compile_connect_direction_error ─────────────────────────

#[test]
fn compile_connect_direction_error() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 1mm }
    port b : in T { param d : Length = 2mm }
    connect a -> b
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // Should have an error about incompatible port directions (In -> In)
    let dir_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.message.contains("incompatible port directions")
        })
        .collect();
    assert!(
        !dir_errors.is_empty(),
        "expected error about incompatible port directions, got: {:?}",
        diagnostics
    );

    // Should still have 1 connection (even though it's invalid)
    assert_eq!(template.connections.len(), 1);
}

// ── Step 23: connector_sub_content_hash_includes_type_and_params ─────

#[test]
fn connector_sub_content_hash_includes_type_and_params() {
    // Two structures with connects between the same port pair but different connector types/params.
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def RivetSet { param grade : Real = 8.8 }
structure def S1 {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 8.8 }
}
structure def S2 {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : RivetSet { grade = 10.9 }
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s1 = module
        .templates
        .iter()
        .find(|t| t.name == "S1")
        .expect("S1");
    let s2 = module
        .templates
        .iter()
        .find(|t| t.name == "S2")
        .expect("S2");

    let s1_connector = s1
        .sub_components
        .iter()
        .find(|s| s.name.starts_with("__connector_"))
        .expect("S1 connector sub-component");
    let s2_connector = s2
        .sub_components
        .iter()
        .find(|s| s.name.starts_with("__connector_"))
        .expect("S2 connector sub-component");

    // Different connector types (BoltSet vs RivetSet) must produce different content hashes
    assert_ne!(
        s1_connector.content_hash, s2_connector.content_hash,
        "connector sub-component hashes should differ when connector types differ \
        (BoltSet vs RivetSet), but both are {:?}",
        s1_connector.content_hash
    );
}

// ── Step 25: template_content_hash_includes_connections ──────────────

#[test]
fn template_content_hash_includes_connections() {
    // Same structure name "S", same ports, only the connect target differs.
    // Compiled as separate modules so the name_hash is identical.
    let source1 = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    port c : in T { param d : Length = 5mm }
    connect a -> b
}
"#;
    let source2 = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    port c : in T { param d : Length = 5mm }
    connect a -> c
}
"#;

    let (t1, diag1) = compile_first_template(source1);
    let (t2, diag2) = compile_first_template(source2);

    let errors1: Vec<_> = diag1
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors2: Vec<_> = diag2
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors1.is_empty(), "unexpected errors: {:?}", errors1);
    assert!(errors2.is_empty(), "unexpected errors: {:?}", errors2);

    // Different connect targets must produce different template content hashes
    assert_ne!(
        t1.content_hash, t2.content_hash,
        "template hashes should differ when connect targets differ \
        (a->b vs a->c), but both are {:?}",
        t1.content_hash
    );
}

#[test]
fn template_content_hash_changes_with_operator() {
    // Same structure name "S", same bidi ports, only the operator differs.
    let source1 = r#"
trait T { param d : Length }
structure def S {
    port a : bidi T { param d : Length = 5mm }
    port b : bidi T { param d : Length = 5mm }
    connect a -> b
}
"#;
    let source2 = r#"
trait T { param d : Length }
structure def S {
    port a : bidi T { param d : Length = 5mm }
    port b : bidi T { param d : Length = 5mm }
    connect a <-> b
}
"#;

    let (t1, diag1) = compile_first_template(source1);
    let (t2, diag2) = compile_first_template(source2);

    let errors1: Vec<_> = diag1
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let errors2: Vec<_> = diag2
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors1.is_empty(), "unexpected errors: {:?}", errors1);
    assert!(errors2.is_empty(), "unexpected errors: {:?}", errors2);

    // Different operators must produce different template content hashes
    assert_ne!(
        t1.content_hash, t2.content_hash,
        "template hashes should differ when connect operators differ \
        (-> vs <->), but both are {:?}",
        t1.content_hash
    );
}

// ── compile_connect_reverse_ok ───────────────────────────────────────

#[test]
fn compile_connect_reverse_ok() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 5mm }
    port b : out T { param d : Length = 5mm }
    connect a <- b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Reverse
    );
}

// ── compile_connect_reverse_direction_error ──────────────────────────

#[test]
fn compile_connect_reverse_direction_error() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : out T { param d : Length = 5mm }
    connect a <- b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);
    let dir_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.message.contains("incompatible port directions")
        })
        .collect();
    assert!(
        !dir_errors.is_empty(),
        "expected direction error, got: {:?}",
        diagnostics
    );
}

// ── compile_connect_bidirectional_direction_error ────────────────────

#[test]
fn compile_connect_bidirectional_direction_error() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : bidi T { param d : Length = 5mm }
    connect a <-> b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);
    let dir_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("bidirectional connect"))
        .collect();
    assert!(
        !dir_errors.is_empty(),
        "expected bidirectional error, got: {:?}",
        diagnostics
    );
}

// ── compile_connect_port_mapping_propagation ─────────────────────────

#[test]
fn compile_connect_port_mapping_propagation() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { shaft -> input_bore }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("shaft".to_string(), "input_bore".to_string())]
    );
}

// ── compile_connect_unknown_port ─────────────────────────────────────

#[test]
fn compile_connect_unknown_port() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    connect a -> nonexistent
}
"#;
    let (_template, diagnostics) = compile_first_template(source);
    let undef_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("undefined port"))
        .collect();
    assert!(
        !undef_errors.is_empty(),
        "expected undefined port error, got: {:?}",
        diagnostics
    );
}

// ── connector_sub_hash_isolates_params ───────────────────────────────

#[test]
fn connector_sub_hash_isolates_params() {
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S1 {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 8.8 }
}
structure def S2 {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 10.9 }
}
"#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s1 = module
        .templates
        .iter()
        .find(|t| t.name == "S1")
        .expect("S1");
    let s2 = module
        .templates
        .iter()
        .find(|t| t.name == "S2")
        .expect("S2");

    let s1_conn = s1
        .sub_components
        .iter()
        .find(|s| s.name.starts_with("__connector_"))
        .expect("S1 connector");
    let s2_conn = s2
        .sub_components
        .iter()
        .find(|s| s.name.starts_with("__connector_"))
        .expect("S2 connector");

    assert_ne!(
        s1_conn.content_hash, s2_conn.content_hash,
        "same connector type with different param values must produce different hashes"
    );
}

// ── step-15: auto_match_chain_desugared ───────────────────────────────

#[test]
fn auto_match_chain_desugared() {
    // Chain a -> b -> c where all three ports have same trait T and matching param `d`.
    // Assert both desugared connections have auto-generated port_mappings [("d", "d")].
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 2mm }
    port c : in T { param d : Length = 3mm }
    chain a -> b -> c
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(
        template.connections.len(),
        2,
        "expected 2 desugared connections"
    );
    // Both connections should have auto-generated identity mapping for 'd'
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-mapping for first desugared connection (a->b)"
    );
    assert_eq!(
        template.connections[1].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-mapping for second desugared connection (b->c)"
    );
}

// ── step-13: auto_match_empty_members ────────────────────────────────

#[test]
fn auto_match_empty_members() {
    // Both ports have same trait but no params/auto members.
    // Assert port_mappings is empty and no diagnostic is emitted (vacuous match).
    let source = r#"
trait EmptyPort {}
structure def S {
    port a : out EmptyPort {}
    port b : in EmptyPort {}
    connect a -> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Empty members — vacuous match, port_mappings is empty
    assert_eq!(
        template.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings for ports with no members"
    );
    // No warnings
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings for empty-member ports, got: {:?}",
        warnings
    );
}

// ── step-11: no_auto_match_dotted_ports ───────────────────────────────

#[test]
fn no_auto_match_dotted_ports() {
    // Connect via dotted sub-component port refs: motor.shaft -> gear.input.
    // These are MemberAccess expressions that resolve to strings with '.'.
    // Auto-matching must be skipped — port_mappings should be empty.
    let source = r#"
trait RotaryPort { param d : Length }
structure def Motor {
    port shaft : out RotaryPort { param d : Length = 10mm }
}
structure def Gear {
    port input : in RotaryPort { param d : Length = 10mm }
}
structure def Assembly {
    sub motor = Motor()
    sub gear = Gear()
    connect motor.shaft -> gear.input
}
"#;
    let module = compile_source(source);
    let asm = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(asm.connections.len(), 1);
    assert_eq!(asm.connections[0].left_port, "motor.shaft");
    assert_eq!(asm.connections[0].right_port, "gear.input");
    // Dotted ports — auto-matching skipped, port_mappings stays empty
    assert_eq!(
        asm.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings for dotted port references"
    );
    // No unmatched-member warnings
    let unmatched_warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("unmatched"))
        .collect();
    assert!(
        unmatched_warnings.is_empty(),
        "expected no 'unmatched' warnings for dotted ports, got: {:?}",
        unmatched_warnings
    );
}

// ── step-9: explicit_mapping_skips_auto_match ─────────────────────────

#[test]
fn explicit_mapping_skips_auto_match() {
    // Same trait, same member names, but explicit mapping `{ d -> d }` provided.
    // Assert the explicit mapping is preserved and no auto-match logic runs.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { d -> d }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Explicit mapping should be preserved exactly as specified
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping to be preserved"
    );
    // No warnings about unmatched members
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings with explicit mapping, got: {:?}",
        warnings
    );
}

// ── step-7: no_auto_match_different_traits ────────────────────────────

#[test]
fn no_auto_match_different_traits() {
    // Left port has trait MechPort, right port has trait RotaryPort, both with param `d`.
    // Assert port_mappings is empty and no auto-match or unmatched diagnostic is emitted.
    let source = r#"
trait MechPort { param d : Length }
trait RotaryPort { param d : Length }
structure def S {
    port a : out MechPort { param d : Length = 5mm }
    port b : in RotaryPort { param d : Length = 5mm }
    connect a -> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    // No unmatched-member warnings
    let unmatched_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("unmatched"))
        .collect();
    assert!(
        unmatched_warnings.is_empty(),
        "expected no 'unmatched' warnings for different-trait ports, got: {:?}",
        unmatched_warnings
    );
    // port_mappings should be empty (no auto-match)
    assert_eq!(
        template.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings for different-trait ports"
    );
}

// ── step-5: auto_match_unmatched_emits_diagnostic ────────────────────

#[test]
fn auto_match_unmatched_emits_diagnostic() {
    // Same trait T, left port a has params {d, l}, right port b has params {d, r}.
    // Should emit a Warning diagnostic containing 'unmatched', and port_mappings stays empty.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T {
        param d : Length = 5mm
        param l : Length = 1mm
    }
    port b : in T {
        param d : Length = 5mm
        param r : Length = 1mm
    }
    connect a -> b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a Warning diagnostic for unmatched members, got: {:?}",
        diagnostics
    );
    let unmatched_warning = warnings.iter().any(|d| d.message.contains("unmatched"));
    assert!(
        unmatched_warning,
        "expected warning message to contain 'unmatched', got: {:?}",
        warnings
    );
    // port_mappings should be empty (no partial auto-match)
    assert_eq!(
        _template.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings when members don't fully match"
    );
}

// ── step-3: auto_match_multiple_members ──────────────────────────────

#[test]
fn auto_match_multiple_members() {
    // Both ports have same trait with 3 params (d, length, angle), no explicit mapping.
    // Assert all 3 are auto-mapped as identity pairs, sorted alphabetically.
    let source = r#"
trait MechPort {
    param d : Length
    param length : Length
    param angle : Real
}
structure def S {
    port a : out MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
        param angle : Real = 0.0
    }
    port b : in MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
        param angle : Real = 0.0
    }
    connect a -> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Auto-generated mappings should be sorted alphabetically: angle, d, length
    assert_eq!(
        template.connections[0].port_mappings,
        vec![
            ("angle".to_string(), "angle".to_string()),
            ("d".to_string(), "d".to_string()),
            ("length".to_string(), "length".to_string()),
        ],
        "expected 3 auto-generated identity mappings sorted alphabetically"
    );
}

// ── step-1: auto_match_ports_same_trait_same_members ─────────────────

#[test]
fn auto_match_ports_same_trait_same_members() {
    // Two ports of same trait T with identical param name `d`, no explicit mapping.
    // After auto-matching, port_mappings should contain [("d", "d")].
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-generated identity mapping for param 'd'"
    );
}

// ── task-246/step-5: compile_connect_mixed_params_and_mappings ───────

#[test]
fn compile_connect_mixed_params_and_mappings() {
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 10.9, shaft -> input_bore }
}
"#;

    let module = compile_source(source);
    let s_template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let diagnostics = &module.diagnostics;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(s_template.connections.len(), 1);
    let conn = &s_template.connections[0];

    // connector_sub should be present with __connector_ prefix
    assert!(conn.connector_sub.is_some(), "expected connector_sub");
    let connector_name = conn.connector_sub.as_ref().unwrap();
    assert!(
        connector_name.starts_with("__connector_"),
        "expected __connector_ prefix, got {}",
        connector_name
    );

    // sub_component for connector should have structure_name="BoltSet"
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name);
    assert!(
        connector_sub.is_some(),
        "expected sub_component for connector"
    );
    assert_eq!(connector_sub.unwrap().structure_name, "BoltSet");

    // port_mappings should be the explicit mapping
    assert_eq!(
        conn.port_mappings,
        vec![("shaft".to_string(), "input_bore".to_string())],
        "expected explicit port mapping shaft->input_bore"
    );
}

// ── task-247/step-1: auto_match_bidi_bidi_operator ───────────────────

#[test]
fn auto_match_bidi_bidi_operator() {
    // Two bidi ports of same trait connected via `<->`.
    // Auto-match should populate port_mappings with identity pairs and no diagnostics.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : bidi T { param d : Length = 5mm }
    port b : bidi T { param d : Length = 5mm }
    connect a <-> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Bidirectional
    );
    // Auto-match: same trait, same member `d` → identity mapping
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-generated identity mapping for bidi <-> bidi"
    );
    // No warnings
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

// ── task-247/step-2: auto_match_reverse_operator ─────────────────────

#[test]
fn auto_match_reverse_operator() {
    // Out->In reversed via `<-` (connect in <- out).
    // Auto-match is operator-agnostic: same trait, same member `d` → identity mapping.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 5mm }
    port b : out T { param d : Length = 5mm }
    connect a <- b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Reverse
    );
    // Auto-match still produces identity mapping regardless of operator
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-generated identity mapping for reverse operator"
    );
    // No warnings
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

// ── task-247/step-3: auto_match_chain_multi_members ──────────────────

#[test]
fn auto_match_chain_multi_members() {
    // Chain of three ports each with two matching members (d and length).
    // Both desugared connections should carry auto-generated identity mappings sorted alphabetically.
    let source = r#"
trait MechPort {
    param d : Length
    param length : Length
}
structure def S {
    port a : out MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
    }
    port b : bidi MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
    }
    port c : in MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
    }
    chain a -> b -> c
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(
        template.connections.len(),
        2,
        "expected 2 desugared connections"
    );
    // Both connections: sorted alphabetically → [("d","d"), ("length","length")]
    let expected = vec![
        ("d".to_string(), "d".to_string()),
        ("length".to_string(), "length".to_string()),
    ];
    assert_eq!(
        template.connections[0].port_mappings, expected,
        "first desugared connection (a->b) should have auto-mappings sorted alphabetically"
    );
    assert_eq!(
        template.connections[1].port_mappings, expected,
        "second desugared connection (b->c) should have auto-mappings sorted alphabetically"
    );
}

// ── task-247/step-4: explicit_mapping_multiple_pairs ─────────────────

#[test]
fn explicit_mapping_multiple_pairs() {
    // Explicit mapping with two pairs `{ d -> d, length -> length }`.
    // Both pairs should be preserved in source order with no diagnostics.
    let source = r#"
trait MechPort {
    param d : Length
    param length : Length
}
structure def S {
    port a : out MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
    }
    port b : in MechPort {
        param d : Length = 5mm
        param length : Length = 10mm
    }
    connect a -> b { d -> d, length -> length }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Both pairs preserved in source order
    assert_eq!(
        template.connections[0].port_mappings,
        vec![
            ("d".to_string(), "d".to_string()),
            ("length".to_string(), "length".to_string()),
        ],
        "expected two explicit mapping pairs preserved in source order"
    );
}

// ── task-247/step-5: explicit_mapping_reverse_operator ───────────────

#[test]
fn explicit_mapping_reverse_operator() {
    // Explicit mapping with `<-` reverse operator.
    // Mapping should be preserved as-is and ConnectOp::Reverse recorded.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 5mm }
    port b : out T { param d : Length = 5mm }
    connect a <- b { d -> d }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Reverse,
        "expected Reverse operator"
    );
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d preserved with reverse operator"
    );
}

// ── task-247/step-6: explicit_mapping_bidirectional_operator ─────────

#[test]
fn explicit_mapping_bidirectional_operator() {
    // Explicit mapping with `<->` between two bidi ports.
    // Mapping preserved and ConnectOp::Bidirectional recorded.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : bidi T { param d : Length = 5mm }
    port b : bidi T { param d : Length = 5mm }
    connect a <-> b { d -> d }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].operator,
        reify_ast::ConnectOp::Bidirectional,
        "expected Bidirectional operator"
    );
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d preserved with bidirectional operator"
    );
}

// ── task-247/step-7: mixed_multiple_params_and_mappings ──────────────

#[test]
fn mixed_multiple_params_and_mappings() {
    // Connector body with 2 params (grade, count) and 2 explicit mappings (d->d, length_m->length_m).
    // Verify connector sub receives both params and port_mappings holds both pairs.
    let source = r#"
trait T {
    param d : Length
    param length_m : Length
}
structure def BoltSet {
    param grade : Real = 8.8
    param count : Real = 4.0
}
structure def S {
    port a : out T {
        param d : Length = 5mm
        param length_m : Length = 10mm
    }
    port b : in T {
        param d : Length = 5mm
        param length_m : Length = 10mm
    }
    connect a -> b : BoltSet { grade = 10.9, count = 6.0, d -> d, length_m -> length_m }
}
"#;
    let module = compile_source(source);
    let s_template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let diagnostics = &module.diagnostics;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    assert_eq!(s_template.connections.len(), 1);
    let conn = &s_template.connections[0];

    // connector_sub should be present
    assert!(conn.connector_sub.is_some(), "expected connector_sub");
    let connector_name = conn.connector_sub.as_ref().unwrap();
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name)
        .expect("expected sub_component for connector");
    assert_eq!(connector_sub.structure_name, "BoltSet");

    // Both params should be in connector args
    assert_eq!(
        connector_sub.args.len(),
        2,
        "expected 2 connector params (grade and count)"
    );

    // Both explicit mappings preserved in source order
    assert_eq!(
        conn.port_mappings,
        vec![
            ("d".to_string(), "d".to_string()),
            ("length_m".to_string(), "length_m".to_string()),
        ],
        "expected both explicit mapping pairs preserved"
    );
}

// ── task-247/step-8: explicit_mapping_overrides_trait_mismatch ───────

#[test]
fn explicit_mapping_overrides_trait_mismatch() {
    // Two ports with different traits (MechPort vs RotaryPort) connected with
    // explicit `{ d -> d }` mapping. Verifies 'explicit always wins' semantics:
    // mapping is preserved, no warning about unmatched members, no error.
    let source = r#"
trait MechPort { param d : Length }
trait RotaryPort { param d : Length }
structure def S {
    port a : out MechPort { param d : Length = 5mm }
    port b : in RotaryPort { param d : Length = 5mm }
    connect a -> b { d -> d }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings with explicit mapping, got: {:?}",
        warnings
    );
    assert_eq!(template.connections.len(), 1);
    // Explicit mapping preserved, no auto-match interference
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d preserved for different-trait ports"
    );
}

// ── task-247/step-9: incomplete_mapping_parser_error ─────────────────

#[test]
fn incomplete_mapping_parser_error() {
    // Malformed `{ d -> }` in connect body produces a parse error.
    // Uses reify_syntax::parse directly (not compile_module) to inspect parse errors
    // without hitting compile_module's assert on parsed.errors.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { d -> }
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        !parsed.errors.is_empty(),
        "expected at least one parse error for malformed mapping"
    );
    // The malformed body causes the connect statement to be reported as invalid.
    // The error message is "invalid connect: ..." and includes the malformed input.
    assert!(
        parsed.errors.iter().any(|e| e.message.contains("d ->")),
        "expected error to include the malformed mapping text 'd ->', got: {:?}",
        parsed.errors
    );
}

// ── task-247/step-10: explicit_mapping_unknown_member_currently_accepted

#[test]
fn explicit_mapping_unknown_member_currently_accepted() {
    // Explicit mapping names members that do not exist on either port.
    // Documents current behavior: no validation of member names, mapping accepted verbatim.
    //
    // NOTE: This documents current (permissive) behavior. A future semantic-validation
    // pass may make this an error — at that point this test is the regression tripwire.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { ghost -> phantom }
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    // No error: mapping member names are not validated against port.members
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Mapping preserved verbatim, no substitution or rejection
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("ghost".to_string(), "phantom".to_string())],
        "expected explicit mapping preserved even for non-existent member names"
    );
}

// ── task-246/step-7: compile_connect_mixed_skips_auto_match ──────────

#[test]
fn compile_connect_mixed_skips_auto_match() {
    // Both ports have same trait T with matching param `d`.
    // Explicit mapping `d -> d` is provided in a mixed body with a param.
    // The explicit mapping should be used (not auto-match), and no warning diagnostics.
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 8.8, d -> d }
}
"#;

    let module = compile_source(source);
    let s_template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let diagnostics = &module.diagnostics;

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);

    assert_eq!(s_template.connections.len(), 1);
    let conn = &s_template.connections[0];

    // connector_sub should be present
    assert!(conn.connector_sub.is_some(), "expected connector_sub");

    // explicit port mapping should be used
    assert_eq!(
        conn.port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit port mapping d->d"
    );
}

// ── task-370/step-1: asymmetric_located_port_emits_warning ───────────

#[test]
fn asymmetric_located_port_emits_warning() {
    // MechPort : LocatedPort (satisfies LocatedPort transitively via refinement)
    // DataPort does NOT satisfy LocatedPort.
    // Connecting a MechPort to a DataPort is asymmetric — one side has a spatial
    // frame, the other does not. The compiler must emit a warning.
    let source = r#"
trait LocatedPort { param frame : Real }
trait MechPort : LocatedPort { param shaft_dia : Length }
trait DataPort { param rate : Real }
structure def S {
    port mech : out MechPort { param shaft_dia : Length = 10mm }
    port data : in DataPort { param rate : Real = 100.0 }
    connect mech -> data
}
"#;

    let (_, diagnostics) = compile_first_template(source);
    let located_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("LocatedPort")
                && d.message.contains("asymmetric")
        })
        .collect();
    assert!(
        !located_warnings.is_empty(),
        "expected a warning about asymmetric LocatedPort connection, got diagnostics: {:?}",
        diagnostics
    );
    assert_eq!(
        located_warnings.len(),
        1,
        "expected exactly one LocatedPort warning, got: {:?}",
        located_warnings
    );
}

// ── task-370/step-2: symmetric_located_port_no_warning ───────────────

#[test]
fn symmetric_located_port_no_warning() {
    // Both ports are MechPort (which satisfies LocatedPort).
    // A symmetric connection — no asymmetric LocatedPort warning should be emitted.
    let source = r#"
trait LocatedPort { param frame : Real }
trait MechPort : LocatedPort { param shaft_dia : Length }
structure def S {
    port a : out MechPort { param shaft_dia : Length = 10mm }
    port b : in MechPort { param shaft_dia : Length = 10mm }
    connect a -> b
}
"#;

    let (_, diagnostics) = compile_first_template(source);
    let located_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("LocatedPort"))
        .collect();
    assert!(
        located_warnings.is_empty(),
        "expected no LocatedPort warnings for symmetric connection, got: {:?}",
        located_warnings
    );
}

// ── task-370/step-3: neither_located_port_no_warning ─────────────────

#[test]
fn neither_located_port_no_warning() {
    // Both ports are DataPort — neither satisfies LocatedPort.
    // No asymmetric LocatedPort warning should be emitted.
    let source = r#"
trait LocatedPort { param frame : Real }
trait DataPort { param rate : Real }
structure def S {
    port a : out DataPort { param rate : Real = 1.0 }
    port b : in DataPort { param rate : Real = 1.0 }
    connect a -> b
}
"#;

    let (_, diagnostics) = compile_first_template(source);
    let located_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("LocatedPort"))
        .collect();
    assert!(
        located_warnings.is_empty(),
        "expected no LocatedPort warnings when neither port satisfies LocatedPort, got: {:?}",
        located_warnings
    );
}

// ── task-370/step-8: forward_ref_connector_type_accepted ─────────────

#[test]
fn forward_ref_connector_type_accepted() {
    // The connector type (ForwardConnector) is defined AFTER the structure that uses it.
    // Documents the design decision: connector_type is stored as a string in
    // SubComponentDecl without compile-time name resolution, so forward references
    // work naturally — no error should be produced.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : ForwardConnector { grade = 8.8 }
}
structure def ForwardConnector { param grade : Real = 8.8 }
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for forward-ref connector type, got: {:?}",
        errors
    );

    let s_template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");

    assert_eq!(s_template.connections.len(), 1);
    // connector_sub should reference ForwardConnector by name
    let conn = &s_template.connections[0];
    assert!(conn.connector_sub.is_some(), "expected connector_sub");
    let connector_name = conn.connector_sub.as_ref().unwrap();
    assert!(
        connector_name.starts_with("__connector_"),
        "expected __connector_ prefix, got {}",
        connector_name
    );
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name)
        .expect("expected sub_component for connector");
    assert_eq!(
        connector_sub.structure_name, "ForwardConnector",
        "expected structure_name to be ForwardConnector"
    );
}

// ── task-370/amend-5: asymmetric_located_port_right_side_emits_warning ──────

#[test]
fn asymmetric_located_port_right_side_emits_warning() {
    // Reversed direction: the RIGHT port (mech) satisfies LocatedPort, but the
    // LEFT port (data) does not.  The warning must fire regardless of which side
    // carries the spatial frame.
    let source = r#"
trait LocatedPort { param frame : Real }
trait MechPort : LocatedPort { param shaft_dia : Length }
trait DataPort { param rate : Real }
structure def S {
    port data : out DataPort { param rate : Real = 100.0 }
    port mech : in MechPort { param shaft_dia : Length = 10mm }
    connect data -> mech
}
"#;

    let (_, diagnostics) = compile_first_template(source);
    let located_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("LocatedPort")
                && d.message.contains("asymmetric")
        })
        .collect();
    assert!(
        !located_warnings.is_empty(),
        "expected a warning about asymmetric LocatedPort (right side located), got diagnostics: {:?}",
        diagnostics
    );
    assert_eq!(
        located_warnings.len(),
        1,
        "expected exactly one LocatedPort warning, got: {:?}",
        located_warnings
    );
}

// ── task-370/step-4: dotted_port_no_false_located_port_warning ───────

#[test]
fn dotted_port_no_false_located_port_warning() {
    // Sub-component ports connected via dotted syntax (motor.shaft -> recv.input).
    // These are dotted references that cannot be resolved to Assembly's port list.
    // The LocatedPort check must gracefully skip — no false warning should be emitted.
    let source = r#"
trait LocatedPort { param frame : Real }
trait MechPort : LocatedPort { param shaft_dia : Length }
trait DataPort { param rate : Real }
structure def Motor {
    port shaft : out MechPort { param shaft_dia : Length = 10mm }
}
structure def Receiver {
    port input : in DataPort { param rate : Real = 100.0 }
}
structure def Assembly {
    sub motor = Motor()
    sub recv = Receiver()
    connect motor.shaft -> recv.input
}
"#;

    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let located_warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("LocatedPort"))
        .collect();
    assert!(
        located_warnings.is_empty(),
        "expected no LocatedPort warning for dotted-port connections, got: {:?}",
        located_warnings
    );
}

// ── task-393/step-1: incompatible_directions_suppresses_auto_match_warning ──

#[test]
fn incompatible_directions_suppresses_auto_match_warning() {
    // Two In ports with same trait T but different members (a has {d,l}, b has {d,r}).
    // Forward connect (In->In) is direction-incompatible, which should suppress auto-matching.
    // Expected: direction-error IS emitted; unmatched-members Warning is NOT emitted.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T {
        param d : Length = 5mm
        param l : Length = 1mm
    }
    port b : in T {
        param d : Length = 5mm
        param r : Length = 1mm
    }
    connect a -> b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);

    assert_has_diagnostic(
        &diagnostics,
        Severity::Error,
        "incompatible port directions",
    );
    assert_no_diagnostic(&diagnostics, Severity::Warning, "do not match");
}

// ── task-393/step-3: incompatible_directions_reverse_suppresses_warning ──

#[test]
fn incompatible_directions_reverse_suppresses_warning() {
    // Two Out ports with same trait T but different members (a has {d,l}, b has {d,r}).
    // Reverse connect (Out <- Out, i.e. checking is_forward_compatible(Out, Out)) is
    // direction-incompatible. Auto-match should be skipped; no unmatched-members warning.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T {
        param d : Length = 5mm
        param l : Length = 1mm
    }
    port b : out T {
        param d : Length = 5mm
        param r : Length = 1mm
    }
    connect a <- b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);

    assert_has_diagnostic(
        &diagnostics,
        Severity::Error,
        "incompatible port directions",
    );
    assert_no_diagnostic(&diagnostics, Severity::Warning, "do not match");
}

// ── task-393/step-4: incompatible_bidi_suppresses_warning ────────────────

#[test]
fn incompatible_bidi_suppresses_warning() {
    // An In port and an Out port with same trait T but different members connected via <->.
    // Bidirectional requires both ports to be bidi, so this is direction-incompatible.
    // Auto-match should be skipped; no unmatched-members warning should be emitted.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T {
        param d : Length = 5mm
        param l : Length = 1mm
    }
    port b : out T {
        param d : Length = 5mm
        param r : Length = 1mm
    }
    connect a <-> b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);

    assert_has_diagnostic(
        &diagnostics,
        Severity::Error,
        "bidirectional connect requires both ports to be bidi",
    );
    assert_no_diagnostic(&diagnostics, Severity::Warning, "do not match");
}

// ── task-1838: hoisted_lookup split tests ────────────────────────────────────

/// Case (a): bare port found, direction compatible (Out→In) → auto-match runs
/// and produces identity mapping for param 'd'.
#[test]
fn hoisted_lookup_bare_found_auto_match() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    // Auto-match ran and produced identity mapping for param 'd'
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-generated identity mapping for param 'd'"
    );
}

/// Case (b): bare port not found → undefined-port error is emitted and names the port.
#[test]
fn hoisted_lookup_bare_not_found_undefined() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    connect a -> nonexistent
}
"#;
    let (_template, diagnostics) = compile_first_template(source);
    let undef_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("undefined port"))
        .collect();
    assert!(
        !undef_errors.is_empty(),
        "expected undefined-port error, got: {:?}",
        diagnostics
    );
    // The undefined port name is included in the error message
    let names_nonexistent = undef_errors
        .iter()
        .any(|d| d.message.contains("nonexistent"));
    assert!(
        names_nonexistent,
        "error message should name the undefined port, got: {:?}",
        undef_errors
    );
}

/// Case (c): dotted ports (motor.shaft → gear.input) → no undefined-port check,
/// no auto-match, empty port_mappings.
#[test]
fn hoisted_lookup_dotted_no_check() {
    let source = r#"
trait RotaryPort { param d : Length }
structure def Motor {
    port shaft : out RotaryPort { param d : Length = 10mm }
}
structure def Gear {
    port input : in RotaryPort { param d : Length = 10mm }
}
structure def Assembly {
    sub motor = Motor()
    sub gear = Gear()
    connect motor.shaft -> gear.input
}
"#;
    let module = compile_source(source);
    let asm = module
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(asm.connections.len(), 1, "expected 1 connection");
    assert_eq!(
        asm.connections[0].left_port, "motor.shaft",
        "expected dotted left_port"
    );
    assert_eq!(
        asm.connections[0].right_port, "gear.input",
        "expected dotted right_port"
    );
    // Dotted ports: no undefined-port error, no auto-match, empty port_mappings
    let undef_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("undefined port"))
        .collect();
    assert!(
        undef_errors.is_empty(),
        "expected NO undefined-port errors for dotted ports, got: {:?}",
        undef_errors
    );
    assert_eq!(
        asm.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings for dotted port references"
    );
}

/// Case (d): mixed — one bare+found ('a'), one dotted ('motor.shaft') → no auto-match.
/// When only one side is dotted, is_bare(&l) && is_bare(&r) is false, so auto-match
/// never runs even though the bare side resolved successfully. The dotted side is
/// also exempt from the undefined-port check because is_bare returns false for it.
#[test]
fn hoisted_lookup_mixed_bare_dotted() {
    let source = r#"
trait T { param d : Length }
structure def Motor {
    port shaft : in T { param d : Length = 5mm }
}
structure def Coupler {
    port a : out T { param d : Length = 5mm }
    sub motor = Motor()
    connect a -> motor.shaft
}
"#;
    let module = compile_source(source);
    let coupler = module
        .templates
        .iter()
        .find(|t| t.name == "Coupler")
        .expect("Coupler template");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(coupler.connections.len(), 1, "expected 1 connection");
    assert_eq!(
        coupler.connections[0].left_port, "a",
        "expected bare left_port"
    );
    assert_eq!(
        coupler.connections[0].right_port, "motor.shaft",
        "expected dotted right_port"
    );
    // Mixed bare+dotted: no auto-match runs, so port_mappings is empty
    assert_eq!(
        coupler.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings for mixed bare+dotted connect"
    );
}

/// Edge case: both ports are bare; left ('a') exists, right ('missing') does not.
/// Verifies: (1) undefined-port error is emitted for 'missing', (2) the connection
/// still appears (compile_connection does not early-return on undefined ports),
/// (3) port_mappings is empty because auto_match_port_members returns Vec::new()
/// when right_compiled is None, and (4) no "do not match" warning is emitted
/// (the unmatched-members path is never reached when one port is None).
#[test]
fn hoisted_lookup_bare_one_undefined_no_auto_match() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    connect a -> missing
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    // (1) undefined-port error emitted for 'missing'
    let undef_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("undefined port"))
        .collect();
    assert!(
        !undef_errors.is_empty(),
        "expected undefined-port error for 'missing', got: {:?}",
        diagnostics
    );
    // (2) connection still appears in template.connections
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    // (3) port_mappings is empty — auto_match short-circuits on None
    assert_eq!(
        template.connections[0].port_mappings,
        Vec::<(String, String)>::new(),
        "expected empty port_mappings when right port is undefined"
    );
    // (4) no "do not match" warning — unmatched-members path not reached
    assert_no_diagnostic(&diagnostics, Severity::Warning, "do not match");
}

// ── task-393/step-5: compatible_directions_still_emits_unmatched_warning ──

#[test]
fn compatible_directions_still_emits_unmatched_warning() {
    // Out->In (compatible directions) with same trait T but different members (a has {d,l}, b has {d,r}).
    // The direction check passes, so auto-match IS invoked and should emit the unmatched-members warning.
    // This regression test ensures the guard doesn't suppress the valid warning path.
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T {
        param d : Length = 5mm
        param l : Length = 1mm
    }
    port b : in T {
        param d : Length = 5mm
        param r : Length = 1mm
    }
    connect a -> b
}
"#;
    let (_template, diagnostics) = compile_first_template(source);

    assert_no_diagnostic(
        &diagnostics,
        Severity::Error,
        "incompatible port directions",
    );
    assert_has_diagnostic(&diagnostics, Severity::Warning, "do not match");
}

// ── task-1832/step-3: auto_match_with_prelooked_ports_same_result ────────────

/// Pinning test: verifies that compile_connection with pre-looked-up (hoisted) port
/// references produces exactly the same auto-match output as the original lookup-based
/// path. This pins expected behavior before the auto_match_port_members signature change.
///
/// Scenario: two bare ports of same trait with two matching params (radius, angle).
/// Compatible Out->In direction. Expected: identity mappings sorted alphabetically.
#[test]
fn auto_match_with_prelooked_ports_same_result() {
    let source = r#"
trait PipePort {
    param radius : Length
    param angle : Real
}
structure def S {
    port feed : out PipePort {
        param radius : Length = 12mm
        param angle : Real = 0.0
    }
    port inlet : in PipePort {
        param radius : Length = 12mm
        param angle : Real = 0.0
    }
    connect feed -> inlet
}
"#;
    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);

    // Auto-match should produce sorted identity mappings: angle, radius
    assert_eq!(
        template.connections[0].port_mappings,
        vec![
            ("angle".to_string(), "angle".to_string()),
            ("radius".to_string(), "radius".to_string()),
        ],
        "expected sorted identity mappings for PipePort members (angle, radius)"
    );

    // No warnings — ports match completely
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

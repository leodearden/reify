//! Connect/chain compilation tests.
//!
//! Tests for compiling connect and chain declarations into CompiledConnection entries.

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
    assert_eq!(template.connections.len(), 1, "expected 1 connection, got {}", template.connections.len());
    assert_eq!(template.connections[0].left_port, "a");
    assert_eq!(template.connections[0].right_port, "b");
    assert_eq!(template.connections[0].operator, reify_syntax::ConnectOp::Forward);

    // Should have a compatibility constraint
    let compat_id = &template.connections[0].compatibility_constraint;
    let has_compat = template.constraints.iter().any(|c| c.id == *compat_id);
    assert!(has_compat, "expected compatibility constraint for connection");
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

    let module = compile_module(source);
    // Get the S template (second one, after BoltSet)
    let s_template = module.templates.iter().find(|t| t.name == "S").expect("expected template S");
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
    assert!(connector_name.starts_with("__connector_"), "expected __connector_ prefix, got {}", connector_name);

    // Should have a sub_component for the connector
    let connector_sub = s_template.sub_components.iter().find(|s| s.name == *connector_name);
    assert!(connector_sub.is_some(), "expected sub_component for connector");
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
    assert_eq!(template.connections.len(), 2, "expected 2 connections, got {}", template.connections.len());

    assert_eq!(template.connections[0].left_port, "a");
    assert_eq!(template.connections[0].right_port, "b");
    assert_eq!(template.connections[0].operator, reify_syntax::ConnectOp::Forward);

    assert_eq!(template.connections[1].left_port, "b");
    assert_eq!(template.connections[1].right_port, "c");
    assert_eq!(template.connections[1].operator, reify_syntax::ConnectOp::Forward);
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
        .filter(|d| d.severity == Severity::Error && d.message.contains("incompatible port directions"))
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

    let module = compile_module(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s1 = module.templates.iter().find(|t| t.name == "S1").expect("S1");
    let s2 = module.templates.iter().find(|t| t.name == "S2").expect("S2");

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

    let errors1: Vec<_> = diag1.iter().filter(|d| d.severity == Severity::Error).collect();
    let errors2: Vec<_> = diag2.iter().filter(|d| d.severity == Severity::Error).collect();
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

    let errors1: Vec<_> = diag1.iter().filter(|d| d.severity == Severity::Error).collect();
    let errors2: Vec<_> = diag2.iter().filter(|d| d.severity == Severity::Error).collect();
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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(template.connections[0].operator, reify_syntax::ConnectOp::Reverse);
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
        .filter(|d| d.severity == Severity::Error && d.message.contains("incompatible port directions"))
        .collect();
    assert!(!dir_errors.is_empty(), "expected direction error, got: {:?}", diagnostics);
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
    assert!(!dir_errors.is_empty(), "expected bidirectional error, got: {:?}", diagnostics);
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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(template.connections[0].port_mappings, vec![("shaft".to_string(), "input_bore".to_string())]);
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
    assert!(!undef_errors.is_empty(), "expected undefined port error, got: {:?}", diagnostics);
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
    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s1 = module.templates.iter().find(|t| t.name == "S1").expect("S1");
    let s2 = module.templates.iter().find(|t| t.name == "S2").expect("S2");

    let s1_conn = s1.sub_components.iter().find(|s| s.name.starts_with("__connector_")).expect("S1 connector");
    let s2_conn = s2.sub_components.iter().find(|s| s.name.starts_with("__connector_")).expect("S2 connector");

    assert_ne!(
        s1_conn.content_hash, s2_conn.content_hash,
        "same connector type with different param values must produce different hashes"
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
    let module = compile_module(source);
    let asm = module.templates.iter().find(|t| t.name == "Assembly").expect("Assembly");
    let errors: Vec<_> = module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    // Explicit mapping should be preserved exactly as specified
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping to be preserved"
    );
    // No warnings about unmatched members
    let warnings: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Warning).collect();
    assert!(warnings.is_empty(), "expected no warnings with explicit mapping, got: {:?}", warnings);
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
    let warnings: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Warning).collect();
    assert!(
        !warnings.is_empty(),
        "expected a Warning diagnostic for unmatched members, got: {:?}",
        diagnostics
    );
    let unmatched_warning = warnings.iter().any(|d| d.message.contains("unmatched"));
    assert!(unmatched_warning, "expected warning message to contain 'unmatched', got: {:?}", warnings);
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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
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
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected auto-generated identity mapping for param 'd'"
    );
}

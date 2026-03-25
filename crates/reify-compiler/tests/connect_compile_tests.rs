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

// ── step-15: content_hash_includes_frame_constraint ──────────────────

#[test]
fn content_hash_includes_frame_constraint() {
    // S1 uses LocatedPort (frame_constraint = Some), S2 uses plain Port (frame_constraint = None)
    let source1 = r#"
trait Port {}
trait LocatedPort : Port { param frame : Length }
structure def Connector {}
structure def S {
    port a : out LocatedPort { param frame : Length = 0mm }
    port b : in LocatedPort { param frame : Length = 0mm }
    connect a -> b : Connector {}
}
"#;
    let source2 = r#"
trait Port {}
structure def Connector {}
structure def S {
    port a : out Port {}
    port b : in Port {}
    connect a -> b : Connector {}
}
"#;

    let module1 = compile_module(source1);
    let module2 = compile_module(source2);

    let errors1: Vec<_> = module1.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    let errors2: Vec<_> = module2.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors1.is_empty(), "unexpected errors in source1: {:?}", errors1);
    assert!(errors2.is_empty(), "unexpected errors in source2: {:?}", errors2);

    let s1 = module1.templates.iter().find(|t| t.name == "S").expect("S in source1");
    let s2 = module2.templates.iter().find(|t| t.name == "S").expect("S in source2");

    // S1 has frame_constraint Some, S2 has None — content_hashes must differ
    assert!(
        s1.connections[0].frame_constraint.is_some(),
        "S1 should have frame_constraint Some (LocatedPort)"
    );
    assert!(
        s2.connections[0].frame_constraint.is_none(),
        "S2 should have frame_constraint None (plain Port)"
    );
    assert_ne!(
        s1.content_hash, s2.content_hash,
        "template content_hashes should differ when frame_constraint presence differs"
    );
}

// ── step-7: port_type_compatibility_mismatch_warning ─────────────────

#[test]
fn port_type_compatibility_mismatch_warning() {
    let source = r#"
trait TraitA {}
trait TraitB {}
structure def S {
    port a : out TraitA {}
    port b : in TraitB {}
    connect a -> b
}
"#;

    let (_template, diagnostics) = compile_first_template(source);

    // Direction is compatible (Out -> In), so no direction error
    let dir_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("incompatible port directions"))
        .collect();
    assert!(dir_errors.is_empty(), "unexpected direction errors: {:?}", dir_errors);

    // Should emit a warning about incompatible port types
    let type_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning
            && (d.message.contains("incompatible port types") || d.message.contains("port type mismatch")))
        .collect();
    assert!(
        !type_warnings.is_empty(),
        "expected a warning about incompatible port types, diagnostics: {:?}",
        diagnostics
    );
}

// ── step-11: connector_param_validation_unknown_param ────────────────

#[test]
fn connector_param_validation_unknown_param() {
    let source = r#"
trait T {}
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T {}
    port b : in T {}
    connect a -> b : BoltSet { unknown_param = 5.0 }
}
"#;

    let module = compile_module(source);

    // Should have an error about unknown_param
    let unknown_errors: Vec<_> = module.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error
            && d.message.contains("unknown")
            && d.message.contains("unknown_param"))
        .collect();
    assert!(
        !unknown_errors.is_empty(),
        "expected error about unknown connector param 'unknown_param', diagnostics: {:?}",
        module.diagnostics
    );
}

// ── step-13: connector_param_validation_valid_params_ok ──────────────

#[test]
fn connector_param_validation_valid_params_ok() {
    let source = r#"
trait T {}
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T {}
    port b : in T {}
    connect a -> b : BoltSet { grade = 10.9 }
}
"#;

    let module = compile_module(source);

    // No error for valid param 'grade' which is declared in BoltSet
    let errors: Vec<_> = module.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors for valid connector param: {:?}", errors);
}

// ── step-9: port_type_compatibility_refinement_ok ────────────────────

#[test]
fn port_type_compatibility_refinement_ok() {
    let source = r#"
trait TraitA {}
trait TraitB : TraitA {}
structure def S {
    port a : out TraitA {}
    port b : in TraitB {}
    connect a -> b
}
"#;

    let (_template, diagnostics) = compile_first_template(source);

    // No type compatibility warning when TraitB refines TraitA
    let type_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning
            && (d.message.contains("incompatible port types") || d.message.contains("port type mismatch")))
        .collect();
    assert!(
        type_warnings.is_empty(),
        "unexpected type compatibility warnings: {:?}",
        type_warnings
    );
}

// ── step-3: frame_alignment_none_when_ports_not_located ──────────────

#[test]
fn frame_alignment_none_when_ports_not_located() {
    let source = r#"
trait T {}
structure def S {
    port a : out T {}
    port b : in T {}
    connect a -> b
}
"#;

    let (template, diagnostics) = compile_first_template(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // frame_constraint should be None when ports do not satisfy LocatedPort
    assert!(
        template.connections[0].frame_constraint.is_none(),
        "expected frame_constraint to be None when ports are not LocatedPort"
    );

    // No constraint with frame_align label should exist
    let frame_align_constraints: Vec<_> = template.constraints
        .iter()
        .filter(|c| c.label.as_deref().map(|l| l.contains("frame_align")).unwrap_or(false))
        .collect();
    assert!(
        frame_align_constraints.is_empty(),
        "expected no frame_align constraints, but found: {:?}",
        frame_align_constraints.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// ── step-5: frame_alignment_via_refinement_chain ─────────────────────

#[test]
fn frame_alignment_via_refinement_chain() {
    let source = r#"
trait Port {}
trait LocatedPort : Port { param frame : Length }
trait MechanicalPort : LocatedPort { param max_load : Real }
structure def Connector {}
structure def S {
    port a : out MechanicalPort { param frame : Length = 0mm  param max_load : Real = 100.0 }
    port b : in MechanicalPort { param frame : Length = 0mm  param max_load : Real = 100.0 }
    connect a -> b : Connector {}
}
"#;

    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module.templates.iter().find(|t| t.name == "S").expect("template S");

    // MechanicalPort transitively refines LocatedPort, so frame_constraint should be Some
    assert!(
        s.connections[0].frame_constraint.is_some(),
        "expected frame_constraint to be Some when both ports are MechanicalPort (refines LocatedPort)"
    );

    let frame_align_constraints: Vec<_> = s.constraints
        .iter()
        .filter(|c| c.label.as_deref().map(|l| l.contains("frame_align")).unwrap_or(false))
        .collect();
    assert!(
        !frame_align_constraints.is_empty(),
        "expected a frame_align constraint for MechanicalPort (transitive refinement)"
    );
}

// ── step-1: frame_alignment_constraint_when_both_ports_located ───────

#[test]
fn frame_alignment_constraint_when_both_ports_located() {
    let source = r#"
trait Port {}
trait LocatedPort : Port { param frame : Length }
structure def Connector {}
structure def S {
    port a : out LocatedPort { param frame : Length = 0mm }
    port b : in LocatedPort { param frame : Length = 0mm }
    connect a -> b : Connector {}
}
"#;

    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module.templates.iter().find(|t| t.name == "S").expect("template S");

    // frame_constraint should be Some when both ports satisfy LocatedPort
    assert!(
        s.connections[0].frame_constraint.is_some(),
        "expected frame_constraint to be Some when both ports are LocatedPort"
    );

    // There should be a constraint labeled frame_align_a_b (or frame_align_* pattern)
    let frame_align_constraints: Vec<_> = s.constraints
        .iter()
        .filter(|c| c.label.as_deref().map(|l| l.contains("frame_align")).unwrap_or(false))
        .collect();
    assert!(
        !frame_align_constraints.is_empty(),
        "expected a constraint with label containing 'frame_align', constraints: {:?}",
        s.constraints.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// ── step-1: connector_with_params_and_port_mappings ───────────────────

#[test]
fn connector_with_params_and_port_mappings() {
    // A connect statement that has BOTH connector params AND port mappings.
    // Syntax: `connect a -> b : BoltSet { grade = 10.9  shaft -> bore }`
    let source = r#"
trait T {}
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T {}
    port b : in T {}
    connect a -> b : BoltSet { grade = 10.9  shaft -> bore }
}
"#;

    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module.templates.iter().find(|t| t.name == "S").expect("template S");

    // Should have exactly 1 connection
    assert_eq!(s.connections.len(), 1);
    let conn = &s.connections[0];

    // Should have a connector_sub pointing to __connector_0
    assert!(conn.connector_sub.is_some(), "expected connector_sub to be Some");
    let connector_name = conn.connector_sub.as_ref().unwrap();
    assert!(
        connector_name.starts_with("__connector_"),
        "expected __connector_ prefix, got {}",
        connector_name
    );

    // Port mappings should be propagated
    assert_eq!(
        conn.port_mappings,
        vec![("shaft".to_string(), "bore".to_string())],
        "expected port_mappings to contain (shaft, bore)"
    );

    // The connector sub-component should exist with structure_name BoltSet and have the grade arg
    let sub = s.sub_components.iter().find(|s| s.name == *connector_name)
        .expect("connector sub-component");
    assert_eq!(sub.structure_name, "BoltSet");
    let grade_arg = sub.args.iter().find(|(name, _)| name == "grade");
    assert!(grade_arg.is_some(), "expected 'grade' arg in connector sub-component");
}

// ── step-3: multiple_connectors_per_entity ────────────────────────────

#[test]
fn multiple_connectors_per_entity() {
    // Two connect statements with connectors in the same structure.
    // Verifies that __connector_0 and __connector_1 are created with distinct names.
    let source = r#"
trait T {}
structure def BoltSet { param grade : Real = 8.8 }
structure def RivetSet { param diameter : Real = 6.0 }
structure def S {
    port a : out T {}
    port b : in T {}
    port c : out T {}
    port d : in T {}
    connect a -> b : BoltSet { grade = 10.9 }
    connect c -> d : RivetSet { diameter = 8.0 }
}
"#;

    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module.templates.iter().find(|t| t.name == "S").expect("template S");

    // Should have exactly 2 connections
    assert_eq!(s.connections.len(), 2);

    // Both connections should have connector_sub set
    let c0_name = s.connections[0].connector_sub.as_ref()
        .expect("expected connector_sub for connection 0");
    let c1_name = s.connections[1].connector_sub.as_ref()
        .expect("expected connector_sub for connection 1");

    // Names must be __connector_0 and __connector_1
    assert_eq!(c0_name, "__connector_0", "first connector should be __connector_0");
    assert_eq!(c1_name, "__connector_1", "second connector should be __connector_1");

    // Sub-components for both connectors must exist
    let sub0 = s.sub_components.iter().find(|s| s.name == "__connector_0")
        .expect("__connector_0 sub-component");
    let sub1 = s.sub_components.iter().find(|s| s.name == "__connector_1")
        .expect("__connector_1 sub-component");

    assert_eq!(sub0.structure_name, "BoltSet",  "connector_0 should be BoltSet");
    assert_eq!(sub1.structure_name, "RivetSet", "connector_1 should be RivetSet");
}

// ── step-5: connector_args_reference_parent_params ────────────────────

#[test]
fn connector_args_reference_parent_params() {
    // Connector parameter expression references parent structure's param 'g'.
    // The compiled arg should contain a ValueRef to entity S, member g.
    let source = r#"
trait T {}
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    param g : Real = 8.8
    port a : out T {}
    port b : in T {}
    connect a -> b : BoltSet { grade = g }
}
"#;

    let module = compile_module(source);
    let errors: Vec<_> = module.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let s = module.templates.iter().find(|t| t.name == "S").expect("template S");

    assert_eq!(s.connections.len(), 1);
    let conn = &s.connections[0];

    let connector_name = conn.connector_sub.as_ref()
        .expect("expected connector_sub");
    let sub = s.sub_components.iter().find(|sc| sc.name == *connector_name)
        .expect("connector sub-component");
    assert_eq!(sub.structure_name, "BoltSet");

    // Find the 'grade' arg
    let (_, grade_expr) = sub.args.iter().find(|(name, _)| name == "grade")
        .expect("expected 'grade' arg in connector sub-component");

    // The expression should be a ValueRef (referencing the parent param 'g')
    let has_value_ref = matches!(
        &grade_expr.kind,
        CompiledExprKind::ValueRef(id) if id.entity == "S" && id.member == "g"
    );
    assert!(
        has_value_ref,
        "expected grade expr to be a ValueRef to S.g, got: {:?}",
        grade_expr.kind
    );
}

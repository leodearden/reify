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

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

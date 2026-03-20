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

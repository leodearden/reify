//! Compiler behavior for ad-hoc selector (@) expressions.
//!
//! Verifies that the compiler emits a Diagnostic::error instead of panicking
//! when it encounters an AdHocSelector expression.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule without
/// asserting on compile errors. Used to inspect diagnostics directly.
fn compile_module_with_diagnostics(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

#[test]
fn compile_ad_hoc_selector_emits_diagnostic() {
    // The compiler does not yet implement ad-hoc selector (@) support.
    // It should emit a Severity::Error diagnostic rather than panicking.
    let source = r#"
structure S {
    let x = port @ face("top")
}
"#;

    // This should NOT panic — it should return with a diagnostic.
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected compile error for ad-hoc selector, but got none"
    );
    let has_selector_error = errors.iter().any(|d| {
        d.message
            .contains("ad-hoc selector (@) is not yet supported")
    });
    assert!(
        has_selector_error,
        "expected diagnostic about unsupported ad-hoc selector, got: {:?}",
        errors
    );
}

// ── task-251 ad-hoc port selectors ─────────────────────────────────────────────
//
// All tests below are TDD-red specs: they describe the DESIRED behavior once
// Tasks 249/250 are re-implemented. They intentionally fail against the current
// stub (which emits "ad-hoc selector (@) is not yet supported" for all
// AdHocSelector nodes). Tracked via escalation esc-251-20.

/// Compile a structure with `port p : out T` and `let resolved = p @ face("top")`.
/// After implementation, the compiler should accept @face with a string-literal
/// argument and emit ZERO Severity::Error diagnostics.
/// Behavior covered: @face with named face (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_face_with_named_string_arg_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let resolved = p @ face("top")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for @face with string arg, got: {:?}",
        errors
    );
}

/// Compile a structure with `let resolved = p @ point(10mm, 20mm, 0mm)`.
/// After implementation, the compiler should accept @point with three
/// numeric-with-unit coordinate arguments and emit ZERO Severity::Error diagnostics.
/// Behavior covered: @point with coordinates (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_point_with_three_coordinate_args_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let resolved = p @ point(10mm, 20mm, 0mm)
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for @point with three coords, got: {:?}",
        errors
    );
}

/// Compile a structure with `let e = p @ edge("left")`.
/// After implementation, @edge with a string-literal argument should be
/// accepted at compile time with ZERO Severity::Error diagnostics.
/// Behavior covered: @edge (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_edge_with_named_string_arg_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let e = p @ edge("left")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for @edge with string arg, got: {:?}",
        errors
    );
}

/// Compile a structure with an unknown selector name `@ bogus("arg")`.
/// After implementation, the compiler should emit at least one Severity::Error
/// diagnostic with a message mentioning the unknown selector kind.
/// Behavior covered: unknown selector error (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_unknown_selector_kind_emits_error_diagnostic() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ bogus("arg")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error diagnostic for unknown selector 'bogus'"
    );
    let has_unknown_selector_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("unknown selector")
            || msg.contains("unknown @")
            || msg.contains("bogus")
            || msg.contains("unsupported selector")
    });
    assert!(
        has_unknown_selector_error,
        "expected diagnostic mentioning unknown selector 'bogus', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Compile a `connect a @ face("top") -> b @ face("bottom")` statement.
/// After implementation:
///   (a) The module should have ZERO Severity::Error diagnostics.
///   (b) template.connections should contain exactly one entry.
///   (c) That connection's `frame_constraint` field should be `Some(_)`.
///   (d) The referenced frame_constraint id should exist in template.constraints.
/// Behavior covered: connect with ad-hoc ports generates frame constraints.
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_connect_with_ad_hoc_ports_both_sides_creates_connection_and_frame_constraint() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a @ face("top") -> b @ face("bottom")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for connect with ad-hoc ports, got: {:?}",
        errors
    );
    let template = &module.templates[0];
    assert_eq!(template.connections.len(), 1, "expected exactly 1 connection");
    let conn = &template.connections[0];
    assert!(
        conn.frame_constraint.is_some(),
        "expected frame_constraint to be Some(_) for ad-hoc port connect"
    );
    let frame_id = conn.frame_constraint.as_ref().unwrap();
    assert!(
        template.constraints.iter().any(|c| &c.id == frame_id),
        "frame_constraint id {:?} should appear in template.constraints",
        frame_id
    );
}

/// Compile a plain logical structure (no geometry let-binding) with @face.
/// After implementation, the compiler should detect the absence of a geometry
/// expression and emit at least one Severity::Error diagnostic mentioning
/// missing/unavailable geometry.
/// Behavior covered: @face on entity without geometry (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_face_on_entity_without_geometry_emits_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ face("top")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for @face on entity without geometry"
    );
    let has_geometry_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("no geometry")
            || msg.contains("without geometry")
            || msg.contains("geometry kernel")
            || msg.contains("geometry")
            || msg.contains("realize")
    });
    assert!(
        has_geometry_error,
        "expected diagnostic mentioning missing geometry, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Compile a structure with a forall quantifier whose predicate uses @face
/// on each element of a collection sub-component.
/// After implementation, the compiler should accept ad-hoc selectors inside
/// forall predicates with ZERO Severity::Error diagnostics.
/// Behavior covered: ad-hoc port in forall quantifier (compile path).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_ad_hoc_port_inside_forall_predicate_no_error() {
    let source = r#"
trait T { param d : Length }
structure def Part {
    port p : out T { param d : Length = 5mm }
}
structure def S {
    sub parts : List<Part>
    constraint forall p in parts: p.p @ face("mount") != p.p @ face("side")
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for @face inside forall predicate, got: {:?}",
        errors
    );
}

/// Compile `p @ face(42)` — integer literal instead of string for the face name.
/// After implementation, the compiler should emit at least one Severity::Error
/// diagnostic indicating a type mismatch (expected string, got integer).
/// Behavior covered: selector argument type checking.
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_face_arg_non_string_type_emits_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ face(42)
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for @face with non-string argument"
    );
    let has_type_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("type")
            || msg.contains("string")
            || msg.contains("expected")
            || msg.contains("face")
    });
    assert!(
        has_type_error,
        "expected diagnostic about argument type for @face(42), got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Compile `p @ edge(width * 2)` — expression argument (not a literal).
/// After implementation, expression arguments should compile through without
/// error, just like the parser test `parse_ad_hoc_selector_with_expr_args`.
/// Behavior covered: selector argument compilation (expression form).
#[test]
#[ignore = "blocked on Task 249/250 re-implementation; see esc-251-20"]
fn compile_ad_hoc_selector_accepts_expression_args_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    param width : Length = 10mm
    port p : out T { param d : Length = 5mm }
    let e = p @ edge(width * 2)
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for @edge with expression arg, got: {:?}",
        errors
    );
}

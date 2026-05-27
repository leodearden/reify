//! Compiler behavior for ad-hoc selector (@) expressions.

use reify_test_support::compile_source;
use reify_core::*;
use reify_ir::*;

#[test]
fn compile_ad_hoc_selector_on_undefined_name_emits_error() {
    // `port` is not a declared port — should produce an unresolved-name error,
    // NOT the old "ad-hoc selector (@) is not yet supported" stub message.
    let source = r#"
structure S {
    let x = port @ face("top")
}
"#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected compile error for unresolved 'port', but got none"
    );
    // The old stub message should no longer appear
    let has_old_stub = errors.iter().any(|d| {
        d.message
            .contains("ad-hoc selector (@) is not yet supported")
    });
    assert!(
        !has_old_stub,
        "old stub error 'not yet supported' should no longer be emitted"
    );
}

// ── ad-hoc port selector compilation ──────────────────────────────────────────

/// Compile a structure with geometry and `let resolved = p @ face("top")`.
/// @face with a string-literal argument should compile with ZERO errors.
#[test]
fn compile_face_with_named_string_arg_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    let shape = box(10mm, 10mm, 10mm)
    port p : out T { param d : Length = 5mm }
    let resolved = p @ face("top")
}
"#;
    let module = compile_source(source);
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
/// @point with three numeric coordinates should compile with ZERO errors.
/// No geometry declaration required for @point.
#[test]
fn compile_point_with_three_coordinate_args_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let resolved = p @ point(10mm, 20mm, 0mm)
}
"#;
    let module = compile_source(source);
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

/// Compile a structure with geometry and `let e = p @ edge("left")`.
/// @edge with a string-literal argument should compile with ZERO errors.
#[test]
fn compile_edge_with_named_string_arg_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    let shape = box(10mm, 10mm, 10mm)
    port p : out T { param d : Length = 5mm }
    let e = p @ edge("left")
}
"#;
    let module = compile_source(source);
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
/// Should emit at least one error mentioning the unknown selector kind.
#[test]
fn compile_unknown_selector_kind_emits_error_diagnostic() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ bogus("arg")
}
"#;
    let module = compile_source(source);
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
#[test]
fn compile_connect_with_ad_hoc_ports_both_sides_creates_connection_and_frame_constraint() {
    let source = r#"
trait T { param d : Length }
structure def S {
    let shape = box(10mm, 10mm, 10mm)
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a @ face("top") -> b @ face("bottom")
}
"#;
    let module = compile_source(source);
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
    assert_eq!(
        template.connections.len(),
        1,
        "expected exactly 1 connection"
    );
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

/// Compile a plain logical structure (no geometry let-binding) with @face on a
/// direct port. The compiler should detect the absence of geometry and emit an error.
#[test]
fn compile_face_on_entity_without_geometry_emits_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ face("top")
}
"#;
    let module = compile_source(source);
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
        msg.contains("geometry")
    });
    assert!(
        has_geometry_error,
        "expected diagnostic mentioning missing geometry, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Compile a structure with a forall quantifier whose predicate uses @face
/// on each element of a collection sub-component. The base is a member-access
/// (p.p), not a direct port — geometry check is deferred to eval time.
#[test]
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
    let module = compile_source(source);
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
/// Should emit at least one error about the argument type.
#[test]
fn compile_face_arg_non_string_type_emits_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ face(42)
}
"#;
    let module = compile_source(source);
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
        msg.contains("string") || msg.contains("expected") || msg.contains("face")
    });
    assert!(
        has_type_error,
        "expected diagnostic about argument type for @face(42), got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Compile `p @ face("a", "b")` — two arguments instead of one.
/// Asserts: (a) an error whose message contains the @face custom text, and
/// (b) that error has a label with the canonical "wrong number of arguments" text.
#[test]
fn compile_face_wrong_arg_count_uses_canonical_label() {
    let source = r#"
trait T { param d : Length }
structure S {
    let shape = box(10mm, 10mm, 10mm)
    port p : out T { param d : Length = 5mm }
    let r = p @ face("a", "b")
}
"#;
    let module = compile_source(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for @face with wrong arg count"
    );
    let arg_count_error = errors
        .iter()
        .find(|d| d.message.contains("@face expects exactly 1 argument"));
    assert!(
        arg_count_error.is_some(),
        "expected error containing '@face expects exactly 1 argument', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let diag = arg_count_error.unwrap();
    let has_canonical_label = diag
        .labels
        .iter()
        .any(|l| l.message == "wrong number of arguments");
    assert!(
        has_canonical_label,
        "expected label 'wrong number of arguments' on @face arg-count error, got labels: {:?}",
        diag.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
}

/// Compile `p @ edge(width * 2)` — expression argument (not a literal).
/// Expression arguments should compile through without error.
#[test]
fn compile_ad_hoc_selector_accepts_expression_args_no_error() {
    let source = r#"
trait T { param d : Length }
structure S {
    let shape = box(10mm, 10mm, 10mm)
    param width : Length = 10mm
    port p : out T { param d : Length = 5mm }
    let e = p @ edge(width * 2)
}
"#;
    let module = compile_source(source);
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

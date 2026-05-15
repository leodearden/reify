//! Parser-level tests for `sub name : StructName { body }` specialization-scope body (task 3569).
//!
//! User-observable signal: `cargo test -p reify-syntax -- sub_decl_specialization_body_parses_from_source`
//! Leaf signal from phase-3-grammar-fiction-triage-log.md §B3.
//!
//! These tests verify that the tree-sitter grammar admits the new specialization-body
//! production and produces a well-formed CST (zero ERROR nodes). Both permitted and
//! forbidden body member kinds must parse — rejection of forbidden kinds is deferred
//! to the downstream validator (task 3571/3573).
//! AST-shape assertions (lowering to `SubDecl.body: Some(...)`) are deferred to
//! sibling task 3571.

use reify_types::ModulePath;

mod common;
use common::{find_cst_node, make_ts_parser};

// ── High-level parse tests (user-observable signal) ─────────────────────────

/// User-signal test: all forms of specialization-body sub declarations parse
/// without errors. Covers permitted-only body, where-guarded param_assignment,
/// where-guard-before-body, and forbidden-but-must-parse bodies.
///
/// `!tree.root_node().has_error()` is the primary signal — it confirms the new
/// grammar production is reachable from `_member` with no CST ERROR nodes.
///
/// `module.errors.is_empty()` is a regression guard for the *surrounding*
/// source (the `structure S { ... }` wrapper): it will fire if an unrelated
/// change breaks the surrounding structure parse, but carries no direct signal
/// about the `specialization_body` construct itself, because lowering for that
/// construct stays `body: None` until sibling task 3571 wires CST→AST.
#[test]
fn sub_decl_specialization_body_parses_from_source() {
    let sources: &[(&str, &str)] = &[
        (
            "permitted-only body (param_assignment + let + constraint + connect)",
            "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm  let m = shaft_diameter * 2  constraint shaft_diameter > 1mm  connect a -> b } }",
        ),
        (
            "where-guarded parameter assignment",
            "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm where high_torque } }",
        ),
        (
            "where-guard on sub before body",
            "structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }",
        ),
        (
            "forbidden body — param member (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { param x : Length } }",
        ),
        (
            "forbidden body — port member (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { port p : MechanicalPort } }",
        ),
        (
            "forbidden body — nested bodyless sub (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { sub child : Foo } }",
        ),
        (
            "bare colon, no body — new grammar branch, no body child expected",
            "structure S { sub a : Foo }",
        ),
        (
            "type_args + guard + body — exercises the full optional chain together",
            "structure S { sub m : Gear<Steel> where n > 0 { ratio = 2 } }",
        ),
    ];

    for (label, source) in sources {
        // Regression guard: asserts the surrounding `structure S { ... }` source
        // still parses cleanly. Does NOT verify specialization_body lowering —
        // lower_sub returns body: None until sibling task 3571 wires CST→AST.
        let module = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "expected zero parse errors for {label:?} form, got: {:?}",
            module.errors,
        );

        // CST-level check: no ERROR nodes in the parse tree.
        let mut parser = make_ts_parser();
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("tree-sitter parse failed");
        assert!(
            !tree.root_node().has_error(),
            "expected no CST ERROR nodes for {label:?} form; \
             has_error() returned true for source: {source:?}",
        );
    }
}

// ── CST-shape assertions ─────────────────────────────────────────────────────

/// A `sub motor : ElectricMotor { shaft_diameter = 8mm }` must produce a
/// `sub_declaration` whose `structure_name` field text is `"ElectricMotor"` and
/// which has a `body` child of kind `specialization_body`.
#[test]
fn sub_decl_specialization_body_cst_has_specialization_body_node() {
    let source = "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    let structure_name = sub_decl
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    let structure_name_text = structure_name
        .utf8_text(source.as_bytes())
        .expect("structure_name node must be valid utf8");
    assert_eq!(
        structure_name_text, "ElectricMotor",
        "structure_name field text must be 'ElectricMotor', got: {structure_name_text:?}",
    );

    let body = sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a `body` field for specialization form");
    assert_eq!(
        body.kind(),
        "specialization_body",
        "body child must be of kind 'specialization_body', got: {:?}",
        body.kind(),
    );
}

/// The `specialization_body` of `{ shaft_diameter = 8mm }` must contain a
/// `param_assignment` node whose `name` field text is `"shaft_diameter"`.
#[test]
fn sub_decl_specialization_body_cst_param_assignment_name_field() {
    let source = "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let param_assign = find_cst_node(tree.root_node(), "param_assignment")
        .expect("expected a param_assignment node in the CST");

    let name = param_assign
        .child_by_field_name("name")
        .expect("param_assignment must have a `name` field");
    let name_text = name
        .utf8_text(source.as_bytes())
        .expect("name node must be valid utf8");
    assert_eq!(
        name_text, "shaft_diameter",
        "param_assignment name field text must be 'shaft_diameter', got: {name_text:?}",
    );
}

/// A forbidden body `{ param x : Length }` still yields ZERO CST ERROR nodes —
/// forbidden kinds parse; rejection is the validator's job.
#[test]
fn sub_decl_specialization_body_forbidden_param_body_parses_without_errors() {
    let source = "structure S { sub motor : ElectricMotor { param x : Length } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "a forbidden param member inside a specialization body must NOT produce CST ERROR \
         nodes — rejection is deferred to the validator; has_error() returned true",
    );
}

/// The where-guard-before-body form must expose both a `guard` (where_clause)
/// child and a `body` child on the same `sub_declaration`.
#[test]
fn sub_decl_specialization_body_cst_has_guard_and_body() {
    let source = "structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    sub_decl
        .child_by_field_name("guard")
        .expect("sub_declaration must have a `guard` (where_clause) field for where-guard-before-body form");

    sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a `body` field for where-guard-before-body form");
}

/// `structure S { sub a : Foo }` (bare colon, no body) must parse without ERROR
/// nodes and produce a `sub_declaration` with a `structure_name` field but NO
/// `body` child — this is the new grammar branch that previously would have been
/// a parse error.
#[test]
fn sub_decl_bare_colon_no_body_cst_shape() {
    let source = "structure S { sub a : Foo }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "bare colon no-body form must parse without CST ERROR nodes",
    );

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    let structure_name = sub_decl
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    let text = structure_name
        .utf8_text(source.as_bytes())
        .expect("utf8");
    assert_eq!(text, "Foo", "structure_name must be 'Foo', got: {text:?}");

    assert!(
        sub_decl.child_by_field_name("body").is_none(),
        "bare colon no-body form must NOT have a `body` field",
    );
}

// ── Negative grammar tests ────────────────────────────────────────────────────

/// A malformed body `structure S { sub s : Foo { = 3mm } }` (assignment missing
/// its name) must cause the parser to emit a CST ERROR node.
#[test]
fn sub_decl_specialization_body_rejects_malformed_assignment() {
    let source = "structure S { sub s : Foo { = 3mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node when a body assignment is missing its name; \
         has_error() returned false for source: {source:?}",
    );
}

// ── D4 collection / instantiation non-regression guard ───────────────────────

/// Regression: `structure S { sub a = Foo() }` still lowers to a `MemberDecl::Sub`
/// with `is_collection == false`, `body.is_none()`, and `structure_name == "Foo"`.
#[test]
fn sub_decl_instantiation_form_regression() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a = Foo() }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration");
    };
    let MemberDecl::Sub(sub) = s.members.iter().find(|m| matches!(m, MemberDecl::Sub(_))).expect("expected a Sub member") else {
        unreachable!()
    };
    assert!(!sub.is_collection, "instantiation form must have is_collection == false");
    assert!(sub.body.is_none(), "instantiation form must have body == None");
    assert_eq!(sub.structure_name, "Foo", "structure_name must be 'Foo'");
}

/// Regression: `structure S { sub a : List<Foo> }` still lowers with
/// `is_collection == true`, `body.is_none()`, `structure_name == "Foo"`.
#[test]
fn sub_decl_collection_form_regression() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a : List<Foo> }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration");
    };
    let MemberDecl::Sub(sub) = s.members.iter().find(|m| matches!(m, MemberDecl::Sub(_))).expect("expected a Sub member") else {
        unreachable!()
    };
    assert!(sub.is_collection, "collection form must have is_collection == true");
    assert!(sub.body.is_none(), "collection form must have body == None");
    assert_eq!(sub.structure_name, "Foo", "structure_name must be 'Foo'");
}

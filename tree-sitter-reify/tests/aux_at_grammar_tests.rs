//! Grammar integration tests for `aux` modifier and `at` pose clause.
//!
//! Task 3899, step-1 (TDD RED): verifies the grammar shape for `aux`/`at`
//! additions after step-2 (grammar edit) lands. Until then, fixtures (a)-(f)
//! produce ERROR nodes and fail — that is the intended RED signal.
//!
//! Fixtures (a)-(f) produce ERROR nodes with the current grammar because
//! `aux` and `at` are not yet reserved keywords. Baselines (g)-(h) pass
//! today and serve as regression guards.
//!
//! All members are wrapped in `structure S { ... }` so the grammar sees
//! them in a valid declaration context.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Walk a tree and collect all node kinds (depth-first, including anonymous nodes).
fn collect_kinds(node: tree_sitter::Node) -> Vec<String> {
    let mut kinds = vec![node.kind().to_string()];
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            kinds.extend(collect_kinds(cursor.node()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    kinds
}

/// Depth-first search for the first node with the given kind.
fn find_node_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_node_by_kind(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

/// Check if a node has an anonymous child whose source text equals `text`.
fn has_anonymous_child_text(node: tree_sitter::Node, source: &[u8], text: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            let slice = &source[child.start_byte()..child.end_byte()];
            if slice == text.as_bytes() {
                return true;
            }
        }
    }
    false
}

// ── Regression baselines (GREEN before and after grammar change) ──────────

/// Baseline (g): plain `let x = 5mm` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_let_parses() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 5mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `let x = 5mm` must parse cleanly (regression baseline)"
    );
}

/// Baseline (h): plain `sub a = Foo()` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_sub_instantiation_parses() {
    let mut parser = make_parser();
    let source = b"structure S { sub a = Foo() }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `sub a = Foo()` must parse cleanly (regression baseline)"
    );
}

// ── Fixture (a): `aux let x = cylinder(8mm, 40mm)` ───────────────────────

/// Fixture (a): `aux let x = cylinder(8mm, 40mm)` inside a structure body.
///
/// RED until step-2 grammar change: `aux` keyword not yet reserved.
#[test]
fn fixture_a_aux_let_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { aux let x = cylinder(8mm, 40mm) }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (a): `aux let x = cylinder(8mm, 40mm)` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (a): the `let_declaration` node must have
/// an anonymous `aux` token child.
///
/// RED until step-2.
#[test]
fn fixture_a_aux_let_has_aux_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { aux let x = cylinder(8mm, 40mm) }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let let_decl = find_node_by_kind(root, "let_declaration")
        .expect("let_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(let_decl, source, "aux"),
        "fixture (a): let_declaration must have an anonymous 'aux' token child; \
         kinds under let_declaration: {:?}",
        collect_kinds(let_decl)
    );
}

// ── Fixture (b): `aux sub a : T` ─────────────────────────────────────────

/// Fixture (b): `aux sub a : T` (bare specialization form) inside a structure body.
///
/// RED until step-2.
#[test]
fn fixture_b_aux_sub_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { aux sub a : T }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (b): `aux sub a : T` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (b): the `sub_declaration` node must have
/// an anonymous `aux` token child.
///
/// RED until step-2.
#[test]
fn fixture_b_aux_sub_has_aux_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { aux sub a : T }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let sub_decl = find_node_by_kind(root, "sub_declaration")
        .expect("sub_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(sub_decl, source, "aux"),
        "fixture (b): sub_declaration must have an anonymous 'aux' token child; \
         kinds under sub_declaration: {:?}",
        collect_kinds(sub_decl)
    );
}

// ── Fixture (c): `sub b : T at frame3(o, b)` ─────────────────────────────

/// Fixture (c): `sub b : T at frame3(o, b)` (specialization form with pose clause).
///
/// RED until step-2: `at` keyword not yet reserved.
#[test]
fn fixture_c_sub_with_at_pose_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { sub b : T at frame3(o, b) }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (c): `sub b : T at frame3(o, b)` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (c): the `sub_declaration` node must expose
/// a `pose` field child (as declared by `field('pose', ...)` in the grammar).
///
/// RED until step-2.
#[test]
fn fixture_c_sub_with_at_pose_has_pose_field() {
    let mut parser = make_parser();
    let source = b"structure S { sub b : T at frame3(o, b) }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let sub_decl = find_node_by_kind(root, "sub_declaration")
        .expect("sub_declaration not found in parse tree");
    assert!(
        sub_decl.child_by_field_name("pose").is_some(),
        "fixture (c): sub_declaration must expose a 'pose' field child; \
         kinds under sub_declaration: {:?}",
        collect_kinds(sub_decl)
    );
}

// ── Fixture (d): `sub c : T { } at p` ────────────────────────────────────

/// Fixture (d): `sub c : T { } at p` — specialization form with body AND trailing pose.
/// The `at` clause comes AFTER the body brace, per PRD §2.2.
///
/// RED until step-2.
#[test]
fn fixture_d_sub_with_body_and_at_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { sub c : T { } at p }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (d): `sub c : T {{ }} at p` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

// ── Fixture (e): instantiation-arm `at`: `sub bolt = Foo() at p` ─────────

/// Fixture (e): instantiation-arm pose clause — `sub bolt = Foo() at p`.
///
/// RED until step-2.
#[test]
fn fixture_e_sub_instantiation_with_at_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { sub bolt = Foo() at p }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (e): `sub bolt = Foo() at p` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

// ── Fixture (f): `pub aux let x = 5mm` ───────────────────────────────────

/// Fixture (f): combined `pub aux let x = 5mm` — both modifiers present.
///
/// RED until step-2.
#[test]
fn fixture_f_pub_aux_let_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { pub aux let x = 5mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (f): `pub aux let x = 5mm` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (f): the `let_declaration` node must have
/// an anonymous `aux` token child.
///
/// RED until step-2.
#[test]
fn fixture_f_pub_aux_let_has_aux_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { pub aux let x = 5mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let let_decl = find_node_by_kind(root, "let_declaration")
        .expect("let_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(let_decl, source, "aux"),
        "fixture (f): let_declaration must have an anonymous 'aux' token child; \
         kinds under let_declaration: {:?}",
        collect_kinds(let_decl)
    );
}

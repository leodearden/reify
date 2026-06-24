//! Grammar integration tests for `priv` member-level visibility modifier.
//!
//! Task 3976, step-1 (TDD RED): verifies the grammar shape for `priv`
//! additions after step-2 (grammar edit) lands. Until then, fixtures (a)-(e)
//! produce ERROR nodes and fail — that is the intended RED signal.
//!
//! Fixtures (a)-(e) produce ERROR nodes with the current grammar because
//! `priv` is not yet in the grammar. Baselines (f)-(h) pass today and serve
//! as regression guards.
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

/// Baseline (f): plain `param w : Length = 1mm` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_param_parses() {
    let mut parser = make_parser();
    let source = b"structure S { param w : Length = 1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `param w : Length = 1mm` must parse cleanly (regression baseline)"
    );
}

/// Baseline (g): plain `sub a = Foo()` inside a structure body parses cleanly.
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

/// Baseline (h): plain `port p : MechPort` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_port_parses() {
    let mut parser = make_parser();
    let source = b"structure S { port p : MechPort }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `port p : MechPort` must parse cleanly (regression baseline)"
    );
}

// ── Fixture (a): `priv param rated_torque : Torque = 5` ──────────────────

/// Fixture (a): `priv param rated_torque : Torque = 5` inside a structure body.
///
/// RED until step-2 grammar change: `priv` keyword not yet in grammar.
#[test]
fn fixture_a_priv_param_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv param rated_torque : Torque = 5 }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (a): `priv param rated_torque : Torque = 5` must parse cleanly after grammar \
         change; got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (a): the `param_declaration` node must have
/// an anonymous `priv` token child.
///
/// RED until step-2.
#[test]
fn fixture_a_priv_param_has_priv_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { priv param rated_torque : Torque = 5 }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let param_decl = find_node_by_kind(root, "param_declaration")
        .expect("param_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(param_decl, source, "priv"),
        "fixture (a): param_declaration must have an anonymous 'priv' token child; \
         kinds under param_declaration: {:?}",
        collect_kinds(param_decl)
    );
}

// ── Fixture (b): `priv sub inner = Inner()` ──────────────────────────────

/// Fixture (b): `priv sub inner = Inner()` (instantiation form) inside a structure body.
///
/// RED until step-2.
#[test]
fn fixture_b_priv_sub_instantiation_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv sub inner = Inner() }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (b): `priv sub inner = Inner()` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (b): the `sub_declaration` node must have
/// an anonymous `priv` token child.
///
/// RED until step-2.
#[test]
fn fixture_b_priv_sub_has_priv_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { priv sub inner = Inner() }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let sub_decl = find_node_by_kind(root, "sub_declaration")
        .expect("sub_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(sub_decl, source, "priv"),
        "fixture (b): sub_declaration must have an anonymous 'priv' token child; \
         kinds under sub_declaration: {:?}",
        collect_kinds(sub_decl)
    );
}

// ── Fixture (c): `priv port hidden : MechPort` ───────────────────────────

/// Fixture (c): `priv port hidden : MechPort` inside a structure body.
///
/// RED until step-2.
#[test]
fn fixture_c_priv_port_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv port hidden : MechPort }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (c): `priv port hidden : MechPort` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (c): the `port_declaration` node must have
/// an anonymous `priv` token child.
///
/// RED until step-2.
#[test]
fn fixture_c_priv_port_has_priv_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { priv port hidden : MechPort }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let port_decl = find_node_by_kind(root, "port_declaration")
        .expect("port_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(port_decl, source, "priv"),
        "fixture (c): port_declaration must have an anonymous 'priv' token child; \
         kinds under port_declaration: {:?}",
        collect_kinds(port_decl)
    );
}

// ── Fixture (d): combined `priv aux sub a : T` ───────────────────────────

/// Fixture (d): combined `priv aux sub a : T` — both modifiers present.
/// Confirms visibility-before-aux ordering (mirrors `pub aux let` from let_declaration).
///
/// RED until step-2.
#[test]
fn fixture_d_priv_aux_sub_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv aux sub a : T }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (d): `priv aux sub a : T` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (d): the `sub_declaration` node must have
/// both anonymous `priv` and `aux` token children.
///
/// RED until step-2.
#[test]
fn fixture_d_priv_aux_sub_has_both_anonymous_children() {
    let mut parser = make_parser();
    let source = b"structure S { priv aux sub a : T }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let sub_decl = find_node_by_kind(root, "sub_declaration")
        .expect("sub_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(sub_decl, source, "priv"),
        "fixture (d): sub_declaration must have an anonymous 'priv' token child; \
         kinds under sub_declaration: {:?}",
        collect_kinds(sub_decl)
    );
    assert!(
        has_anonymous_child_text(sub_decl, source, "aux"),
        "fixture (d): sub_declaration must have an anonymous 'aux' token child; \
         kinds under sub_declaration: {:?}",
        collect_kinds(sub_decl)
    );
}

// ── Negative ordering test ────────────────────────────────────────────────

/// Negative: `aux priv sub a : T` (reversed modifier order) must produce an ERROR node.
///
/// The grammar fixes ordering as `optional('priv'), optional('aux')`, mirroring
/// `optional('pub'), optional('aux')` from `let_declaration`. The reversed form
/// `aux priv sub` is therefore a parse error. This test pins the single-order
/// contract so a future grammar refactor cannot silently start accepting both
/// orders (reviewer suggestion, task 3976).
#[test]
fn reversed_aux_priv_order_is_rejected() {
    let mut parser = make_parser();
    let source = b"structure S { aux priv sub a : T }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "`aux priv sub a : T` (reversed modifier order) must produce an ERROR node; \
         got node kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── Fixture (e): mv-2/mv-3 fixture files (user-observable exit-0 signal) ─

/// Fixture (e): `mv-2-priv-param.ri` parses with no error.
///
/// This is the user-observable exit-0 signal for the `priv param` feature.
/// RED until step-2: the fixture contains `priv param`, which the current grammar rejects.
#[test]
fn fixture_file_mv2_priv_param_parses() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/mv-2-priv-param.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "mv-2-priv-param.ri must parse with no error after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// Fixture (e): `mv-3-priv-sub-port.ri` parses with no error.
///
/// This is the user-observable exit-0 signal for the `priv sub`/`priv port` feature.
/// RED until step-2.
#[test]
fn fixture_file_mv3_priv_sub_port_parses() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/mv-3-priv-sub-port.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "mv-3-priv-sub-port.ri must parse with no error after grammar change; \
         got node kinds: {kinds:?}"
    );
}

// ── Task 4755 β-supplement: priv let + priv constraint ────────────────────
//
// Regression baselines (GREEN before and after grammar change): plain `let`
// and plain `constraint` must continue to parse cleanly throughout.
//
// Fixtures (4755-a)/(4755-b): `priv let` / `priv constraint` parse cleanly
// and carry an anonymous `priv` child on the respective declaration node.
// These are RED until step-2 widens the grammar.
//
// Negative guard (4755-d): `pub priv let` is rejected by the grammar
// (mutual-exclusion: optional(choice('pub','priv')) admits exactly one).

// ── Regression baselines (GREEN throughout) ───────────────────────────────

/// Baseline: plain `let x = 1mm` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_let_parses() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `let x = 1mm` must parse cleanly (regression baseline)"
    );
}

/// Baseline: plain `constraint x > 0` inside a structure body parses cleanly.
/// Must remain GREEN throughout all steps.
#[test]
fn baseline_plain_constraint_parses() {
    let mut parser = make_parser();
    let source = b"structure S { constraint x > 0 }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "plain `constraint x > 0` must parse cleanly (regression baseline)"
    );
}

// ── Fixture (4755-a): `priv let x = 1mm` ─────────────────────────────────

/// Fixture (4755-a): `priv let x = 1mm` inside a structure body.
///
/// RED until step-2 grammar change: `priv` not yet accepted before `let`.
#[test]
fn fixture_4755a_priv_let_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv let x = 1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (4755-a): `priv let x = 1mm` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (4755-a): the `let_declaration` node must
/// have an anonymous `priv` token child.
///
/// RED until step-2.
#[test]
fn fixture_4755a_priv_let_has_priv_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { priv let x = 1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let let_decl = find_node_by_kind(root, "let_declaration")
        .expect("let_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(let_decl, source, "priv"),
        "fixture (4755-a): let_declaration must have an anonymous 'priv' token child; \
         kinds under let_declaration: {:?}",
        collect_kinds(let_decl)
    );
}

// ── Fixture (4755-b): `priv constraint x > 0` ────────────────────────────

/// Fixture (4755-b): `priv constraint x > 0` inside a structure body.
///
/// RED until step-2 grammar change: `priv` not yet accepted before `constraint`.
#[test]
fn fixture_4755b_priv_constraint_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"structure S { priv constraint x > 0 }";
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "fixture (4755-b): `priv constraint x > 0` must parse cleanly after grammar change; \
         got node kinds: {kinds:?}"
    );
}

/// CST-structure assert for fixture (4755-b): the `constraint_declaration` node
/// must have an anonymous `priv` token child.
///
/// RED until step-2.
#[test]
fn fixture_4755b_priv_constraint_has_priv_anonymous_child() {
    let mut parser = make_parser();
    let source = b"structure S { priv constraint x > 0 }";
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();

    let constraint_decl = find_node_by_kind(root, "constraint_declaration")
        .expect("constraint_declaration not found in parse tree");
    assert!(
        has_anonymous_child_text(constraint_decl, source, "priv"),
        "fixture (4755-b): constraint_declaration must have an anonymous 'priv' token child; \
         kinds under constraint_declaration: {:?}",
        collect_kinds(constraint_decl)
    );
}

// ── Negative guard (4755-d): `pub priv let` mutual exclusion ─────────────

/// Negative (4755-d): `pub priv let x = 1mm` must produce an ERROR node.
///
/// `let_declaration` uses `optional(choice('pub', 'priv'))`, which admits
/// at most one visibility modifier. The doubled form `pub priv let` is
/// therefore a parse error. This test pins the mutual-exclusion contract so
/// a future grammar refactor cannot silently start accepting both at once.
#[test]
fn pub_priv_let_double_modifier_is_rejected() {
    let mut parser = make_parser();
    let source = b"structure S { pub priv let x = 1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "`pub priv let x = 1mm` (double visibility modifier) must produce an ERROR node; \
         got node kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── Fixture (4755-e): mv-4-priv-let-constraint.ri ────────────────────────

/// Fixture (4755-e): `mv-4-priv-let-constraint.ri` parses with no error.
///
/// This is the user-observable exit-0 signal for the `priv let`/`priv constraint`
/// feature (task 4755). RED until step-2 widens the grammar.
#[test]
fn fixture_file_mv4_priv_let_constraint_parses() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/mv-4-priv-let-constraint.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let kinds = collect_kinds(tree.root_node());
    assert!(
        !tree.root_node().has_error(),
        "mv-4-priv-let-constraint.ri must parse with no error after grammar change; \
         got node kinds: {kinds:?}"
    );
}

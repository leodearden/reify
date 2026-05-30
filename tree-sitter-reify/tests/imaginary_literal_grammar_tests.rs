//! Grammar integration tests for `imaginary_literal` (j-suffix on a number).
//!
//! Task 3947, step-1 (TDD RED): verifies the CST shape for imaginary literals
//! after step-2 (grammar.js + scanner.c edit) lands.
//!
//! Until step-2 lands:
//!   - fixtures (a)-(c) FAIL: 4.1j/2j/1.5e-3j parse as quantity_literal today.
//!   - fixtures (d)-(e) PASS: quantity regression guards (4.1mm, 4.1jk remain
//!     quantity_literal) are already correct and serve as non-regression anchors.
//!   - fixture (f) PASSES: capital-J stays quantity_literal (D1: only lowercase j).
//!
//! All members are wrapped in `structure S { let x = <expr> }` so the grammar
//! sees them in a valid declaration context.

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

// ── Fixture (a): 4.1j → imaginary_literal ────────────────────────────────

/// Fixture (a): `let x = 4.1j` must parse without errors and produce an
/// `imaginary_literal` node.
///
/// RED until step-2 grammar change: currently parses as quantity_literal.
#[test]
fn fixture_a_decimal_j_is_imaginary_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 4.1j }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "fixture (a): `4.1j` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_some(),
        "fixture (a): expected an `imaginary_literal` node for `4.1j`; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    // The value field must be a number_literal.
    let imag_node = imag.unwrap();
    let value_child = imag_node.child_by_field_name("value");
    assert!(
        value_child.map_or(false, |n| n.kind() == "number_literal"),
        "fixture (a): imaginary_literal must have a `value` field of kind `number_literal`; \
         found: {:?}",
        collect_kinds(imag_node)
    );
}

// ── Fixture (b): 2j → imaginary_literal ──────────────────────────────────

/// Fixture (b): `let x = 2j` (integer mantissa) must produce an
/// `imaginary_literal` node.
///
/// RED until step-2.
#[test]
fn fixture_b_integer_j_is_imaginary_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 2j }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "fixture (b): `2j` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_some(),
        "fixture (b): expected an `imaginary_literal` node for `2j`; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── Fixture (c): 1.5e-3j → imaginary_literal (scientific mantissa) ────────

/// Fixture (c): `let x = 1.5e-3j` (scientific-notation mantissa + j) must
/// produce an `imaginary_literal` node.
///
/// RED until step-2.
#[test]
fn fixture_c_scientific_j_is_imaginary_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 1.5e-3j }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "fixture (c): `1.5e-3j` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_some(),
        "fixture (c): expected an `imaginary_literal` node for `1.5e-3j`; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── Regression guard (d): 4.1mm → quantity_literal ───────────────────────

/// Regression guard (d): `let x = 4.1mm` must remain a `quantity_literal`.
/// GREEN before and after grammar change.
#[test]
fn guard_d_mm_stays_quantity_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 4.1mm }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "guard (d): `4.1mm` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let qty = find_node_by_kind(tree.root_node(), "quantity_literal");
    assert!(
        qty.is_some(),
        "guard (d): `4.1mm` must produce a `quantity_literal` node; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    // Confirm no imaginary_literal appeared.
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_none(),
        "guard (d): `4.1mm` must NOT produce an `imaginary_literal` node"
    );
}

// ── Regression guard (e): 4.1jk → quantity_literal with unit "jk" ────────

/// Regression guard (e): `let x = 4.1jk` must remain a `quantity_literal`
/// whose unit_name spans the full string "jk", not just "k".
/// GREEN before and after grammar change.
#[test]
fn guard_e_jk_multi_char_stays_quantity_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 4.1jk }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "guard (e): `4.1jk` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let qty = find_node_by_kind(tree.root_node(), "quantity_literal");
    assert!(
        qty.is_some(),
        "guard (e): `4.1jk` must produce a `quantity_literal` node; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    // No imaginary_literal.
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_none(),
        "guard (e): `4.1jk` must NOT produce an `imaginary_literal` node"
    );
    // The unit_name must cover "jk".
    let unit_name = find_node_by_kind(tree.root_node(), "unit_name")
        .expect("guard (e): unit_name node not found");
    let unit_text = &source[unit_name.start_byte()..unit_name.end_byte()];
    assert_eq!(
        unit_text, b"jk",
        "guard (e): unit_name must span `jk` but got `{}`",
        String::from_utf8_lossy(unit_text)
    );
}

// ── Regression guard (f): 4.1J → quantity_literal (capital J = joule, D1) ──

/// Regression guard (f): `let x = 4.1J` (capital J = Joule) must remain a
/// `quantity_literal`.  D1 specifies only lowercase `j` becomes imaginary.
/// GREEN before and after grammar change.
#[test]
fn guard_f_capital_j_stays_quantity_literal() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 4.1J }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "guard (f): `4.1J` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let qty = find_node_by_kind(tree.root_node(), "quantity_literal");
    assert!(
        qty.is_some(),
        "guard (f): `4.1J` must produce a `quantity_literal` node; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_none(),
        "guard (f): `4.1J` must NOT produce an `imaginary_literal` node (D1: lowercase j only)"
    );
}

//! Grammar integration tests for `imaginary_literal` (j-suffix on a number).
//!
//! Task 3947: verifies the CST shape for imaginary literals.  The grammar
//! change (grammar.js + scanner.c) has landed; all fixtures below should pass.
//!
//! These tests focus on the assertions the corpus harness cannot express via
//! Rust API:
//!   - fixture (a): `.child_by_field_name("value")` field-kind check
//!   - guard (e): byte-span verification that unit_name covers "jk" (not just "k")
//!
//! Broader CST shape assertions (imaginary_literal node exists, quantity guards)
//! are also retained here because `cargo test` is the pipeline-exercised gate.
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

// ── Fixture (a): 4.1j → imaginary_literal with value-field check ──────────

/// Fixture (a): `let x = 4.1j` must parse without errors and produce an
/// `imaginary_literal` node whose `value` field is a `number_literal`.
///
/// The field-kind check (`.child_by_field_name("value")`) is the unique
/// assertion not expressible in the corpus S-expr harness.
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
        value_child.is_some_and(|n| n.kind() == "number_literal"),
        "fixture (a): imaginary_literal must have a `value` field of kind `number_literal`; \
         found: {:?}",
        collect_kinds(imag_node)
    );
}

// ── Regression guard (d): 4.1mm → quantity_literal ───────────────────────

/// Regression guard (d): `let x = 4.1mm` must remain a `quantity_literal`.
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
///
/// This byte-span assertion is not expressible in the corpus S-expr harness
/// (the corpus only asserts node kind, not text content).
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

// ── Regression guard (g): 2 j (space-separated) is rejected, never imaginary_literal ──

/// Regression guard (g): `let x = 2 j` (mantissa and `j` separated by whitespace)
/// must be REJECTED. The space breaks j-suffix adjacency; D1 requires the `j`
/// to be lexically fused to the mantissa for an `imaginary_literal` to form.
///
/// This is the CI-exercised (`cargo test`) home for the rejection invariant that
/// `test/corpus/imaginary_literal.txt`'s "no-whitespace: 2 j with a space must
/// NOT form an imaginary_literal" case also pins — `test/corpus/` is NOT run by
/// any gate (see #5492). Unlike the corpus pin, this guard deliberately does NOT
/// assert on the `UNEXPECTED` pseudo-token that `tree-sitter test` renders: that
/// token is a GLR error-recovery rendering detail, not reachable via `Node`'s
/// kind/span API, and it is precisely the detail that drifted under #5492 (it
/// moved from `'j'` to `'\n'` without the rejection itself regressing). Task
/// 3947 lineage: see fixture (a) and guards (d)-(f) above.
#[test]
fn guard_g_space_separated_j_is_rejected() {
    let mut parser = make_parser();
    let source = b"structure S { let x = 2 j }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "guard (g): `2 j` (space-separated) must be REJECTED; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let imag = find_node_by_kind(tree.root_node(), "imaginary_literal");
    assert!(
        imag.is_none(),
        "guard (g): `2 j` (space-separated) must NEVER produce an `imaginary_literal` \
         node; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let num = find_node_by_kind(tree.root_node(), "number_literal");
    assert!(
        num.is_some(),
        "guard (g): the mantissa `2` must still parse as a bare `number_literal`; \
         got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    let err =
        find_node_by_kind(tree.root_node(), "ERROR").expect("guard (g): expected an ERROR node");
    let err_text = &source[err.start_byte()..err.end_byte()];
    assert_eq!(
        err_text, b"j",
        "guard (g): the ERROR node must span exactly the `j`, but got `{}`",
        String::from_utf8_lossy(err_text)
    );
}

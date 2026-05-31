//! Grammar integration tests for named-field enum declarations (task 3936 α).
//!
//! Step-3 RED: the current grammar only accepts bare identifiers in the enum
//! body — named-field variants like `Circle { radius: Length }` produce ERROR
//! subtrees.  Step-4 lands the `enum_variant` / `variant_field_decl` grammar
//! productions that make these assertions pass (GREEN).
//!
//! Two fixture files drive the assertions:
//!   - `test/fixtures/dce-2-nameddecl.ri` — mixed bare + named-field enum
//!   - `test/fixtures/dce-construction-expr.ri` — brace construction in param-default

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first count of ERROR and MISSING nodes.
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = 0;
    if node.is_error() || node.is_missing() {
        count += 1;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            count += count_errors(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    count
}

/// Collect all node kinds depth-first (for error diagnostics).
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
fn find_node_by_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
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

/// Collect all nodes with the given kind (depth-first).
fn find_all_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut results = Vec::new();
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            results.extend(find_all_nodes_by_kind(cursor.node(), kind));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    results
}

// ── (a) Bare-enum baseline ───────────────────────────────────────────────────

/// Baseline: `enum Dir { In, Out }` parses with 0 ERROR nodes.
/// This should pass before AND after the grammar change.
#[test]
fn bare_enum_baseline_parses_cleanly() {
    let mut parser = make_parser();
    let source = b"enum Dir { In, Out }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "bare enum must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── (b) Named-field enum fixture ────────────────────────────────────────────

/// Parse dce-2-nameddecl.ri and assert 0 ERROR/MISSING nodes.
///
/// RED: the current grammar produces ERROR nodes for `Circle { radius: Length }`.
/// GREEN (step-4): the `enum_variant` + `variant_field_decl` productions handle it.
#[test]
fn fixture_named_field_enum_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/dce-2-nameddecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "dce-2-nameddecl.ri must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// The enum body in dce-2-nameddecl.ri must contain `enum_variant` nodes.
///
/// RED: no `enum_variant` production exists yet.
#[test]
fn fixture_named_field_enum_contains_enum_variant_nodes() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/dce-2-nameddecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");

    let variants = find_all_nodes_by_kind(tree.root_node(), "enum_variant");
    assert_eq!(
        variants.len(),
        3,
        "expected 3 enum_variant nodes (Point, Circle, Rect); got {}; kinds: {:?}",
        variants.len(),
        collect_kinds(tree.root_node())
    );
}

/// The bare variant `Point` produces an `enum_variant` with no payload children.
///
/// RED: no `enum_variant` production exists yet.
#[test]
fn bare_variant_point_has_no_payload_fields() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/dce-2-nameddecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");

    let variants = find_all_nodes_by_kind(tree.root_node(), "enum_variant");
    // First variant should be Point (bare).
    assert!(!variants.is_empty(), "no enum_variant nodes found");
    let point = &variants[0];
    let name_node = point.child_by_field_name("name");
    assert!(
        name_node.is_some(),
        "Point enum_variant must have a 'name' field"
    );
    let field_decls = find_all_nodes_by_kind(*point, "variant_field_decl");
    assert_eq!(
        field_decls.len(),
        0,
        "Point must have no variant_field_decl children (bare variant)"
    );
}

/// The named variant `Circle` produces an `enum_variant` with one `variant_field_decl`.
///
/// RED: no `enum_variant` production exists yet.
#[test]
fn named_variant_circle_has_one_field_decl() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/dce-2-nameddecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");

    let variants = find_all_nodes_by_kind(tree.root_node(), "enum_variant");
    assert!(variants.len() >= 2, "expected at least 2 enum_variant nodes");
    let circle = &variants[1]; // second variant
    let field_decls = find_all_nodes_by_kind(*circle, "variant_field_decl");
    assert_eq!(
        field_decls.len(),
        1,
        "Circle must have 1 variant_field_decl; got {}",
        field_decls.len()
    );
    // The field must have a 'field' child (identifier) and a 'type' child.
    let field = &field_decls[0];
    assert!(
        field.child_by_field_name("field").is_some(),
        "variant_field_decl must have a 'field' named child"
    );
    assert!(
        field.child_by_field_name("type").is_some(),
        "variant_field_decl must have a 'type' named child"
    );
}

/// The named variant `Rect` has two `variant_field_decl` children.
///
/// RED: no `enum_variant` production exists yet.
#[test]
fn named_variant_rect_has_two_field_decls() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/dce-2-nameddecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");

    let variants = find_all_nodes_by_kind(tree.root_node(), "enum_variant");
    assert!(variants.len() >= 3, "expected at least 3 enum_variant nodes");
    let rect = &variants[2]; // third variant
    let field_decls = find_all_nodes_by_kind(*rect, "variant_field_decl");
    assert_eq!(
        field_decls.len(),
        2,
        "Rect must have 2 variant_field_decls; got {}",
        field_decls.len()
    );
}

//! Grammar integration tests for type parameters on enum declarations (task 4029 α).
//!
//! Step-1 RED: the current grammar's `enum_declaration` has no
//! `optional($.type_parameters)`, so `enum Maybe<T> { ... }` produces ERROR
//! subtrees.  Step-2 adds `optional($.type_parameters)` to `enum_declaration`
//! at the same post-name position used by structure_definition / trait_declaration
//! / function_definition — making all assertions below pass (GREEN).
//!
//! Three fixture files drive the parse-signal assertions:
//!   - `test/fixtures/gde-0-baseline.ri` — bare enum (regression floor)
//!   - `test/fixtures/gde-6-genbarevariants.ri` — enum Maybe<T> { Nothing, Just }
//!   - `test/fixtures/gde-1-genenumdecl.ri` — enum Result<T,E> { Ok{value:T}, … }

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

// ── (c) Baseline regression floor ────────────────────────────────────────────

/// `enum Dir { In, Out }` must parse with 0 ERROR/MISSING nodes.
/// This is the regression floor — must stay GREEN before and after the change.
///
/// Both before and after step-2 this should pass (bare enum is not affected).
#[test]
fn baseline_bare_enum_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/gde-0-baseline.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "gde-0-baseline.ri must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── (a) Generic bare-variant enum fixture ────────────────────────────────────

/// `enum Maybe<T> { Nothing, Just }` parses with 0 ERROR/MISSING nodes.
///
/// RED: no `optional($.type_parameters)` in enum_declaration — `<T>` is an ERROR.
/// GREEN (step-2): type_parameters added to the enum head.
#[test]
fn fixture_generic_bare_variants_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/gde-6-genbarevariants.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "gde-6-genbarevariants.ri must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// The `enum Maybe<T>` enum_declaration must contain a `type_parameters` node
/// whose `type_parameter` name child text is "T".
///
/// RED: no type_parameters production in enum_declaration.
/// GREEN (step-2): the type_parameters node is present.
#[test]
fn generic_bare_variants_enum_has_type_parameters_node() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/gde-6-genbarevariants.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let source_bytes = source.as_ref();

    // Locate the enum_declaration node.
    let enum_decl = find_node_by_kind(tree.root_node(), "enum_declaration")
        .expect("expected an enum_declaration node");

    // It must have a type_parameters child.
    let type_params = find_node_by_kind(enum_decl, "type_parameters")
        .expect("enum_declaration must have a type_parameters node");

    // The single type_parameter must have name "T".
    let type_param_nodes = find_all_nodes_by_kind(type_params, "type_parameter");
    assert_eq!(
        type_param_nodes.len(),
        1,
        "Maybe<T> must have exactly one type_parameter; got {}",
        type_param_nodes.len()
    );
    let name_node = type_param_nodes[0]
        .child_by_field_name("name")
        .expect("type_parameter must have a 'name' field");
    let name_text = &source_bytes[name_node.byte_range()];
    assert_eq!(
        name_text,
        b"T",
        "type_parameter name must be 'T'; got {:?}",
        std::str::from_utf8(name_text)
    );
}

// ── (b) Generic named-field enum fixture ─────────────────────────────────────

/// `enum Result<T, E> { Ok { value: T }, Err { error: E } }` parses with 0
/// ERROR/MISSING nodes.
///
/// RED: `<T, E>` after the enum name produces ERROR nodes.
/// GREEN (step-2): type_parameters added to the enum head.
#[test]
fn fixture_generic_named_field_enum_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/gde-1-genenumdecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "gde-1-genenumdecl.ri must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// In `enum Result<T, E>`, the Ok variant's `variant_field_decl` named 'value'
/// must have a `type` child that is a `type_expr` wrapping an `identifier` "T".
///
/// RED: the whole enum_declaration is broken by the ERROR for `<T, E>`.
/// GREEN (step-2): the full CST is well-formed.
#[test]
fn result_ok_value_field_type_is_identifier_t() {
    let mut parser = make_parser();
    let source = include_bytes!("../test/fixtures/gde-1-genenumdecl.ri");
    let tree = parser.parse(source, None).expect("parse failed");
    let source_bytes = source.as_ref();

    // Find all variant_field_decl nodes (across the whole tree).
    let field_decls = find_all_nodes_by_kind(tree.root_node(), "variant_field_decl");
    assert!(
        !field_decls.is_empty(),
        "expected variant_field_decl nodes in gde-1; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );

    // The first field_decl should be Ok's `value: T`.
    let value_decl = &field_decls[0];
    let field_name_node = value_decl
        .child_by_field_name("field")
        .expect("variant_field_decl must have a 'field' child");
    let field_name_text = &source_bytes[field_name_node.byte_range()];
    assert_eq!(
        field_name_text,
        b"value",
        "first field must be 'value'; got {:?}",
        std::str::from_utf8(field_name_text)
    );

    // The type child must be a type_expr node.
    let type_child = value_decl
        .child_by_field_name("type")
        .expect("variant_field_decl must have a 'type' child");
    assert_eq!(
        type_child.kind(),
        "type_expr",
        "type child of variant_field_decl must be type_expr; got {}",
        type_child.kind()
    );

    // Inside type_expr there must be an identifier "T".
    let ident = find_node_by_kind(type_child, "identifier")
        .expect("type_expr must contain an identifier");
    let ident_text = &source_bytes[ident.byte_range()];
    assert_eq!(
        ident_text,
        b"T",
        "type_expr identifier must be 'T'; got {:?}",
        std::str::from_utf8(ident_text)
    );
}

// ── (f) Recursive generic form — inline source ────────────────────────────────

/// Inline source: `enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }`
///
/// Asserts:
/// - parses with 0 ERROR/MISSING nodes
/// - Node's `left` variant_field_decl `type` child is a `type_expr` wrapping a
///   `parameterized_type` (Tree<T>)
///
/// RED: `<T>` in the enum head produces ERROR.
/// GREEN (step-2): the full recursive form is valid.
#[test]
fn recursive_generic_enum_parses_with_zero_errors() {
    let mut parser = make_parser();
    let source = b"enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }";
    let tree = parser.parse(source, None).expect("parse failed");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "recursive Tree<T> enum must parse with 0 ERROR/MISSING; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// In the recursive `Tree<T>`, Node's `left` field type is a `parameterized_type`
/// (i.e. `Tree<T>` — a named type with a type argument).
///
/// RED: the enum head breaks the whole declaration.
/// GREEN (step-2): the CST is well-formed and `left` has a parameterized_type.
#[test]
fn recursive_generic_enum_left_field_is_parameterized_type() {
    let mut parser = make_parser();
    let source = b"enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }";
    let tree = parser.parse(source, None).expect("parse failed");

    // Collect all variant_field_decl nodes.
    let field_decls = find_all_nodes_by_kind(tree.root_node(), "variant_field_decl");
    // Expected order: Leaf.value, Node.left, Node.right (3 total).
    assert!(
        field_decls.len() >= 2,
        "expected at least 2 variant_field_decl nodes; got {}",
        field_decls.len()
    );

    // Node.left is the second field_decl (index 1).
    let left_decl = &field_decls[1];
    let type_child = left_decl
        .child_by_field_name("type")
        .expect("left variant_field_decl must have a 'type' child");
    assert_eq!(
        type_child.kind(),
        "type_expr",
        "type child must be type_expr; got {}",
        type_child.kind()
    );

    // The type_expr must contain a parameterized_type (Tree<T>).
    let param_type = find_node_by_kind(type_child, "parameterized_type")
        .expect("type_expr for 'left: Tree<T>' must contain a parameterized_type node");
    assert!(
        !param_type.is_error(),
        "parameterized_type must not be an ERROR node"
    );
}

//! Grammar integration tests for qualified references through an import
//! binding — `pp.Pulley` in TYPE position and as a `sub` structure_name
//! (task 5495 μ, step-1 RED / step-2 GREEN), plus the EXPRESSION-position call
//! form `pp.Pulley()` (step-3 RED / step-4 GREEN).
//!
//! Step-1 RED: `type_expr` (grammar.js ~1064) is
//! `choice(function_type, parameterized_type, qualified_type, identifier)` — it
//! has NO dotted arm, so the `.` in `pp.Pulley` matches no arm and produces an
//! ERROR subtree.  `sub_declaration` (grammar.js ~805) binds
//! `field('structure_name', $.identifier)` in all three arms, so a dotted
//! structure name likewise ERRORs.
//!
//! Step-2 GREEN: one shared rule
//!   `namespaced_name: $ => seq(field('binding', $.identifier), '.',
//!                              field('name', $.identifier))`
//! is added as an arm of `type_expr` (covering all `$.type_expr` use sites plus
//! `type_arg_list` at once) and as an alternative for `structure_name` in all
//! three `sub_declaration` arms.
//!
//! This file is the CI-visible pin for PRD `docs/prds/v0_6/stdlib-namespace.md`
//! §7 boundary #15: `tree-sitter parse --quiet` and `tree-sitter test` are never
//! invoked by any gate or hook, so the two prd-gate fixtures would otherwise be
//! silently unverified.  The fixtures are read via `CARGO_MANIFEST_DIR` (the
//! idiom at `crates/reify-compiler/tests/buckling_stdlib_compile.rs:709`).
//!
//! The harness (make_parser / count_errors / collect_kinds / find_node_by_kind /
//! find_all_nodes_by_kind) mirrors
//! tree-sitter-reify/tests/function_type_grammar_tests.rs.

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

/// Parse `source`, assert 0 ERROR/MISSING nodes, and return the tree.
fn parse_clean(source: &str) -> tree_sitter::Tree {
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`{source}` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    tree
}

/// Text of a node's named field child, for field-level assertions.
fn field_text<'a>(node: tree_sitter::Node<'a>, field: &str, source: &'a str) -> String {
    node.child_by_field_name(field)
        .unwrap_or_else(|| panic!("node `{}` has no `{field}` field", node.kind()))
        .utf8_text(source.as_bytes())
        .expect("utf8")
        .to_string()
}

// ── (a) The two prd-gate fixtures — boundary #15's CI-visible pin ────────────

/// Read a fixture from `tests/prd-gate/fixtures/` relative to this crate.
fn read_prd_gate_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/prd-gate/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read prd-gate fixture {}: {e}", path.display()))
}

/// `tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri` (`param p : pp.Pulley`)
/// parses with 0 ERROR/MISSING nodes.
///
/// RED: `type_expr` has no dotted arm — the `.` produces an ERROR subtree.
/// GREEN (step-2): the `namespaced_name` arm accepts it.
#[test]
fn prd_gate_qualified_type_fixture_parses_with_zero_errors() {
    let source = read_prd_gate_fixture("stdlib_ns_qualified_type.ri");
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "prd-gate fixture stdlib_ns_qualified_type.ri must parse with 0 \
         ERROR/MISSING nodes (PRD §7 boundary #15); got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// `tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri` (`sub p = pp.Pulley()`)
/// parses with 0 ERROR/MISSING nodes.
///
/// RED: `sub_declaration`'s `structure_name` is a bare `$.identifier`.
/// GREEN (step-2): `choice($.identifier, $.namespaced_name)` accepts it.
#[test]
fn prd_gate_qualified_expr_fixture_parses_with_zero_errors() {
    let source = read_prd_gate_fixture("stdlib_ns_qualified_expr.ri");
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "prd-gate fixture stdlib_ns_qualified_expr.ri must parse with 0 \
         ERROR/MISSING nodes (PRD §7 boundary #15); got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── (b) Type position ───────────────────────────────────────────────────────

/// `param p : pp.Pulley` puts a `namespaced_name` under `param_declaration`'s
/// `type` field, with `binding` and `name` child fields.
#[test]
fn qualified_type_annotation_yields_namespaced_name_with_fields() {
    let source = "structure def S {\n    param p : pp.Pulley\n}\n";
    let tree = parse_clean(source);

    let param = find_node_by_kind(tree.root_node(), "param_declaration")
        .expect("expected a param_declaration node");
    let type_node = param
        .child_by_field_name("type")
        .expect("param_declaration must have a `type` field");

    let ns = find_node_by_kind(type_node, "namespaced_name").unwrap_or_else(|| {
        panic!(
            "expected a namespaced_name node under the param's `type` field for \
             `pp.Pulley`; got kinds: {:?}",
            collect_kinds(type_node)
        )
    });

    assert_eq!(field_text(ns, "binding", source), "pp");
    assert_eq!(field_text(ns, "name", source), "Pulley");
}

/// `param q : List<pp.Pulley>` nests the `namespaced_name` inside a
/// `type_arg_list` — the single `type_expr` arm covers type-argument position
/// too, with no separate rule.
#[test]
fn qualified_type_inside_type_arg_list() {
    let source = "structure def S {\n    param q : List<pp.Pulley>\n}\n";
    let tree = parse_clean(source);

    let type_args = find_node_by_kind(tree.root_node(), "type_arg_list")
        .expect("expected a type_arg_list node for `List<pp.Pulley>`");
    let ns = find_node_by_kind(type_args, "namespaced_name").unwrap_or_else(|| {
        panic!(
            "expected a namespaced_name nested inside type_arg_list; got kinds: {:?}",
            collect_kinds(type_args)
        )
    });

    assert_eq!(field_text(ns, "binding", source), "pp");
    assert_eq!(field_text(ns, "name", source), "Pulley");
}

// ── (c) All three `sub_declaration` arms ────────────────────────────────────

/// Instantiation arm: `sub p = pp.Pulley()` binds `structure_name` to a
/// `namespaced_name`.
#[test]
fn sub_instantiation_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub p = pp.Pulley()\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "instantiation-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

/// Specialization arm: `sub h : pp.Pulley` binds `structure_name` to a
/// `namespaced_name`.
#[test]
fn sub_specialization_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub h : pp.Pulley\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "specialization-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

/// Collection arm: `sub i : List<pp.Pulley>` binds `structure_name` to a
/// `namespaced_name` (the `List` keyword still wins the lexer rule-#2 tie-break).
#[test]
fn sub_collection_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub i : List<pp.Pulley>\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "collection-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

// ── (d) Negative controls — nothing that parses today may change ────────────

/// `sub j = Plain()` keeps a bare `identifier` structure_name.
#[test]
fn bare_structure_name_stays_identifier() {
    let source = "structure def S {\n    sub j = Plain()\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "identifier",
        "an unqualified structure_name must stay a bare identifier; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_name").is_none(),
        "`Plain()` must not produce a namespaced_name node"
    );
}

/// `param n : Beam::Material` stays a `qualified_type` (the `::` form is
/// untouched by the new dotted arm).
#[test]
fn double_colon_type_stays_qualified_type() {
    let source = "structure def S {\n    param n : Beam::Material\n}\n";
    let tree = parse_clean(source);

    assert!(
        find_node_by_kind(tree.root_node(), "qualified_type").is_some(),
        "`Beam::Material` must stay a qualified_type; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_name").is_none(),
        "`Beam::Material` must not produce a namespaced_name node"
    );
}

/// The `List<Foo>` vs `Listicle<Foo>` lexer rule-#1 / rule-#2 discipline
/// (grammar.js:845-880) is unchanged by widening `structure_name`.
///
/// `List<Foo>` → collection arm (rule #2: the `'List'` string token beats the
/// equal-length identifier regex), so `structure_name` is `Foo`.
/// `Listicle<Foo>` → specialization arm (rule #1: longest match), so
/// `structure_name` is `Listicle`.
#[test]
fn list_vs_listicle_lexer_discipline_unchanged() {
    let list_src = "structure def S {\n    sub a : List<Foo>\n}\n";
    let tree = parse_clean(list_src);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    assert_eq!(
        field_text(sub, "structure_name", list_src),
        "Foo",
        "`List<Foo>` must take the collection arm (lexer rule #2); got kinds: {:?}",
        collect_kinds(sub)
    );

    let listicle_src = "structure def S {\n    sub b : Listicle<Foo>\n}\n";
    let tree = parse_clean(listicle_src);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    assert_eq!(
        field_text(sub, "structure_name", listicle_src),
        "Listicle",
        "`Listicle<Foo>` must take the specialization arm (lexer rule #1, \
         longest match); got kinds: {:?}",
        collect_kinds(sub)
    );
}

/// A plain (unqualified) type annotation is unaffected: `param s : Steel`
/// still has a bare `identifier` under its `type` field.
#[test]
fn bare_type_annotation_unchanged() {
    let source = "structure def S {\n    param s : Steel\n}\n";
    let tree = parse_clean(source);

    let param = find_node_by_kind(tree.root_node(), "param_declaration")
        .expect("expected a param_declaration node");
    let type_node = param
        .child_by_field_name("type")
        .expect("param_declaration must have a `type` field");
    assert!(
        find_node_by_kind(type_node, "identifier").is_some(),
        "`Steel` must stay a bare identifier; got kinds: {:?}",
        collect_kinds(type_node)
    );
    assert!(
        find_all_nodes_by_kind(type_node, "namespaced_name").is_empty(),
        "`Steel` must not produce a namespaced_name node"
    );
}

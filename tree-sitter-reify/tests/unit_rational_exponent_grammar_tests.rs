//! Grammar integration tests for rational unit exponents (`m^1/2`).
//!
//! Task #9236. The unit-pow exponent token is `unit_exponent`,
//! `token.immediate(/-?\d+(\/\d+)?/)`. It is lexed only after the unit-pow
//! `^`, so its greedy match makes a contiguous `^p/q` one rational exponent
//! inside the `unit_expr`. Before #9236 the token was integral, so
//! `50MPa*m^1/2` ended its unit expression at `^1` and the trailing `/2`
//! became a value-level division: a silent 25 MPa·m where 50 MPa·√m was
//! written.
//!
//! Canonical form (lowest terms, denominator ≥ 2) and the refusal of a `/`
//! touching an exponent are LOWERING checks (`reify-syntax`), pinned there;
//! this file pins only the CST shape.
//!
//! House restraint as in the sibling `*_grammar_tests.rs`: error PRESENCE via
//! `has_error()`, shape via node lookup and field access, never s-expressions.
//! Every source is wrapped in `structure S { let x = <expr> }`.

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

/// Every pow arm's `exponent` field under `node`, in source order, as
/// `(kind, text)`.
fn exponents(node: tree_sitter::Node, src: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(exp) = node.child_by_field_name("exponent") {
        out.push((
            exp.kind().to_string(),
            exp.utf8_text(src).expect("valid UTF-8").to_string(),
        ));
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            out.extend(exponents(cursor.node(), src));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    out
}

/// Parse `structure S { let x = <expr> }`, assert it is error-free, and return
/// the tree plus the full source.
fn parse_clean(expr: &str) -> (tree_sitter::Tree, String) {
    let source = format!("structure S {{ let x = {expr} }}");
    let tree = make_parser()
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "`{expr}` must parse cleanly; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    (tree, source)
}

/// The `let` declaration's `value` node.
fn let_value(tree: &tree_sitter::Tree) -> tree_sitter::Node<'_> {
    let decl =
        find_node_by_kind(tree.root_node(), "let_declaration").expect("a let_declaration node");
    decl.child_by_field_name("value")
        .expect("let_declaration exposes its `value` field")
}

/// A field of `node`, as its source text.
fn field_text(node: tree_sitter::Node, field: &str, src: &[u8]) -> String {
    node.child_by_field_name(field)
        .unwrap_or_else(|| {
            panic!(
                "expected a `{field}` field on `{}`; got kinds: {:?}",
                node.kind(),
                collect_kinds(node)
            )
        })
        .utf8_text(src)
        .expect("field text is valid UTF-8")
        .to_string()
}

/// Assert `expr`'s `let` value is ONE `quantity_literal` (no value-level
/// `binary_expression` anywhere) whose pow exponents are `expected`, each of
/// kind `unit_exponent`. Returns the tree and source for further checks.
fn assert_quantity_with_exponents(expr: &str, expected: &[&str]) -> (tree_sitter::Tree, String) {
    let (tree, source) = parse_clean(expr);
    let value = let_value(&tree);
    assert_eq!(
        value.kind(),
        "quantity_literal",
        "`{expr}` must be a single quantity_literal; got kinds: {:?}",
        collect_kinds(value)
    );
    assert!(
        find_node_by_kind(value, "binary_expression").is_none(),
        "`{expr}` must contain no value-level binary_expression; got kinds: {:?}",
        collect_kinds(value)
    );
    let got = exponents(value, source.as_bytes());
    let want: Vec<(String, String)> = expected
        .iter()
        .map(|t| ("unit_exponent".to_string(), (*t).to_string()))
        .collect();
    assert_eq!(got, want, "`{expr}`: unexpected pow exponents");
    (tree, source)
}

/// Assert `expr`'s `let` value is a value-level `binary_expression` with op `op`.
fn assert_value_binary(expr: &str, op: &str) {
    let (tree, source) = parse_clean(expr);
    let value = let_value(&tree);
    assert_eq!(
        value.kind(),
        "binary_expression",
        "`{expr}` must stay a value-level binary_expression; got kinds: {:?}",
        collect_kinds(value)
    );
    assert_eq!(field_text(value, "op", source.as_bytes()), op);
}

// ── ACCEPT: a contiguous `^p/q` is one rational exponent ─────────────────────

#[test]
fn half_exponent_is_one_unit_exponent_token() {
    assert_quantity_with_exponents("1m^1/2", &["1/2"]);
}

#[test]
fn negative_half_exponent_is_one_unit_exponent_token() {
    assert_quantity_with_exponents("1m^-1/2", &["-1/2"]);
}

/// The task's canonical repro: no top-level `/`, the pow sits inside the
/// unit expression.
#[test]
fn fracture_toughness_repro_keeps_the_half_inside_the_unit() {
    let (tree, source) = assert_quantity_with_exponents("50MPa*m^1/2", &["1/2"]);
    let value = let_value(&tree);
    assert_eq!(field_text(value, "unit", source.as_bytes()), "MPa*m^1/2");
}

/// The exact `Display for DimensionVector` spelling of FractureToughness.
#[test]
fn display_spelling_of_fracture_toughness_parses() {
    assert_quantity_with_exponents("1kg·m^-1/2·s^-2", &["-1/2", "-2"]);
}

/// The `.ri` emitter's spelling: the exponent `1/2` is followed by a
/// unit-level `/`.
#[test]
fn emitter_spelling_of_fracture_toughness_parses() {
    let (tree, source) = assert_quantity_with_exponents("1kg/m^1/2/s^2", &["1/2", "2"]);
    let src = source.as_bytes();
    let unit = let_value(&tree)
        .child_by_field_name("unit")
        .expect("quantity_literal exposes its `unit` field");
    assert_eq!(field_text(unit, "left", src), "kg/m^1/2");
    assert_eq!(field_text(unit, "op", src), "/");
    assert_eq!(field_text(unit, "right", src), "s^2");
}

#[test]
fn integer_exponent_is_a_unit_exponent_token() {
    assert_quantity_with_exponents("1m^2", &["2"]);
}

// ── CONTROLS: shapes the widening must not touch ─────────────────────────────

/// `/` after a unit NAME followed by a digit stays value division.
#[test]
fn slash_after_unit_name_then_digit_stays_value_division() {
    assert_value_binary("25USD/1kg", "/");
    assert_value_binary("50MPa/2", "/");
}

/// `/` after an integer exponent followed by a unit stays a unit-level `/`.
#[test]
fn slash_after_exponent_then_unit_stays_unit_division() {
    let (tree, source) = assert_quantity_with_exponents("1m^2/s", &["2"]);
    let src = source.as_bytes();
    let unit = let_value(&tree)
        .child_by_field_name("unit")
        .expect("quantity_literal exposes its `unit` field");
    assert_eq!(field_text(unit, "left", src), "m^2");
    assert_eq!(field_text(unit, "op", src), "/");
    assert_eq!(field_text(unit, "right", src), "s");
}

/// A space before `/` divides the value.
#[test]
fn spaced_slash_after_exponent_stays_value_division() {
    assert_value_binary("1m^2 / 2", "/");
}

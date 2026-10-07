//! CST tests for task #8300: an `@annotation` line after a value-ended member
//! is its own member, and an ad-hoc selector's `@` must share its base
//! expression's line.

use tree_sitter_reify::language;

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser.parse(source, None).expect("parse failed")
}

fn nodes_of_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Vec<tree_sitter::Node<'a>> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == kind {
            found.push(n);
        }
        let mut cursor = n.walk();
        stack.extend(n.children(&mut cursor));
    }
    found
}

/// Kinds of the named, non-extra children between `container`'s first `{` and
/// last `}` — its members, in source order.
fn member_kinds(container: tree_sitter::Node<'_>) -> Vec<String> {
    let mut cursor = container.walk();
    let children: Vec<_> = container.children(&mut cursor).collect();
    let open = children
        .iter()
        .position(|c| c.kind() == "{")
        .expect("no `{`");
    let close = children
        .iter()
        .rposition(|c| c.kind() == "}")
        .expect("no `}`");
    children[open + 1..close]
        .iter()
        .filter(|c| c.is_named() && !c.is_extra())
        .map(|c| c.kind().to_string())
        .collect()
}

fn top_level_kinds(root: tree_sitter::Node<'_>) -> Vec<String> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|c| !c.is_extra())
        .map(|c| c.kind().to_string())
        .collect()
}

fn assert_clean_without_selector(source: &str) -> tree_sitter::Tree {
    let tree = parse(source);
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "parse error in:\n{source}\n{}",
        root.to_sexp()
    );
    assert!(
        nodes_of_kind(root, "ad_hoc_selector").is_empty(),
        "an annotation line was joined as an ad_hoc_selector in:\n{source}\n{}",
        root.to_sexp()
    );
    tree
}

/// `structure S { <predecessor> ⏎ @solver_hint(..) ⏎ param b : Length = auto }`,
/// one member per line at column 4.
fn annotated_after(predecessor: &str) -> String {
    format!(
        "structure S {{\n    {predecessor}\n    @solver_hint(\"discrete_set\", standard_bolt_lengths)\n    param b : Length = auto\n}}\n"
    )
}

fn assert_annotation_is_its_own_member(predecessor: &str, predecessor_kind: &str) {
    let source = annotated_after(predecessor);
    let tree = assert_clean_without_selector(&source);
    let structure = nodes_of_kind(tree.root_node(), "structure_definition")
        .into_iter()
        .next()
        .expect("no structure_definition");
    assert_eq!(
        member_kinds(structure),
        [predecessor_kind, "annotation", "param_declaration"],
        "member split wrong for predecessor `{predecessor}`:\n{}",
        tree.root_node().to_sexp()
    );
}

// ── Every member kind before an annotation ─────────────────────────────────

#[test]
fn annotation_after_a_valued_param_is_its_own_member() {
    assert_annotation_is_its_own_member("param a : Length = 5mm", "param_declaration");
}

#[test]
fn annotation_after_a_let_list_is_its_own_member() {
    assert_annotation_is_its_own_member("let sizes = [22mm, 24mm, 26mm]", "let_declaration");
}

#[test]
fn annotation_after_a_constraint_is_its_own_member() {
    assert_annotation_is_its_own_member("constraint a >= 1mm", "constraint_declaration");
}

#[test]
fn annotation_after_a_sub_instantiation_is_its_own_member() {
    assert_annotation_is_its_own_member("sub s = Foo()", "sub_declaration");
}

#[test]
fn annotation_after_a_sub_with_a_pose_is_its_own_member() {
    assert_annotation_is_its_own_member("sub s = Foo() at frame0", "sub_declaration");
}

#[test]
fn annotation_after_a_minimize_is_its_own_member() {
    assert_annotation_is_its_own_member("minimize a + b", "minimize_declaration");
}

#[test]
fn annotation_after_an_auto_param_is_its_own_member() {
    assert_annotation_is_its_own_member("param x : Length = auto", "param_declaration");
}

#[test]
fn annotation_after_a_sub_collection_is_its_own_member() {
    assert_annotation_is_its_own_member("sub s : List<Foo>", "sub_declaration");
}

#[test]
fn annotations_in_both_guarded_block_bodies_are_their_own_members() {
    let source = "structure S {\n    where enabled {\n        let a = 1mm\n        @deprecated(\"x\")\n        param b : Length = auto\n    } else {\n        let c = 2mm\n        @deprecated(\"y\")\n        let d = 3mm\n    }\n}\n";
    let tree = assert_clean_without_selector(source);
    let guarded = nodes_of_kind(tree.root_node(), "guarded_block")
        .into_iter()
        .next()
        .expect("no guarded_block");
    assert_eq!(
        member_kinds(guarded),
        [
            "let_declaration",
            "annotation",
            "param_declaration",
            "let_declaration",
            "annotation",
            "let_declaration",
        ],
        "{}",
        tree.root_node().to_sexp()
    );
}

// ── Top level: the join was silent there (no member-continuation guard) ─────

fn assert_top_level_annotation_after(declaration: &str, field: &str, field_kind: &str) {
    let source = format!("{declaration}\n@deprecated(\"x\")\nstructure S {{}}\n");
    let tree = assert_clean_without_selector(&source);
    let root = tree.root_node();
    let decl = root.named_child(0).expect("no first declaration");
    assert_eq!(
        top_level_kinds(root),
        [decl.kind(), "annotation", "structure_definition"],
        "{}",
        root.to_sexp()
    );
    let value = decl
        .child_by_field_name(field)
        .unwrap_or_else(|| panic!("no `{field}` field: {}", root.to_sexp()));
    assert_eq!(value.kind(), field_kind, "{}", root.to_sexp());
}

#[test]
fn top_level_annotation_after_a_unit_conversion_is_its_own_declaration() {
    assert_top_level_annotation_after("unit foo : Length = 0.001", "conversion", "number_literal");
}

#[test]
fn top_level_annotation_after_a_default_is_its_own_declaration() {
    assert_top_level_annotation_after("default Material = steel", "value", "identifier");
}

// ── Comments between the base and a next-line `@` ───────────────────────────

#[test]
fn annotation_after_a_trailing_line_comment_is_its_own_member() {
    let source =
        "structure S {\n    let x = body // c\n    @deprecated(\"x\")\n    param y : Real = 1\n}\n";
    let tree = assert_clean_without_selector(source);
    let structure = nodes_of_kind(tree.root_node(), "structure_definition")[0];
    assert_eq!(
        member_kinds(structure),
        ["let_declaration", "annotation", "param_declaration"],
        "{}",
        tree.root_node().to_sexp()
    );
}

// ── Cross-line selector attempts are rejected ───────────────────────────────

#[test]
fn a_next_line_spaced_selector_is_a_parse_error() {
    let tree = parse("structure S {\n    let x = body\n        @ face(\"top\")\n}\n");
    assert!(
        tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
}

#[test]
fn a_next_line_selector_inside_an_argument_list_is_a_parse_error() {
    let tree = parse("structure S {\n    let y = f(body\n        @face(\"top\"))\n}\n");
    assert!(
        tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
}

// ── Same-line selectors still parse as ad_hoc_selector ──────────────────────

fn assert_same_line_selectors(member: &str, expected: usize) {
    let source = format!("structure S {{\n    {member}\n}}\n");
    let tree = parse(&source);
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "parse error in:\n{source}\n{}",
        root.to_sexp()
    );
    assert_eq!(
        nodes_of_kind(root, "ad_hoc_selector").len(),
        expected,
        "{}",
        root.to_sexp()
    );
}

#[test]
fn a_spaced_same_line_selector_still_parses() {
    assert_same_line_selectors("let x = port @ face(\"top\")", 1);
}

#[test]
fn an_unspaced_same_line_selector_still_parses() {
    assert_same_line_selectors("let x = body@face(\"top\")", 1);
}

#[test]
fn a_chained_same_line_selector_still_parses() {
    assert_same_line_selectors("let x = a.b @ face(\"t\") @ edge(1)", 2);
}

#[test]
fn a_block_comment_between_base_and_selector_is_transparent() {
    assert_same_line_selectors("let x = body /* c */ @face(\"top\")", 1);
}

#[test]
fn connect_port_refs_with_selectors_still_parse() {
    assert_same_line_selectors(
        "connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive",
        2,
    );
}

/// The deliberate boundary of the same-line rule: an `@name(...)` on the SAME
/// line as a value is still a selector (the compiler rejects the unknown kind).
#[test]
fn a_same_line_annotation_lookalike_stays_a_selector() {
    assert_same_line_selectors(
        "param a : Length = 5mm @solver_hint(\"discrete_set\", standard_bolt_lengths) param b : Length = auto",
        1,
    );
}

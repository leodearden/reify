//! Grammar integration tests for geometric-joints α (task 4395):
//! `joint NAME(datums) with <DOF> = <body>` definition syntax — BOTH the
//! single form (`with angle: Angle in 0deg..120deg`) and the record form
//! (`with { angle: Angle, travel: Length }`).
//!
//! TDD structure:
//!   - step-1 (RED): single-form tests RED until step-2 (grammar adds joint rule).
//!   - step-3 (RED): record-form tests RED until step-4 (grammar extends joint_dof).
//!
//! Parse-gate fixtures:
//!   - gr-05a-joint-with.ri  (single form)  — asserted in step-1, GREEN at step-2.
//!   - gr-05b-joint-with-rec.ri (record form) — asserted in step-3, GREEN at step-4.

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

/// Depth-first collect of every node with the given kind (pre-order).
fn find_all_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut out = Vec::new();
    collect_nodes_by_kind(node, kind, &mut out);
    out
}

fn collect_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
    out: &mut Vec<tree_sitter::Node<'a>>,
) {
    if node.kind() == kind {
        out.push(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_nodes_by_kind(cursor.node(), kind, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Check if a node has an anonymous child whose source text equals `text`.
#[allow(dead_code)]
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

/// Parse `source` and assert it has NO ERROR / MISSING node, returning the tree.
fn parse_clean(source: &[u8]) -> tree_sitter::Tree {
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "source must parse cleanly; got node kinds: {:?}\nsource: {}",
        collect_kinds(tree.root_node()),
        String::from_utf8_lossy(source)
    );
    tree
}

/// Read the source text behind a node.
fn text<'a>(node: tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap()
}

/// Read a gate fixture from `test/fixtures/<name>` and return its bytes.
#[allow(dead_code)]
fn read_fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/test/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

/// Assert a named gate fixture parses with zero ERROR/MISSING nodes.
#[allow(dead_code)]
fn assert_fixture_parses_clean(name: &str) {
    let source = read_fixture(name);
    let mut parser = make_parser();
    let tree = parser.parse(&source, None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "fixture {name} must parse with zero ERROR nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

// ── step-1 (RED until step-2): single-form `joint … with` ───────────────────

/// `joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in
/// 0deg..120deg = { coaxial(a, b)  on(a.point, stop) }` parses clean;
/// root contains a `joint_definition` node whose `name` field = "revolute";
/// it has 3 `fn_param` children; its `dof` field is a `joint_dof` carrying
/// exactly one `joint_dof_field` whose `name`="angle", `type`="Angle", and
/// `range` field is present; its `body` field is a `joint_body` carrying 2
/// `relation_member` children.
/// RED: grammar has no `joint` rule, so `joint …` yields ERROR nodes and
/// `joint_definition` is not found.
#[test]
fn joint_single_form_parses() {
    let source = b"joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = { coaxial(a, b)  on(a.point, stop) }";
    let tree = parse_clean(source);
    let root = tree.root_node();

    // Root must contain a joint_definition
    let jdef = find_node_by_kind(root, "joint_definition")
        .expect("joint_definition not found");

    // name field = "revolute"
    let name_node = jdef
        .child_by_field_name("name")
        .expect("joint_definition must expose a `name` field");
    assert_eq!(text(name_node, source), "revolute");

    // 3 fn_param children
    let params = find_all_nodes_by_kind(jdef, "fn_param");
    assert_eq!(params.len(), 3, "expected 3 fn_param children, got {}", params.len());

    // dof field: joint_dof with exactly 1 joint_dof_field
    let dof_node = jdef
        .child_by_field_name("dof")
        .expect("joint_definition must expose a `dof` field");
    assert_eq!(
        dof_node.kind(),
        "joint_dof",
        "dof field must be joint_dof, got {}",
        dof_node.kind()
    );
    let dof_fields = find_all_nodes_by_kind(dof_node, "joint_dof_field");
    assert_eq!(dof_fields.len(), 1, "single-form dof must have exactly 1 joint_dof_field, got {}", dof_fields.len());

    // DOF field assertions: name="angle", type="Angle", range present
    let dof_field = &dof_fields[0];
    let dof_name = dof_field
        .child_by_field_name("name")
        .expect("joint_dof_field must have a `name` field");
    assert_eq!(text(dof_name, source), "angle");

    let dof_type = dof_field
        .child_by_field_name("type")
        .expect("joint_dof_field must have a `type` field");
    assert_eq!(text(dof_type, source), "Angle");

    assert!(
        dof_field.child_by_field_name("range").is_some(),
        "single-form dof field with `in …` clause must have a `range` field"
    );

    // body field: joint_body with 2 relation_member children
    let body_node = jdef
        .child_by_field_name("body")
        .expect("joint_definition must expose a `body` field");
    assert_eq!(
        body_node.kind(),
        "joint_body",
        "body field must be joint_body, got {}",
        body_node.kind()
    );
    let members = find_all_nodes_by_kind(body_node, "relation_member");
    assert_eq!(
        members.len(),
        2,
        "block body must carry 2 relation_member children, got {}",
        members.len()
    );
}

/// `joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)`
/// parses clean; the lone `joint_dof_field` has NO `range` child; the `body` is the
/// single-expression form (a `function_call`, not a brace block).
/// RED until step-2.
#[test]
fn joint_single_no_range_parses() {
    let source = b"joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)";
    let tree = parse_clean(source);
    let root = tree.root_node();

    let jdef = find_node_by_kind(root, "joint_definition")
        .expect("joint_definition not found");

    // name
    assert_eq!(
        text(jdef.child_by_field_name("name").expect("name field missing"), source),
        "ball"
    );

    // dof: 1 joint_dof_field, NO range
    let dof_node = jdef.child_by_field_name("dof").expect("dof field missing");
    let dof_fields = find_all_nodes_by_kind(dof_node, "joint_dof_field");
    assert_eq!(dof_fields.len(), 1);
    assert!(
        dof_fields[0].child_by_field_name("range").is_none(),
        "joint_dof_field without `in …` must NOT carry a `range` field"
    );

    // body: single-expression (no brace block → no relation_member children at
    // joint_body level; the body's `result` field is a function_call)
    let body_node = jdef.child_by_field_name("body").expect("body field missing");
    let result_node = body_node
        .child_by_field_name("result")
        .expect("single-expr body must expose a `result` field");
    assert_eq!(
        result_node.kind(),
        "function_call",
        "single-expr body result must be a function_call, got {}",
        result_node.kind()
    );
    assert_eq!(
        find_all_nodes_by_kind(body_node, "relation_member").len(),
        0,
        "single-expr body must NOT carry relation_member children"
    );
}

/// Consolidated gate: gr-05a-joint-with.ri (the single form from the PRD α signal)
/// parses with zero ERROR nodes once step-2 lands.
/// RED until step-2.
#[test]
fn gate_gr05a_fixture_parses_clean() {
    assert_fixture_parses_clean("gr-05a-joint-with.ri");
}

// ── step-3 (RED until step-4): record-form `joint … with { … }` ─────────────

/// `joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } =
/// coaxial(a, b)` parses clean; the `joint_definition`'s `dof` field is a
/// `joint_dof` in the braced (record) variant — assert it has an anonymous `{`
/// child and carries exactly 2 `joint_dof_field` children with `name` fields
/// "angle" and "travel" in source order; the `body` is the single-expression form.
/// RED: joint_dof currently accepts only the single form, so the `{` after `with`
/// yields ERROR nodes.
#[test]
fn joint_record_form_parses() {
    let source = b"joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = coaxial(a, b)";
    let tree = parse_clean(source);
    let root = tree.root_node();

    let jdef = find_node_by_kind(root, "joint_definition")
        .expect("joint_definition not found");

    // name
    assert_eq!(
        text(jdef.child_by_field_name("name").expect("name field missing"), source),
        "cylindrical"
    );

    // dof: braced (record) variant — has anonymous '{' child and 2 joint_dof_field nodes
    let dof_node = jdef.child_by_field_name("dof").expect("dof field missing");
    assert!(
        has_anonymous_child_text(dof_node, source, "{"),
        "record-form joint_dof must have an anonymous '{{' child"
    );
    let dof_fields = find_all_nodes_by_kind(dof_node, "joint_dof_field");
    assert_eq!(
        dof_fields.len(),
        2,
        "record-form dof must have exactly 2 joint_dof_field children, got {}",
        dof_fields.len()
    );
    assert_eq!(
        text(dof_fields[0].child_by_field_name("name").expect("name field missing on dof[0]"), source),
        "angle"
    );
    assert_eq!(
        text(dof_fields[1].child_by_field_name("name").expect("name field missing on dof[1]"), source),
        "travel"
    );
    assert_eq!(
        text(dof_fields[0].child_by_field_name("type").expect("type field missing on dof[0]"), source),
        "Angle"
    );
    assert_eq!(
        text(dof_fields[1].child_by_field_name("type").expect("type field missing on dof[1]"), source),
        "Length"
    );

    // body: single-expression form
    let body_node = jdef.child_by_field_name("body").expect("body field missing");
    assert!(
        body_node.child_by_field_name("result").is_some(),
        "record-form example must have a single-expr body (result field)"
    );
}

/// Consolidated gate: gr-05b-joint-with-rec.ri (the record form from the PRD α signal)
/// parses with zero ERROR nodes once step-4 lands.
/// RED until step-4.
#[test]
fn gate_gr05b_fixture_parses_clean() {
    assert_fixture_parses_clean("gr-05b-joint-with-rec.ri");
}

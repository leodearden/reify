//! Grammar integration tests for geometric-relations δ (task 4384):
//! parameterized `auto(name = value, …)`, `at auto` pose-binding, member-level
//! `relate { }`, and inline `at … where { }` relate-blocks.
//!
//! TDD structure (mirrors `aux_at_grammar_tests.rs`):
//!   - step-1 (RED): parameterized `auto(seed = …)` / `auto(x = …, …)` parse to
//!     an `auto_keyword` carrying `auto_param_list` / `auto_param` children.
//!     RED until step-2 (grammar adds the auto-param arm).
//!   - step-3 (RED): `at auto` / `at auto(…)` at the sub pose position.
//!   - step-5 (RED): member-level `relate { }` blocks.
//!   - step-7 (RED): inline `sub … at … where { }` relate-blocks.
//!
//! The consolidated gr-01/02/03 fixture gates are asserted in the step after
//! each fixture's last enabling feature (gr-03 in step-3, gr-01 in step-5,
//! gr-02 in step-7), per the plan's distributed-gate design decision.
//!
//! All snippets wrap members in `structure S { … }` so the grammar sees them in
//! a valid declaration context.

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
/// Used by the relate-block / where-block steps (5/7) to assert keyword tokens.
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

// ── Regression baselines: bare `auto` / `auto(free)` (GREEN before and after) ──

/// Baseline: bare `auto` at the param-default binding site parses to an
/// `auto_keyword` with NO `modifier` child and NO `auto_param`. Must stay GREEN.
#[test]
fn baseline_bare_auto_parses() {
    let source = b"structure S { param p : Scalar = auto }";
    let tree = parse_clean(source);
    let root = tree.root_node();
    let auto = find_node_by_kind(root, "auto_keyword")
        .expect("auto_keyword not found for bare `auto`");
    assert!(
        auto.child_by_field_name("modifier").is_none(),
        "bare `auto` must NOT carry a `modifier` field child"
    );
    assert!(
        find_node_by_kind(auto, "auto_param").is_none(),
        "bare `auto` must NOT carry an auto_param child"
    );
}

/// Baseline: `auto(free)` parses to an `auto_keyword` WITH a `modifier` field
/// child and NO `auto_param`. Must stay GREEN (precedence: `free` keyword arm
/// wins over the new `name = value` auto_param arm).
#[test]
fn baseline_auto_free_parses() {
    let source = b"structure S { param p : Scalar = auto(free) }";
    let tree = parse_clean(source);
    let root = tree.root_node();
    let auto = find_node_by_kind(root, "auto_keyword")
        .expect("auto_keyword not found for `auto(free)`");
    assert!(
        auto.child_by_field_name("modifier").is_some(),
        "`auto(free)` must carry a `modifier` field child"
    );
    assert!(
        find_node_by_kind(auto, "auto_param").is_none(),
        "`auto(free)` must NOT carry an auto_param child (free is a bare keyword)"
    );
}

// ── step-1 (RED until step-2): parameterized `auto(name = value, …)` ──────────

/// `auto(seed = 5mm)` at a binding site parses to an `auto_keyword` carrying an
/// `auto_param_list` with one `auto_param` whose `name` field is `seed` and whose
/// `value` field is present. RED: the parameterized form ERRORs on the base
/// grammar (the only `(`-arm is `auto(free)`).
#[test]
fn auto_param_single_seed_parses() {
    let source = b"structure S { param p : Frame = auto(seed = 5mm) }";
    let tree = parse_clean(source);
    let root = tree.root_node();

    let auto = find_node_by_kind(root, "auto_keyword")
        .expect("auto_keyword not found for `auto(seed = 5mm)`");
    assert!(
        find_node_by_kind(auto, "auto_param_list").is_some(),
        "`auto(seed = 5mm)` must carry an auto_param_list; kinds: {:?}",
        collect_kinds(auto)
    );

    let params = find_all_nodes_by_kind(auto, "auto_param");
    assert_eq!(params.len(), 1, "expected exactly 1 auto_param, got: {}", params.len());
    let name = params[0]
        .child_by_field_name("name")
        .expect("auto_param must expose a `name` field");
    assert_eq!(text(name, source), "seed", "auto_param name field must be `seed`");
    assert!(
        params[0].child_by_field_name("value").is_some(),
        "auto_param must expose a `value` field"
    );
}

/// `auto(seed = self.frame)` — the design's seed form referencing a `self`
/// projection — parses with the seed param name and a present value. RED until
/// step-2.
#[test]
fn auto_param_seed_self_frame_parses() {
    let source = b"structure S { param p : Frame = auto(seed = self.frame) }";
    let tree = parse_clean(source);
    let root = tree.root_node();
    let auto = find_node_by_kind(root, "auto_keyword").expect("auto_keyword not found");
    let params = find_all_nodes_by_kind(auto, "auto_param");
    assert_eq!(params.len(), 1, "expected 1 auto_param, got {}", params.len());
    assert_eq!(
        text(params[0].child_by_field_name("name").unwrap(), source),
        "seed"
    );
    assert!(params[0].child_by_field_name("value").is_some());
}

/// `auto(x = 5mm, orientation = orient_identity())` — the multi-param
/// component-fix form — parses to two `auto_param`s in source order with the
/// expected `name` fields. RED until step-2.
#[test]
fn auto_param_multi_component_fix_parses() {
    let source =
        b"structure S { param p : Frame = auto(x = 5mm, orientation = orient_identity()) }";
    let tree = parse_clean(source);
    let root = tree.root_node();

    let auto = find_node_by_kind(root, "auto_keyword").expect("auto_keyword not found");
    assert!(
        find_node_by_kind(auto, "auto_param_list").is_some(),
        "multi-param `auto(...)` must carry an auto_param_list"
    );
    let params = find_all_nodes_by_kind(auto, "auto_param");
    assert_eq!(params.len(), 2, "expected 2 auto_params, got {}", params.len());
    assert_eq!(
        text(params[0].child_by_field_name("name").unwrap(), source),
        "x",
        "first auto_param name must be `x`"
    );
    assert_eq!(
        text(params[1].child_by_field_name("name").unwrap(), source),
        "orientation",
        "second auto_param name must be `orientation`"
    );
    for p in &params {
        assert!(
            p.child_by_field_name("value").is_some(),
            "every auto_param must expose a `value` field"
        );
    }
}

// ── step-3 (RED until step-4): `at auto` / `at auto(…)` at the sub pose ───────

/// `sub b : B at auto` — the bare-auto pose-binding — parses so the
/// sub_declaration's `pose` field child is an `auto_keyword`, NO ERROR. RED:
/// `auto` is not accepted at the pose (an `_expression`) position on the base
/// grammar (the external scanner emits AUTO_TOKEN out-of-valid → ERROR).
#[test]
fn sub_at_auto_bare_parses() {
    let source = b"structure S { sub b : B at auto }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");
    let pose = sub
        .child_by_field_name("pose")
        .expect("sub_declaration must expose a `pose` field for `at auto`");
    assert_eq!(
        pose.kind(),
        "auto_keyword",
        "`at auto` pose field must be an auto_keyword, got {}",
        pose.kind()
    );
}

/// `sub b : B at auto(free)` — pose field is an auto_keyword carrying the
/// `modifier` (free) child. RED until step-4.
#[test]
fn sub_at_auto_free_parses() {
    let source = b"structure S { sub b : B at auto(free) }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");
    let pose = sub.child_by_field_name("pose").expect("pose field missing");
    assert_eq!(pose.kind(), "auto_keyword");
    assert!(
        pose.child_by_field_name("modifier").is_some(),
        "`at auto(free)` pose must carry a `modifier` child"
    );
}

/// `sub b : B at auto(seed = self.frame)` — pose field is an auto_keyword
/// carrying an auto_param_list. RED until step-4.
#[test]
fn sub_at_auto_seed_parses() {
    let source = b"structure S { sub b : B at auto(seed = self.frame) }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");
    let pose = sub.child_by_field_name("pose").expect("pose field missing");
    assert_eq!(pose.kind(), "auto_keyword");
    assert!(
        find_node_by_kind(pose, "auto_param_list").is_some(),
        "`at auto(seed = …)` pose must carry an auto_param_list"
    );
    let params = find_all_nodes_by_kind(pose, "auto_param");
    assert_eq!(params.len(), 1);
    assert_eq!(
        text(params[0].child_by_field_name("name").unwrap(), source),
        "seed"
    );
}

/// Regression: a concrete `at <expr>` pose (e.g. `at frame3(o, b)`) still parses
/// to a `pose` field that is NOT an auto_keyword. GREEN before and after step-4.
#[test]
fn sub_at_concrete_pose_still_parses() {
    let source = b"structure S { sub b : B at frame3(o, b) }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");
    let pose = sub.child_by_field_name("pose").expect("pose field missing");
    assert_ne!(
        pose.kind(),
        "auto_keyword",
        "a concrete `at frame3(...)` pose must NOT lower to auto_keyword"
    );
    assert_eq!(pose.kind(), "function_call");
}

/// Consolidated gate: gr-03-auto-param.ri (only `at auto(...)` forms) parses
/// with zero ERROR nodes once steps 2 + 4 land. RED until step-4.
#[test]
fn gate_gr03_auto_param_fixture_parses_clean() {
    assert_fixture_parses_clean("gr-03-auto-param.ri");
}

// ── step-5 (RED until step-6): member-level `relate { }` block ────────────────

/// A member-level `relate { concentric(a, b)  flush(c, d) }` block (inside
/// `structure S { … }`) parses to a `relate_block` node carrying two
/// `relation_member` children, each whose `expr` field is the relation call,
/// with NO ERROR. RED: `relate` is not a keyword/member on the base grammar, so
/// the block ERRORs.
#[test]
fn relate_block_two_relations_parses() {
    let source = b"structure S { relate { concentric(a, b)  flush(c, d) } }";
    let tree = parse_clean(source);
    let root = tree.root_node();

    let block = find_node_by_kind(root, "relate_block")
        .expect("relate_block not found for member-level `relate { }`");
    let members = find_all_nodes_by_kind(block, "relation_member");
    assert_eq!(
        members.len(),
        2,
        "expected exactly 2 relation_member children, got {}",
        members.len()
    );
    for m in &members {
        let expr = m
            .child_by_field_name("expr")
            .expect("relation_member must expose an `expr` field");
        assert_eq!(
            expr.kind(),
            "function_call",
            "relation_member expr must be a function_call, got {}",
            expr.kind()
        );
    }
    // first relation is `concentric(...)`, second is `flush(...)`
    assert_eq!(
        text(
            members[0].child_by_field_name("expr").unwrap().child_by_field_name("name").unwrap(),
            source
        ),
        "concentric"
    );
    assert_eq!(
        text(
            members[1].child_by_field_name("expr").unwrap().child_by_field_name("name").unwrap(),
            source
        ),
        "flush"
    );
}

/// An empty `relate { }` block also parses to a `relate_block` node with zero
/// `relation_member` children, NO ERROR. RED until step-6.
#[test]
fn relate_block_empty_parses() {
    let source = b"structure S { relate { } }";
    let tree = parse_clean(source);
    let block = find_node_by_kind(tree.root_node(), "relate_block")
        .expect("empty `relate { }` must parse to a relate_block");
    assert_eq!(
        find_all_nodes_by_kind(block, "relation_member").len(),
        0,
        "empty `relate {{ }}` must carry zero relation_member children"
    );
}

/// Consolidated gate: gr-01-at-auto-relate.ri (needs `at auto` from step-4 AND
/// the member-level `relate { }` block) parses with zero ERROR nodes once
/// step-6 lands. RED until step-6.
#[test]
fn gate_gr01_at_auto_relate_fixture_parses_clean() {
    assert_fixture_parses_clean("gr-01-at-auto-relate.ri");
}

// ── step-7 (RED until step-8): inline `at … where { }` relate-block ───────────

/// `sub b : B at auto where { concentric(a, b)  flush(c, d) }` parses so the
/// sub_declaration carries an inline relate-block in its `relations` field — a
/// `sub_relate_block` holding the two relations as `relation_member` children —
/// with NO ERROR. RED: the trailing `at … where { }` block is not accepted on
/// the base grammar (it misparses as a following member-level guarded_block
/// with a MISSING condition).
#[test]
fn sub_inline_where_relate_block_parses() {
    let source = b"structure S { sub b : B at auto where { concentric(a, b)  flush(c, d) } }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");

    let pose = sub.child_by_field_name("pose").expect("pose field missing");
    assert_eq!(pose.kind(), "auto_keyword", "pose must be an auto_keyword");

    let block = sub
        .child_by_field_name("relations")
        .expect("sub_declaration must expose a `relations` field for the inline where-block");
    assert_eq!(
        block.kind(),
        "sub_relate_block",
        "inline relations must be a sub_relate_block, got {}",
        block.kind()
    );
    let members = find_all_nodes_by_kind(block, "relation_member");
    assert_eq!(
        members.len(),
        2,
        "expected 2 relation_member children in the inline where-block, got {}",
        members.len()
    );
    for m in &members {
        assert_eq!(
            m.child_by_field_name("expr")
                .expect("relation_member must expose an `expr` field")
                .kind(),
            "function_call",
            "each inline relation must be a function_call"
        );
    }
}

/// Discrimination regression: the existing `where <expr>` sub guard (no braces)
/// still parses to a `guard` field that is a `where_clause`, and produces NO
/// `sub_relate_block`. GREEN before and after step-8 — guards against step-8
/// stealing the guard's `where`.
#[test]
fn sub_where_clause_guard_still_parses() {
    let source = b"structure S { sub b : B where x > 0mm }";
    let tree = parse_clean(source);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("sub_declaration not found");
    let guard = sub
        .child_by_field_name("guard")
        .expect("sub `where <expr>` guard field missing");
    assert_eq!(
        guard.kind(),
        "where_clause",
        "a `where <expr>` sub guard must remain a where_clause, got {}",
        guard.kind()
    );
    assert!(
        find_node_by_kind(sub, "sub_relate_block").is_none(),
        "a bare `where <expr>` guard must NOT produce a sub_relate_block"
    );
}

/// Discrimination regression: a member-level `where <expr> { }` guarded_block
/// still parses to a `guarded_block` node retaining its `condition` field —
/// distinct from the new conditionless `where { }` relate-block. GREEN before
/// and after step-8.
#[test]
fn member_guarded_block_still_parses() {
    let source = b"structure S { where x > 0mm { } }";
    let tree = parse_clean(source);
    let gb = find_node_by_kind(tree.root_node(), "guarded_block")
        .expect("guarded_block not found for `where <expr> { }`");
    assert!(
        gb.child_by_field_name("condition").is_some(),
        "guarded_block must retain its `condition` field (distinct from conditionless relate-block)"
    );
    assert!(
        find_node_by_kind(gb, "sub_relate_block").is_none(),
        "a member-level guarded_block must NOT be a sub_relate_block"
    );
}

/// Consolidated gate: gr-02-at-auto-where.ri (needs `at auto` + the inline
/// trailing `where { }` relate-block) parses with zero ERROR nodes once step-8
/// lands. RED until step-8. With gr-01/02/03 all GREEN, this completes the
/// user-observable parse-gate signal (PRD §9).
#[test]
fn gate_gr02_at_auto_where_fixture_parses_clean() {
    assert_fixture_parses_clean("gr-02-at-auto-where.ri");
}

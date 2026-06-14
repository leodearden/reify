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

//! Parser-level tests for the decl-level `match { ... => sub head : ... }` block (task 3563).
//!
//! User-observable signal: `cargo test -p reify-syntax -- match_decl_block_parses_from_source`
//! Leaf signal from phase-3-grammar-fiction-triage-log.md §B2.
//!
//! These tests verify that the tree-sitter grammar admits the new `match_arm_decl_block`
//! production and produces a well-formed CST (zero ERROR nodes). AST-shape assertions
//! (lowering to `MatchArmDeclGroupDecl`) are deferred to sibling task 3564.

use reify_core::ModulePath;

mod common;
use common::{find_cst_node, find_outermost_cst_nodes, make_ts_parser};

// ── High-level parse tests (user-observable signal) ─────────────────────────

/// User-signal test: all three forms of decl-level match block parse without
/// errors. Covers union arms, variant-pipe arm, and single (non-exhaustive) arm.
///
/// `!tree.root_node().has_error()` is the primary signal — it confirms the new
/// grammar production is reachable from `_member` with no CST ERROR nodes.
///
/// `module.errors.is_empty()` is a regression guard for the *surrounding*
/// source (the `structure S { ... }` wrapper): it will fire if an unrelated
/// change breaks the surrounding structure parse, but it carries no signal about
/// the `match_arm_decl_block` construct itself, because lowering for that
/// construct is a no-op (silent drop via `lower_member`'s `_ => None`) until
/// sibling task 3564 wires the CST→AST mapping.
#[test]
fn match_decl_block_parses_from_source() {
    let sources: &[(&str, &str)] = &[
        (
            "union arms (two arms)",
            "structure S { match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead } }",
        ),
        (
            "variant-pipe arm",
            "structure S { match head_type { Hex | Button => sub head : RecessedHead, Slider => sub head : SlideHead } }",
        ),
        (
            "single arm (non-exhaustive)",
            "structure S { match head_type { Hex => sub head : HexHead } }",
        ),
    ];

    for (label, source) in sources {
        // Regression guard: asserts the surrounding `structure S { ... }` source
        // still parses cleanly. Does NOT verify match_arm_decl_block itself —
        // lowering is a no-op (silent drop) until sibling task 3564 wires CST→AST.
        let module = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "expected zero parse errors for {label:?} form, got: {:?}",
            module.errors,
        );

        // CST-level check: no ERROR nodes in the parse tree.
        let mut parser = make_ts_parser();
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("tree-sitter parse failed");
        assert!(
            !tree.root_node().has_error(),
            "expected no CST ERROR nodes for {label:?} form; \
             has_error() returned true for source: {source:?}",
        );
    }
}

// ── CST-shape assertions ─────────────────────────────────────────────────────

/// The union-arms form must produce a `match_arm_decl_block` node whose
/// `discriminant` field text is `"head_type"`.
#[test]
fn match_decl_block_cst_has_match_arm_decl_block_node() {
    let source =
        "structure S { match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let block = find_cst_node(tree.root_node(), "match_arm_decl_block")
        .expect("expected a match_arm_decl_block node in the CST");

    let discriminant = block
        .child_by_field_name("discriminant")
        .expect("match_arm_decl_block must have a `discriminant` field");
    let discriminant_text = discriminant
        .utf8_text(source.as_bytes())
        .expect("discriminant node must be valid utf8");
    assert_eq!(
        discriminant_text, "head_type",
        "discriminant field text must be 'head_type', got: {discriminant_text:?}",
    );
}

/// The union-arms form must produce exactly 2 `match_arm_decl_arm` nodes,
/// each carrying a `match_pattern` with exactly one `identifier` child.
#[test]
fn match_decl_block_cst_two_arms_each_carry_one_pattern() {
    let source =
        "structure S { match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let arms = find_outermost_cst_nodes(tree.root_node(), "match_arm_decl_arm");
    assert_eq!(
        arms.len(),
        2,
        "expected 2 match_arm_decl_arm nodes for the union-arms form, got {}",
        arms.len(),
    );

    for (i, arm) in arms.iter().enumerate() {
        let pattern = arm
            .child_by_field_name("pattern")
            .expect("match_arm_decl_arm must have a `pattern` field");
        let idents = find_outermost_cst_nodes(pattern, "identifier");
        assert_eq!(
            idents.len(),
            1,
            "arm[{i}] pattern must have exactly 1 identifier (single variant), got {}",
            idents.len(),
        );
    }
}

/// The variant-pipe form must produce a first arm whose `match_pattern` field
/// contains two `identifier` children with text `"Hex"` and `"Button"`.
#[test]
fn match_decl_block_cst_variant_pipe_arm_carries_multiple_patterns() {
    let source =
        "structure S { match head_type { Hex | Button => sub head : RecessedHead, Slider => sub head : SlideHead } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let arms = find_outermost_cst_nodes(tree.root_node(), "match_arm_decl_arm");
    assert!(
        !arms.is_empty(),
        "expected at least one match_arm_decl_arm node in the CST",
    );

    let first_arm = arms[0];
    let pattern = first_arm
        .child_by_field_name("pattern")
        .expect("match_arm_decl_arm must have a `pattern` field");

    let idents: Vec<&str> = find_outermost_cst_nodes(pattern, "identifier")
        .iter()
        .map(|n| {
            n.utf8_text(source.as_bytes())
                .expect("identifier node must be valid utf8")
        })
        .collect();

    assert_eq!(
        idents,
        ["Hex", "Button"],
        "first arm's pattern identifiers must be ['Hex', 'Button'], got: {idents:?}",
    );
}

// ── Negative grammar test ────────────────────────────────────────────────────
//
// The arm body is required to start with `sub`. Any other keyword or identifier
// in arm-body position must produce a CST ERROR node. This mirrors
// `auto_type_arg_rejects_unrecognized_modifier` from auto_type_arg_tests.rs.
//
// Note: this test operates at the CST level (not via `module.errors`) because
// the lowering pipeline does not necessarily propagate all CST ERROR nodes that
// appear inside structure bodies to `module.errors`.

/// A malformed arm body (`not_a_sub_decl` instead of `sub head : HexHead`)
/// must cause the parser to emit a CST ERROR node.
#[test]
fn match_decl_block_rejects_malformed_arm_body() {
    let source = "structure S { match d { Hex => not_a_sub_decl } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node when the arm body is not a `sub` declaration; \
         the grammar must require `sub name : StructureName` in arm-body position",
    );
}

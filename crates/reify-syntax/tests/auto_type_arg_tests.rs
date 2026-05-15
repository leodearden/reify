//! Tests for `auto:` / `auto(free):` in `type_arg_list` position (task 3526).
//!
//! User-observable signal: `cargo test -p reify-syntax --test auto_type_arg_tests`
//! passes.  The load-bearing coverage is in the CST-level bound-identifier tests
//! (`auto_type_arg_cst_bound_identifier_strict`, `_multi_param`) and the
//! negative-coverage test `auto_type_arg_rejects_unrecognized_modifier`.
//!
//! A small `parse_pipeline_smoke_auto_type_arg` test pins that
//! `reify_syntax::parse` returns without panicking on `auto: Seal` in type-arg
//! position.  It cannot detect grammar regressions on its own — the lowering
//! pipeline does not propagate CST ERROR nodes from return-type expressions
//! into `module.errors`.  See the doc comment on
//! `auto_type_arg_rejects_unrecognized_modifier` for detail.
//!
//! AST-shape assertions (e.g. the bound identifier is surfaced in TypeExprKind)
//! are deferred to sibling task 3477, which wires the lowering extension.

use reify_types::ModulePath;

mod common;
use common::{find_cst_node, find_outermost_cst_nodes, make_ts_parser};

// ── Parse-pipeline smoke check ──────────────────────────────────────────────

#[test]
fn parse_pipeline_smoke_auto_type_arg() {
    // No assertion beyond "does not panic": the lowering pipeline does not
    // propagate CST ERROR nodes from return-type expressions into module.errors,
    // so a meaningful grammar-regression check has to live at the CST level.
    // See `auto_type_arg_rejects_unrecognized_modifier` for that load-bearing guard.
    let _ = reify_syntax::parse(
        "fn f() -> Bearing<auto: Seal> { 0 }",
        ModulePath::single("test"),
    );
}

// ── Suggestion #1: strict vs free modifier discrimination ────────────────────
//
// The corpus S-expression format hides anonymous string nodes, so both
// `auto:` and `auto(free):` produce identical `(auto_keyword)` S-expressions
// — meaning the corpus test alone cannot verify that the `(free)` modifier
// was actually consumed by the parser.  These CST-level tests guard that gap.

/// Bare `auto:` must produce an `auto_keyword` node with NO `modifier` field.
#[test]
fn auto_type_arg_cst_strict_has_no_modifier_field() {
    let source = "fn f() -> Bearing<auto: Seal> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected an auto_keyword node in the CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "bare `auto:` should have no `modifier` field child on auto_keyword; \
         found: {:?}",
        kw.child_by_field_name("modifier").map(|n| n.kind()),
    );
}

/// `auto(free):` must produce an `auto_keyword` node whose `modifier` field
/// child has text `"free"`.
#[test]
fn auto_type_arg_cst_free_has_modifier_field_with_text_free() {
    let source = "fn g() -> Bearing<auto(free): Seal> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected an auto_keyword node in the CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("`auto(free):` must have a `modifier` field child on auto_keyword");
    let modifier_text = modifier
        .utf8_text(source.as_bytes())
        .expect("modifier node must be valid utf8");
    assert_eq!(
        modifier_text, "free",
        "`auto(free):` modifier field must have text 'free', got: {modifier_text:?}",
    );
}

// ── Suggestion #2: bound-identifier assertions ───────────────────────────────
//
// The high-level parse test above only checks `errors.is_empty()`.
// If the grammar accidentally dropped `auto_type_arg` from `type_arg_list`
// but still parsed the surrounding `fn` cleanly, it would silently pass.
// These CST-level tests verify that the `auto_type_arg` node is actually
// produced and carries the correct bound identifier text.

/// Single `auto: Seal` — the CST must contain an `auto_type_arg` node whose
/// `bound` field text is `"Seal"`.
#[test]
fn auto_type_arg_cst_bound_identifier_strict() {
    let source = "fn f() -> Bearing<auto: Seal> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    let node = find_cst_node(tree.root_node(), "auto_type_arg")
        .expect("expected an auto_type_arg node in the CST");
    let bound = node
        .child_by_field_name("bound")
        .expect("auto_type_arg must have a `bound` field");
    let bound_text = bound
        .utf8_text(source.as_bytes())
        .expect("bound node must be valid utf8");
    assert_eq!(
        bound_text, "Seal",
        "bound identifier must be 'Seal', got: {bound_text:?}",
    );
}

/// Multi-param `auto: A, auto: B` — the CST must contain exactly two
/// `auto_type_arg` nodes with bound identifiers `"A"` and `"B"`.
#[test]
fn auto_type_arg_cst_bound_identifiers_multi_param() {
    let source = "fn h() -> Coupling<auto: A, auto: B> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    let nodes = find_outermost_cst_nodes(tree.root_node(), "auto_type_arg");
    assert_eq!(
        nodes.len(),
        2,
        "expected 2 auto_type_arg nodes for `auto: A, auto: B`, got {}",
        nodes.len(),
    );

    let bounds: Vec<&str> = nodes
        .iter()
        .map(|n| {
            n.child_by_field_name("bound")
                .expect("auto_type_arg must have a `bound` field")
                .utf8_text(source.as_bytes())
                .expect("bound node must be valid utf8")
        })
        .collect();
    assert_eq!(
        bounds,
        ["A", "B"],
        "bound identifiers must be ['A', 'B'] (in order), got: {bounds:?}",
    );
}

// ── Suggestion #3: negative coverage — unrecognized modifier ─────────────────
//
// The grammar hard-codes `free` as the only accepted modifier inside `auto(…)`.
// This mirrors the spirit of `parse_auto_unrecognized_modifier_is_error` in
// `boundary1_producer.rs` (which guards the param-default position) for the
// type-arg position.  If someone widens `auto_keyword` to accept arbitrary
// identifiers, this test will fail and force an explicit decision.
//
// Note: this test operates at the CST level rather than via `module.errors`.
// The `reify_syntax` lowering pipeline does not propagate CST ERROR nodes that
// appear inside function return-type expressions to `module.errors` (that gap
// is in `ts_parser.rs`, outside this task's scope).  Using the tree-sitter API
// directly is the correct layer for a grammar-layer regression guard.

/// `auto(constrained): Seal` must produce a CST ERROR node in type-arg position.
///
/// The span overlap check ensures the error is attributed to the
/// `(constrained)` portion of the token, not an unrelated part of the source.
/// Mirrors the `boundary1_producer.rs` guard for the param-default position.
#[test]
fn auto_type_arg_rejects_unrecognized_modifier() {
    let source = "fn f() -> Bearing<auto(constrained): Seal> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    // The grammar must reject `auto(constrained)` with an ERROR node.
    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node for `auto(constrained):` in type-arg position; \
         the grammar should only accept `free` as the auto modifier",
    );

    // The ERROR span must overlap the `(constrained)` portion of the token.
    // Using `str::find` avoids hard-coded byte offsets that become stale
    // when the source fixture changes.
    let error_node = find_cst_node(tree.root_node(), "ERROR")
        .expect("expected at least one ERROR node when has_error() is true");
    let token = "(constrained)";
    let token_start = source
        .find(token)
        .expect("fixture must contain '(constrained)'") as u32;
    let token_end = token_start + token.len() as u32;
    let error_start = error_node.start_byte() as u32;
    let error_end = error_node.end_byte() as u32;
    assert!(
        error_start < token_end && error_end > token_start,
        "expected ERROR node to overlap `(constrained)` \
         (bytes {token_start}..{token_end}), got error at {error_start}..{error_end}",
    );
}

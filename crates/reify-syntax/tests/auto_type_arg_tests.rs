//! Tests for `auto:` / `auto(free):` in `type_arg_list` position (task 3526).
//!
//! User-observable signal: `cargo test -p reify-syntax --test auto_type_arg_tests`
//! passes.  The load-bearing coverage spans three layers:
//!
//! * **CST level** — `auto_type_arg_cst_bound_identifier_strict`, `_multi_param`,
//!   `auto_type_arg_cst_strict_has_no_modifier_field`,
//!   `auto_type_arg_cst_free_has_modifier_field_with_text_free`, and the
//!   grammar-layer negative test `auto_type_arg_rejects_unrecognized_modifier`.
//!
//! * **AST level** — `auto_type_arg_lowers_to_ast_strict`, `_free`, and
//!   `auto_type_arg_multi_param_lowers_both` (task 3665: `TypeExprKind::Auto`
//!   variant + `lower_type_args_from_node` extension).
//!
//! * **Error propagation** — `auto_type_arg_cst_error_propagates_to_module_errors`
//!   and `auto_type_arg_clean_input_has_no_spurious_errors` (task 3665: CST ERROR
//!   nodes inside `type_arg_list` are now surfaced in `module.errors`).
//!
//! Task 3662 references task 3665 as its gate for parse-level free/multi-param
//! coverage; the lowering extension (Auto variant + ERROR propagation) landed here.

use reify_types::ModulePath;

mod common;
use common::{find_cst_node, find_outermost_cst_nodes, make_ts_parser};

// ── Parse-pipeline smoke check ──────────────────────────────────────────────

#[test]
fn parse_pipeline_smoke_auto_type_arg() {
    // Keeps the "does not panic" contract for well-formed auto type-args.
    // Richer assertions (Auto variant, error propagation) live in the step-1 /
    // step-3 tests below.  The grammar-layer negative test is
    // `auto_type_arg_rejects_unrecognized_modifier`.
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
// This CST-level test guards that the grammar PRODUCES an ERROR node.  The
// higher-level test `auto_type_arg_cst_error_propagates_to_module_errors`
// (task 3665, step-3) guards that the lowering pipeline propagates that ERROR
// into `module.errors`.  Both layers are complementary.

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

// ── Step-1/2 (task 3665): TypeExprKind::Auto variant — GREEN ────────────────
//
// Added in step-1 as non-compiling RED tests (TypeExprKind::Auto was absent);
// turned GREEN in step-2 when the variant was added to lib.rs and
// lower_type_args_from_node was extended to handle auto_type_arg children.

/// Helper: parse `source`, find the single Function declaration, and return its
/// return-type's `type_args` slice. Panics with a clear message if any step fails.
fn get_return_type_args(source: &str) -> Vec<reify_syntax::TypeExpr> {
    let m = reify_syntax::parse(source, ModulePath::single("t"));
    assert!(
        m.errors.is_empty(),
        "parse of {:?} produced unexpected errors: {:?}",
        source,
        m.errors
    );
    let decls = &m.declarations;
    assert_eq!(decls.len(), 1, "expected exactly one declaration in {:?}", source);
    if let reify_syntax::Declaration::Function(f) = &decls[0] {
        let rt = f.return_type.as_ref().expect("expected a return type");
        if let reify_syntax::TypeExprKind::Named { type_args, .. } = &rt.kind {
            type_args.clone()
        } else {
            panic!("expected outer type to be Named, got {:?}", rt.kind);
        }
    } else {
        panic!("expected Function declaration, got {:?}", decls[0]);
    }
}

/// Strict `auto: Seal` in type-arg position must lower to
/// `TypeExprKind::Auto { free: false, bound: "Seal" }`.
#[test]
fn auto_type_arg_lowers_to_ast_strict() {
    let args = get_return_type_args("fn f() -> Bearing<auto: Seal> { 0 }");
    assert_eq!(args.len(), 1, "expected exactly one type argument");
    match &args[0].kind {
        reify_syntax::TypeExprKind::Auto { free, bound } => {
            assert!(!free, "strict auto: Seal must have free=false");
            assert_eq!(bound, "Seal", "bound must be 'Seal'");
        }
        other => panic!("expected TypeExprKind::Auto, got {:?}", other),
    }
}

/// Free `auto(free): Seal` in type-arg position must lower to
/// `TypeExprKind::Auto { free: true, bound: "Seal" }`.
#[test]
fn auto_type_arg_lowers_to_ast_free() {
    let args = get_return_type_args("fn g() -> Bearing<auto(free): Seal> { 0 }");
    assert_eq!(args.len(), 1, "expected exactly one type argument");
    match &args[0].kind {
        reify_syntax::TypeExprKind::Auto { free, bound } => {
            assert!(*free, "free auto(free): Seal must have free=true");
            assert_eq!(bound, "Seal", "bound must be 'Seal'");
        }
        other => panic!("expected TypeExprKind::Auto, got {:?}", other),
    }
}


// ── Step-3 (task 3665): RED tests — ERROR propagation into module.errors ────────
//
// AC#1: a CST ERROR node inside a type_arg_list must surface in module.errors.
// These tests drive the implementation in step-4 (lower_type_args_from_node).

/// `auto(constrained): Seal` contains an unrecognised modifier; the grammar
/// produces an ERROR node inside the `type_arg_list` subtree.  The lowering
/// pipeline (task 3665, step-4) scans the subtree and pushes at least one
/// entry to `module.errors`.
#[test]
fn auto_type_arg_cst_error_propagates_to_module_errors() {
    let source = "fn f() -> Bearing<auto(constrained): Seal> { 0 }";
    let m = reify_syntax::parse(source, ModulePath::single("t"));
    assert!(
        !m.errors.is_empty(),
        "expected at least one parse error for `auto(constrained):` in type-arg position; \
         module.errors was empty — the ERROR node is not being propagated from the \
         type_arg_list subtree into module.errors"
    );
    // The error message must mention the nature of the problem.
    let has_relevant_message = m.errors.iter().any(|e| {
        e.message.contains("type arg") || e.message.contains("syntax error") || e.message.contains("ERROR")
    });
    assert!(
        has_relevant_message,
        "expected an error message mentioning 'type arg', 'syntax error', or 'ERROR'; \
         got: {:?}",
        m.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Well-formed `auto:` type-arg inputs must produce ZERO parse errors (negative
/// guard — the ERROR subtree scan in `lower_type_args_from_node` must not
/// false-positive on valid inputs).
#[test]
fn auto_type_arg_clean_input_has_no_spurious_errors() {
    for source in &[
        "fn f() -> Bearing<auto: Seal> { 0 }",
        "fn h() -> Coupling<auto: A, auto: B> { 0 }",
    ] {
        let m = reify_syntax::parse(source, ModulePath::single("t"));
        assert!(
            m.errors.is_empty(),
            "expected no parse errors for well-formed input {:?}; got: {:?}",
            source,
            m.errors,
        );
    }
}

// ── Step-5 (task 3665): multi-param + no-regression ─────────────────────────
//
// These tests cover the remaining gaps called out in the task description:
// (1) multi-param auto type-args lower both entries correctly, and
// (2) pre-existing Named/IntegerLiteral lowering is not broken by the new branches.
// Both should pass after steps 2+4 with no new production code.

/// Multi-param `auto: A, auto: B` in type-arg position must lower to exactly
/// `[Auto{free:false, bound:"A"}, Auto{free:false, bound:"B"}]`.
///
/// Covers the task-description "multi-param `auto: A, auto: B` shape are invisible
/// to callers/tests" gap.
#[test]
fn auto_type_arg_multi_param_lowers_both() {
    let args = get_return_type_args("fn h() -> Coupling<auto: A, auto: B> { 0 }");
    assert_eq!(args.len(), 2, "expected exactly two type arguments");
    match &args[0].kind {
        reify_syntax::TypeExprKind::Auto { free, bound } => {
            assert!(!free, "first arg must have free=false");
            assert_eq!(bound, "A", "first arg bound must be 'A'");
        }
        other => panic!("expected TypeExprKind::Auto for first arg, got {:?}", other),
    }
    match &args[1].kind {
        reify_syntax::TypeExprKind::Auto { free, bound } => {
            assert!(!free, "second arg must have free=false");
            assert_eq!(bound, "B", "second arg bound must be 'B'");
        }
        other => panic!("expected TypeExprKind::Auto for second arg, got {:?}", other),
    }
}

/// Pre-existing Named and IntegerLiteral type-arg lowering must be unaffected
/// by the new auto_type_arg and ERROR-scan branches.
///
/// Regression guard: if the new branches accidentally change control flow for
/// well-known arg shapes (e.g. early return from the loop), these assertions
/// will catch it.
#[test]
fn mixed_type_args_unaffected() {
    // Named type arguments (e.g. Map<String, Int>)
    let args = get_return_type_args("fn f() -> Map<String, Int> { 0 }");
    assert_eq!(args.len(), 2, "Map<String, Int> must have 2 type args");
    match &args[0].kind {
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "String");
            assert!(type_args.is_empty());
        }
        other => panic!("expected Named for 'String', got {:?}", other),
    }
    match &args[1].kind {
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Int");
            assert!(type_args.is_empty());
        }
        other => panic!("expected Named for 'Int', got {:?}", other),
    }

    // IntegerLiteral type arguments (e.g. Tensor<2, 3, MomentOfInertia>)
    let args2 = get_return_type_args("fn g() -> Tensor<2, 3, MomentOfInertia> { 0 }");
    assert_eq!(args2.len(), 3, "Tensor<2, 3, MomentOfInertia> must have 3 type args");
    match &args2[0].kind {
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            assert_eq!(*n, 2u32, "first arg must be 2");
        }
        other => panic!("expected IntegerLiteral(2), got {:?}", other),
    }
    match &args2[1].kind {
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            assert_eq!(*n, 3u32, "second arg must be 3");
        }
        other => panic!("expected IntegerLiteral(3), got {:?}", other),
    }
    match &args2[2].kind {
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "MomentOfInertia");
            assert!(type_args.is_empty());
        }
        other => panic!("expected Named for 'MomentOfInertia', got {:?}", other),
    }
}

// ── Display impl for TypeExprKind::Auto (task 3665 amendment) ───────────────
//
// The Display output is user-facing: it appears in compiler diagnostics via
// format!("... {}", field_def.codomain_type) in functions.rs and expr.rs.
// These tests guard against regressions in the format string without requiring
// a full parse round-trip.

/// Strict `auto: Seal` — Display must render `"auto: Seal"`.
#[test]
fn auto_type_expr_display_strict() {
    use reify_types::SourceSpan;
    let te = reify_syntax::TypeExpr {
        kind: reify_syntax::TypeExprKind::Auto {
            free: false,
            bound: "Seal".to_string(),
        },
        span: SourceSpan::empty(0),
    };
    assert_eq!(
        format!("{}", te),
        "auto: Seal",
        "strict Auto Display must be 'auto: Seal'"
    );
}

/// Free `auto(free): Seal` — Display must render `"auto(free): Seal"`.
#[test]
fn auto_type_expr_display_free() {
    use reify_types::SourceSpan;
    let te = reify_syntax::TypeExpr {
        kind: reify_syntax::TypeExprKind::Auto {
            free: true,
            bound: "Seal".to_string(),
        },
        span: SourceSpan::empty(0),
    };
    assert_eq!(
        format!("{}", te),
        "auto(free): Seal",
        "free Auto Display must be 'auto(free): Seal'"
    );
}

// ── Task 3725: span narrowing for malformed type_arg_list ─────────────────────
//
// When a `type_arg_list` has_error(), the diagnostic span must point at the
// first ERROR/MISSING descendant node — NOT at the whole type_arg_list.  This
// prevents the diagnostic from covering well-formed sibling arguments.

/// When a nested `type_arg_list` contains a malformed interior (e.g. `Vec<,>`)
/// alongside a well-formed sibling (e.g. `String`), the diagnostic span emitted
/// by `lower_type_args_from_node` must lie strictly inside the malformed
/// portion and must NOT extend into the well-formed sibling.
///
/// Fixture: `fn f() -> Map<Vec<,>, String> { 0 }`.
/// Malformed portion: `Vec<,>` — the inner `type_arg_list` `<,>` contains a
/// bare comma which tree-sitter surfaces as an ERROR/MISSING descendant.
/// Well-formed sibling: `String`.
///
/// RED state (before step-2): `lower_type_args_from_node` emits
/// `self.span(child)` for the whole outer `type_arg_list` `<Vec<,>, String>`,
/// whose end byte extends past `String` — the narrow-span assertion fails.
///
/// GREEN state (after step-2): `first_error_or_missing_descendant` finds the
/// first ERROR/MISSING node inside the malformed portion and uses its (narrow)
/// span — the assertion passes.
#[test]
fn type_arg_list_error_span_narrows_to_first_error_descendant() {
    let source = "fn f() -> Map<Vec<,>, String> { 0 }";
    let m = reify_syntax::parse(source, ModulePath::single("t"));
    assert!(
        !m.errors.is_empty(),
        "expected at least one parse error for malformed `Vec<,>` in type-arg position; \
         module.errors was empty"
    );
    // Compute byte offsets via str::find — avoids hard-coded numbers that go
    // stale when the fixture changes.  Mirrors the pattern in
    // auto_type_arg_tests.rs:168 (`auto_type_arg_rejects_unrecognized_modifier`).
    let malformed_start = source
        .find("Vec<")
        .expect("fixture must contain 'Vec<'") as u32;
    let well_formed_start = source
        .find("String")
        .expect("fixture must contain 'String'") as u32;
    // At least one diagnostic must be *strictly within* the malformed `Vec<,>`
    // region — starting at or after `malformed_start` AND ending before the
    // well-formed `String` sibling.
    //
    // The two-sided check prevents false passes from a degenerate span that
    // merely happens to end before `String` (e.g. a span anchored at byte 0
    // that covers the whole preamble) or one that starts after `Vec<` but
    // extends into `String`.
    //
    // Before step-2: `lower_type_args_from_node` emits `self.span(child)` for
    // the whole `<Vec<,>, String>` type_arg_list — end byte lies past `String`,
    // so the assertion fails.
    //
    // After step-2: `first_error_or_missing_descendant` finds the first
    // ERROR/MISSING descendant inside `Vec<,>` — span is confined to that
    // region, satisfying both bounds.
    let has_narrow_span = m.errors.iter().any(|e| {
        e.span.start >= malformed_start && e.span.end <= well_formed_start
    });
    assert!(
        has_narrow_span,
        "expected at least one diagnostic span to lie within the malformed \
         `Vec<,>` region (bytes {malformed_start}..{well_formed_start}); \
         got errors: {:?}",
        m.errors
            .iter()
            .map(|e| (&e.message, e.span.start, e.span.end))
            .collect::<Vec<_>>(),
    );
}


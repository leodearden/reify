//! Tests for `fn_param` default-value expressions (task 3449).
//!
//! User-observable signal: `cargo test -p reify-syntax --test fn_param_default_tests`
//! passes.
//!
//! Two test tiers:
//! 1. **CST-level** (via `tree_sitter::Parser` directly) — catches grammar
//!    regressions independently of the lowering pipeline.
//! 2. **AST-level** (via `reify_syntax::parse`) — catches lowering regressions
//!    and verifies that `FnParam.default` round-trips the source expression.
//!
//! Design decision: `fn_param` defaults accept `$._expression` only — NOT
//! `choice($.auto_keyword, $._expression)` like `param_declaration` does.
//! Function parameters are call-site provided, not solver-determined.

// ── CST-level helpers ────────────────────────────────────────────────────────

/// Build a tree-sitter parser loaded with the Reify grammar.
fn make_ts_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first search — returns the first node with the given kind.
fn find_cst_node<'a>(root: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if root.kind() == kind {
        return Some(root);
    }
    let mut cursor = root.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_cst_node(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

// ── CST-level tests ───────────────────────────────────────────────────────────

/// `fn f(x : T = Foo.bar) -> T { x }` — CST must contain an `fn_param` node
/// with a `default` field child whose kind is `member_access`.
#[test]
fn fn_param_cst_with_default_has_default_field_child() {
    let source = "fn f(x : T = Foo.bar) -> T { x }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected no CST errors for `fn f(x : T = Foo.bar) -> T {{ x }}`; \
         grammar does not yet accept fn_param default values",
    );

    let param = find_cst_node(tree.root_node(), "fn_param")
        .expect("expected an fn_param node in the CST");

    let default_node = param
        .child_by_field_name("default")
        .expect("fn_param with default must have a `default` field child");

    assert_eq!(
        default_node.kind(),
        "member_access",
        "default field child for `Foo.bar` must be `member_access`, got: {:?}",
        default_node.kind(),
    );
}

/// `fn f(x : T) -> T { x }` — CST must contain an `fn_param` node with NO
/// `default` field child (regression guard: the optional path still parses).
#[test]
fn fn_param_cst_without_default_has_no_default_field_child() {
    let source = "fn f(x : T) -> T { x }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected no CST errors for `fn f(x : T) -> T {{ x }}`",
    );

    let param = find_cst_node(tree.root_node(), "fn_param")
        .expect("expected an fn_param node in the CST");

    assert!(
        param.child_by_field_name("default").is_none(),
        "fn_param without default must have no `default` field child; \
         found: {:?}",
        param.child_by_field_name("default").map(|n| n.kind()),
    );
}

// ── AST-level tests ───────────────────────────────────────────────────────────

use reify_syntax::*;

/// `fn f(x : T = Foo.bar) -> T { x }` — AST must carry `FnParam.default`
/// as `Some(Expr { kind: ExprKind::MemberAccess { .. }, .. })`.
#[test]
fn fn_param_ast_with_default_carries_lowered_expr() {
    let source = "fn f(x : T = Foo.bar) -> T { x }";
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("test"));

    assert!(
        module.errors.is_empty(),
        "expected no parse errors for `{source}`; got: {:?}",
        module.errors,
    );

    let fn_def = match module.declarations.as_slice() {
        [Declaration::Function(f)] => f,
        other => panic!("expected a single function declaration; got: {other:?}"),
    };

    assert_eq!(fn_def.params.len(), 1, "expected exactly 1 fn_param");

    let default = fn_def.params[0]
        .default
        .as_ref()
        .expect("fn_param[0].default must be Some for `x : T = Foo.bar`");

    assert!(
        matches!(&default.kind, ExprKind::MemberAccess { .. }),
        "default expr for `Foo.bar` must be ExprKind::MemberAccess; got: {:?}",
        default.kind,
    );
}

/// `fn f(x : T) -> T { x }` — AST must carry `FnParam.default` as `None`.
#[test]
fn fn_param_ast_without_default_is_none() {
    let source = "fn f(x : T) -> T { x }";
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("test"));

    assert!(
        module.errors.is_empty(),
        "expected no parse errors for `{source}`; got: {:?}",
        module.errors,
    );

    let fn_def = match module.declarations.as_slice() {
        [Declaration::Function(f)] => f,
        other => panic!("expected a single function declaration; got: {other:?}"),
    };

    assert_eq!(fn_def.params.len(), 1, "expected exactly 1 fn_param");

    assert!(
        fn_def.params[0].default.is_none(),
        "fn_param[0].default must be None when no default is present",
    );
}

/// `fn f(x : T = 1, y : U) -> T { x }` — multi-param mix: first param has
/// default, second does not.  Verifies param order is preserved.
#[test]
fn fn_param_ast_multi_param_mixed_defaults() {
    let source = "fn f(x : T = 1, y : U) -> T { x }";
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("test"));

    assert!(
        module.errors.is_empty(),
        "expected no parse errors for `{source}`; got: {:?}",
        module.errors,
    );

    let fn_def = match module.declarations.as_slice() {
        [Declaration::Function(f)] => f,
        other => panic!("expected a single function declaration; got: {other:?}"),
    };

    assert_eq!(fn_def.params.len(), 2, "expected exactly 2 fn_params");

    assert!(
        fn_def.params[0].default.is_some(),
        "fn_param[0] (`x : T = 1`) must have Some default",
    );
    assert!(
        fn_def.params[1].default.is_none(),
        "fn_param[1] (`y : U`) must have None default",
    );
}

// ── Negative coverage test ────────────────────────────────────────────────────

/// `fn f(x = 1) -> T { x }` (default WITHOUT type annotation) must produce a
/// CST ERROR node — the `:` separator and type_expr remain mandatory.
///
/// This is a regression guard ensuring the optional default clause does NOT
/// make the colon-and-type optional.  The grammar keeps `:` and `type_expr`
/// inside `seq(...)` before the `optional(seq('=', ...))` clause.
#[test]
fn fn_param_rejects_default_without_type() {
    let source = "fn f(x = 1) -> T { x }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node for `fn f(x = 1) -> T {{ x }}`; \
         the grammar requires `:` and a type_expr before the optional default",
    );

    let error_node = find_cst_node(tree.root_node(), "ERROR")
        .expect("expected at least one ERROR node when has_error() is true");

    let token = "= 1";
    let token_start = source
        .find(token)
        .expect("fixture must contain '= 1'") as u32;
    let token_end = token_start + token.len() as u32;
    let error_start = error_node.start_byte() as u32;
    let error_end = error_node.end_byte() as u32;
    assert!(
        error_start < token_end && error_end > token_start,
        "expected ERROR node to overlap `= 1` (bytes {token_start}..{token_end}), \
         got error at {error_start}..{error_end}",
    );
}

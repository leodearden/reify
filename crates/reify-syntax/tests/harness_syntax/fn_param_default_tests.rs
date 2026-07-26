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

use reify_ast::*;

/// `fn f(x : T = Foo.bar) -> T { x }` — AST must carry `FnParam.default`
/// as `Some(Expr { kind: ExprKind::MemberAccess { .. }, .. })`.
#[test]
fn fn_param_ast_with_default_carries_lowered_expr() {
    let source = "fn f(x : T = Foo.bar) -> T { x }";
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));

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
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));

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
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));

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
///
/// Two-tier contract:
/// 1. CST: `tree.root_node().has_error()` — the grammar rejects `x = 1` without a type.
/// 2. AST: the parse module either reports errors, or any recovered function's
///    params all have `default.is_none()` — the lowering pipeline never
///    attaches a default expression to a param that has no valid type annotation.
#[test]
fn fn_param_rejects_default_without_type() {
    let source = "fn f(x = 1) -> T { x }";

    // Tier 1: CST must report an error node.
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node for `fn f(x = 1) -> T {{ x }}`; \
         the grammar requires `:` and a type_expr before the optional default",
    );

    // Tier 2: AST-level invariant — either the module reports parse errors, or
    // any recovered function declaration has no param with a default expression.
    // This directly captures the "`:`+type stays mandatory; the optional default
    // does not make the colon/type optional" invariant at the lowering layer.
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    let recovered_fn_has_default = module
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Function(f) => Some(f),
            _ => None,
        })
        .any(|f| f.params.iter().any(|p| p.default.is_some()));
    assert!(
        !module.errors.is_empty() || !recovered_fn_has_default,
        "AST invariant violated: source `fn f(x = 1)` should either produce parse \
         errors or produce no param with a default; got errors={:?}, \
         recovered_fn_has_default={}",
        module.errors,
        recovered_fn_has_default,
    );
}

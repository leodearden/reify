//! Integration tests for task 3918: `undef` as a first-class expression literal.
//!
//! ## Structure
//!
//! - **CST section (step-1 RED / step-2 GREEN):** Verify that the grammar emits
//!   an `undef_literal` CST node (not `identifier`) for `= undef` at a param
//!   default site and for `thickness * undef` at a binary operand site.
//!
//! - **Lowering section (step-3 RED / step-4 GREEN):** Added in step-3 once
//!   `ExprKind::Undef` exists. Verifies that the AST lowerer maps `undef_literal`
//!   CST nodes to `ExprKind::Undef`.
//!
//! User-observable signal: `cargo test -p reify-syntax --test undef_literal_tests`

mod common;
use common::{find_cst_node, make_ts_parser};

// ── CST section ──────────────────────────────────────────────────────────────

/// `param t : Length = undef` must produce an `undef_literal` CST node at the
/// default site.
///
/// On base (before grammar change) this fails: the grammar emits `(identifier)`.
/// After step-2 grammar impl this passes.
#[test]
fn param_default_undef_produces_undef_literal_node() {
    let source = "structure S { param t : Length = undef }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    // Grammar must produce no ERROR nodes.
    assert!(
        !tree.root_node().has_error(),
        "expected no parse error in: {source:?}"
    );

    // There must be an `undef_literal` node in the CST.
    assert!(
        find_cst_node(tree.root_node(), "undef_literal").is_some(),
        "expected an `undef_literal` CST node for `= undef`; \
         got only an identifier (silent-degradation bug still present)"
    );
}

/// `thickness * undef` must produce an `undef_literal` CST node at the right
/// operand of the binary expression.
///
/// On base (before grammar change) this fails: the grammar emits `(identifier)`.
/// After step-2 grammar impl this passes.
#[test]
fn binary_right_operand_undef_produces_undef_literal_node() {
    let source = "structure S { let a : Length = thickness * undef }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");

    assert!(
        !tree.root_node().has_error(),
        "expected no parse error in: {source:?}"
    );

    assert!(
        find_cst_node(tree.root_node(), "undef_literal").is_some(),
        "expected an `undef_literal` CST node for `thickness * undef`; \
         got only an identifier (silent-degradation bug still present)"
    );
}

// ── Lowering section ─────────────────────────────────────────────────────────

use reify_ast::*;
use reify_core::ModulePath;

/// `let a : Length = undef` must lower the value expression to `ExprKind::Undef`.
///
/// Fails to compile on base (variant `ExprKind::Undef` does not exist) — valid RED.
/// After step-4 (AST variant + ts_parser lowering) this passes.
#[test]
fn let_value_undef_lowers_to_expr_kind_undef() {
    let source = "structure S { let a : Length = undef }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert!(
        matches!(let_decl.value.kind, ExprKind::Undef),
        "expected ExprKind::Undef for `let a = undef`, got {:?}",
        let_decl.value.kind
    );
}

/// `let a : Length = 5 * undef` — the right operand must lower to `ExprKind::Undef`.
///
/// Fails to compile on base (variant `ExprKind::Undef` does not exist) — valid RED.
/// After step-4 this passes.
#[test]
fn binary_right_operand_undef_lowers_to_expr_kind_undef() {
    let source = "structure S { let a : Length = 5 * undef }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    let right = match &let_decl.value.kind {
        ExprKind::BinOp { right, .. } => right.as_ref(),
        other => panic!("expected BinOp, got {:?}", other),
    };
    assert!(
        matches!(right.kind, ExprKind::Undef),
        "expected ExprKind::Undef for the right operand of `5 * undef`, got {:?}",
        right.kind
    );
}

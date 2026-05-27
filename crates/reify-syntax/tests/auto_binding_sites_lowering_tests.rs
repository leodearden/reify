//! AST-level companion to `auto_binding_sites_grammar_tests.rs` (CST-level).
//!
//! ## Coverage
//!
//! This file tests that `auto_keyword` CST nodes at binding sites are correctly
//! lowered to `ExprKind::Auto { free: bool }` AST nodes. It covers the four
//! AST-observable binding sites (in order of the grammar's `_binding_value` rule):
//!
//! 1. **`let_declaration.value`** — covered here.
//! 2. **`named_argument.value`** — covered here.
//! 3. **`connect_param_assignment.value`** — covered here.
//! 4. **`param_declaration.default`** — already covered in
//!    `boundary1_producer.rs::parse_auto_param` / `parse_auto_free_param` /
//!    `parse_mixed_auto_and_auto_free`; no new tests needed here.
//! 5. **`param_assignment.value`** — no AST snapshot in β (task 3804) because
//!    no `MemberDecl` variant exists for `param_assignment` yet; that is
//!    deferred to γ = task 3806 which adds the sub-instance-override
//!    end-to-end. CST-level coverage lives in
//!    `auto_binding_sites_grammar_tests.rs::param_assignment_auto_strict_produces_auto_keyword`
//!    and `param_assignment_auto_free_has_modifier_field`.
//!
//! ## Test naming convention
//!
//! `<site>_auto_<flavor>_lowers_to_expr_kind_auto_<expected_free>`
//!
//! where `<flavor>` is `strict` (bare `auto`) or `free` (`auto(free)`).

use reify_syntax::*;
use reify_types::ModulePath;

// ── Site-specific helpers ─────────────────────────────────────────────────────

/// Assert that the first `let` member value in `source` lowers to
/// `ExprKind::Auto { free: expected_free }`.
///
/// Callers supply the full source string; the convention is
/// `"structure S { let m : Length = <auto-expr> }"`.
fn assert_let_value_auto(source: &str, expected_free: bool) {
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
    match let_decl.value.kind {
        ExprKind::Auto { free } => assert_eq!(
            free, expected_free,
            "wrong `free` flag: expected {}, got {}",
            expected_free, free
        ),
        ref other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

/// Assert that the `bore` named-argument value in `source` lowers to
/// `ExprKind::Auto { free: expected_free }`.
///
/// Callers supply the full source string; the convention is
/// `"structure S { sub b = Bearing(bore: <auto-expr>) }"`.
fn assert_named_arg_value_auto(source: &str, expected_free: bool) {
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
    let sub_decl = match &structure.members[0] {
        MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    let (_, expr) = sub_decl
        .args
        .iter()
        .find(|(n, _)| n == "bore")
        .expect("expected a 'bore' named arg");
    match expr.kind {
        ExprKind::Auto { free } => assert_eq!(
            free, expected_free,
            "wrong `free` flag: expected {}, got {}",
            expected_free, free
        ),
        ref other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

/// Assert that the `gain` connect-param value in `source` lowers to
/// `ExprKind::Auto { free: expected_free }`.
///
/// Callers supply the full source string; the convention is
/// `"structure S { connect a -> b { gain = <auto-expr> } }"`.
fn assert_connect_param_value_auto(source: &str, expected_free: bool) {
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
    let connect_decl = structure
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Connect(c) => Some(c),
            _ => None,
        })
        .expect("expected a Connect member");
    let (_, expr) = connect_decl
        .params
        .iter()
        .find(|(n, _)| n == "gain")
        .expect("expected a 'gain' connect param");
    match expr.kind {
        ExprKind::Auto { free } => assert_eq!(
            free, expected_free,
            "wrong `free` flag: expected {}, got {}",
            expected_free, free
        ),
        ref other => panic!("expected ExprKind::Auto, got {:?}", other),
    }
}

// ── Site 1: let_declaration.value ────────────────────────────────────────────

/// `let m : Length = auto` — strict form lowers to `ExprKind::Auto { free: false }`.
#[test]
fn let_value_auto_strict_lowers_to_expr_kind_auto_false() {
    assert_let_value_auto("structure S { let m : Length = auto }", false);
}

/// `let m : Length = auto(free)` — free form lowers to `ExprKind::Auto { free: true }`.
#[test]
fn let_value_auto_free_lowers_to_expr_kind_auto_true() {
    assert_let_value_auto("structure S { let m : Length = auto(free) }", true);
}

/// `let m : Length = 1.0` — a non-auto literal still lowers normally.
///
/// Regression guard: `lower_binding_value` must not short-circuit non-`auto_keyword`
/// nodes — they must fall through to `lower_expr` and produce the expected `ExprKind`.
#[test]
fn let_value_non_auto_lowers_normally() {
    let source = "structure S { let m : Length = 1.0 }";
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
        matches!(let_decl.value.kind, ExprKind::NumberLiteral { .. }),
        "expected NumberLiteral, got {:?}",
        let_decl.value.kind
    );
}

// ── Site 2: named_argument.value ─────────────────────────────────────────────

/// `sub b = Bearing(bore: auto)` — strict form lowers the `bore` arg's value
/// to `ExprKind::Auto { free: false }`.
#[test]
fn named_argument_value_auto_strict_lowers_to_expr_kind_auto_false() {
    assert_named_arg_value_auto("structure S { sub b = Bearing(bore: auto) }", false);
}

/// `sub b = Bearing(bore: auto(free))` — free form lowers the `bore` arg's value
/// to `ExprKind::Auto { free: true }`.
#[test]
fn named_argument_value_auto_free_lowers_to_expr_kind_auto_true() {
    assert_named_arg_value_auto("structure S { sub b = Bearing(bore: auto(free)) }", true);
}

/// `sub b = Bearing(bore: 1.0)` — a non-auto literal still lowers normally.
///
/// Regression guard: `lower_binding_value` must not short-circuit non-`auto_keyword`
/// nodes at the named-argument site.
#[test]
fn named_argument_value_non_auto_lowers_normally() {
    let source = "structure S { sub b = Bearing(bore: 1.0) }";
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
    let sub_decl = match &structure.members[0] {
        MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    let (_, expr) = sub_decl
        .args
        .iter()
        .find(|(n, _)| n == "bore")
        .expect("expected a 'bore' named arg");
    assert!(
        matches!(expr.kind, ExprKind::NumberLiteral { .. }),
        "expected NumberLiteral, got {:?}",
        expr.kind
    );
}

// ── Site 3: connect_param_assignment.value ───────────────────────────────────

/// `connect a -> b { gain = auto }` — strict form lowers the `gain` param's
/// value to `ExprKind::Auto { free: false }`.
#[test]
fn connect_param_value_auto_strict_lowers_to_expr_kind_auto_false() {
    assert_connect_param_value_auto("structure S { connect a -> b { gain = auto } }", false);
}

/// `connect a -> b { gain = auto(free) }` — free form lowers the `gain` param's
/// value to `ExprKind::Auto { free: true }`.
#[test]
fn connect_param_value_auto_free_lowers_to_expr_kind_auto_true() {
    assert_connect_param_value_auto("structure S { connect a -> b { gain = auto(free) } }", true);
}

/// `connect a -> b { gain = 1.0 }` — a non-auto literal still lowers normally.
///
/// Regression guard: `lower_binding_value` must not short-circuit non-`auto_keyword`
/// nodes at the connect-param site.
#[test]
fn connect_param_value_non_auto_lowers_normally() {
    let source = "structure S { connect a -> b { gain = 1.0 } }";
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
    let connect_decl = structure
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Connect(c) => Some(c),
            _ => None,
        })
        .expect("expected a Connect member");
    let (_, expr) = connect_decl
        .params
        .iter()
        .find(|(n, _)| n == "gain")
        .expect("expected a 'gain' connect param");
    assert!(
        matches!(expr.kind, ExprKind::NumberLiteral { .. }),
        "expected NumberLiteral, got {:?}",
        expr.kind
    );
}

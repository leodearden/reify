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

// ── Site 1: let_declaration.value ────────────────────────────────────────────

/// `let m : Length = auto` — strict form lowers to `ExprKind::Auto { free: false }`.
#[test]
fn let_value_auto_strict_lowers_to_expr_kind_auto_false() {
    let source = "structure S { let m : Length = auto }";
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

    assert_eq!(let_decl.name, "m");
    assert!(
        matches!(let_decl.value.kind, ExprKind::Auto { free: false }),
        "expected ExprKind::Auto {{ free: false }}, got {:?}",
        let_decl.value.kind
    );
}

/// `let m : Length = auto(free)` — free form lowers to `ExprKind::Auto { free: true }`.
#[test]
fn let_value_auto_free_lowers_to_expr_kind_auto_true() {
    let source = "structure S { let m : Length = auto(free) }";
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

    assert_eq!(let_decl.name, "m");
    assert!(
        matches!(let_decl.value.kind, ExprKind::Auto { free: true }),
        "expected ExprKind::Auto {{ free: true }}, got {:?}",
        let_decl.value.kind
    );
}

// ── Site 2: named_argument.value ─────────────────────────────────────────────

/// `sub b = Bearing(bore: auto)` — strict form lowers the `bore` arg's value
/// to `ExprKind::Auto { free: false }`.
#[test]
fn named_argument_value_auto_strict_lowers_to_expr_kind_auto_false() {
    let source = "structure S { sub b = Bearing(bore: auto) }";
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

    let (name, expr) = sub_decl
        .args
        .iter()
        .find(|(n, _)| n == "bore")
        .expect("expected a 'bore' named arg");
    assert_eq!(name, "bore");
    assert!(
        matches!(expr.kind, ExprKind::Auto { free: false }),
        "expected ExprKind::Auto {{ free: false }}, got {:?}",
        expr.kind
    );
}

/// `sub b = Bearing(bore: auto(free))` — free form lowers the `bore` arg's value
/// to `ExprKind::Auto { free: true }`.
#[test]
fn named_argument_value_auto_free_lowers_to_expr_kind_auto_true() {
    let source = "structure S { sub b = Bearing(bore: auto(free)) }";
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

    let (name, expr) = sub_decl
        .args
        .iter()
        .find(|(n, _)| n == "bore")
        .expect("expected a 'bore' named arg");
    assert_eq!(name, "bore");
    assert!(
        matches!(expr.kind, ExprKind::Auto { free: true }),
        "expected ExprKind::Auto {{ free: true }}, got {:?}",
        expr.kind
    );
}

// ── Site 3: connect_param_assignment.value ───────────────────────────────────

/// `connect a -> b { gain = auto }` — strict form lowers the `gain` param's
/// value to `ExprKind::Auto { free: false }`.
#[test]
fn connect_param_value_auto_strict_lowers_to_expr_kind_auto_false() {
    let source = "structure S { connect a -> b { gain = auto } }";
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

    let (name, expr) = connect_decl
        .params
        .iter()
        .find(|(n, _)| n == "gain")
        .expect("expected a 'gain' connect param");
    assert_eq!(name, "gain");
    assert!(
        matches!(expr.kind, ExprKind::Auto { free: false }),
        "expected ExprKind::Auto {{ free: false }}, got {:?}",
        expr.kind
    );
}

/// `connect a -> b { gain = auto(free) }` — free form lowers the `gain` param's
/// value to `ExprKind::Auto { free: true }`.
#[test]
fn connect_param_value_auto_free_lowers_to_expr_kind_auto_true() {
    let source = "structure S { connect a -> b { gain = auto(free) } }";
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

    let (name, expr) = connect_decl
        .params
        .iter()
        .find(|(n, _)| n == "gain")
        .expect("expected a 'gain' connect param");
    assert_eq!(name, "gain");
    assert!(
        matches!(expr.kind, ExprKind::Auto { free: true }),
        "expected ExprKind::Auto {{ free: true }}, got {:?}",
        expr.kind
    );
}

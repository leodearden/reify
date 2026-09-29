//! Lowering tests for `aux` modifier and `at` pose clause (task 3899).
//!
//! Step-3 (default-shape tests): assert that existing plain let/sub forms
//! lower to `is_aux == false` and `pose_expr == None`. These tests drive the
//! mechanical field-addition in step-4 and serve as regression pins that
//! plain forms are unaffected by the new fields.
//!
//! Step-5 (behavioral tests): added later in this same file — assert that
//! `aux let`/`aux sub`/`sub … at p` lower to the expected non-default values.
//!
//! Both sets reference `LetDecl::is_aux`, `SubDecl::is_aux`, and
//! `SubDecl::pose_expr` — fields that do NOT exist until step-4 lands, so
//! this file produces a **compile-error RED** until step-4 is complete.
//! This is the idiomatic TDD signal in this codebase (see
//! sub_decl_specialization_tests.rs header for precedent).

use reify_ast::{Declaration, LetDecl, MemberDecl, SubDecl};
use reify_core::ModulePath;

// ── Test helpers ──────────────────────────────────────────────────────────

/// Parse `source` and return the first structure's member list.
fn parse_first_structure_members(source: &str) -> Vec<MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure declaration, got {:?}", other),
    }
}

/// Locate the first `MemberDecl::Let` in a member slice.
fn first_let(members: &[MemberDecl]) -> &LetDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) => Some(l),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Let in the parsed structure")
}

/// Locate the first `MemberDecl::Sub` in a member slice.
fn first_sub(members: &[MemberDecl]) -> &SubDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sub(s) => Some(s),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Sub in the parsed structure")
}

// ── Step-3: default-shape tests (GREEN after step-4, drive field addition) ──

/// Plain `let x = 5mm` lowers to `LetDecl.is_aux == false`.
///
/// Compile-error RED until step-4 adds `is_aux` to `LetDecl`.
#[test]
fn plain_let_has_is_aux_false() {
    let source = "structure S { let x = 5mm }";
    let members = parse_first_structure_members(source);
    let let_decl = first_let(&members);
    assert!(
        !let_decl.is_aux,
        "plain `let x = 5mm` must lower to is_aux == false"
    );
}

/// Plain `sub a = Foo()` lowers to `SubDecl.is_aux == false` and
/// `SubDecl.pose_expr.is_none()`.
///
/// Compile-error RED until step-4 adds `is_aux`/`pose_expr` to `SubDecl`.
#[test]
fn plain_sub_instantiation_has_defaults() {
    let source = "structure S { sub a = Foo() }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        !sub.is_aux,
        "plain `sub a = Foo()` must lower to is_aux == false"
    );
    assert!(
        sub.pose_expr.is_none(),
        "plain `sub a = Foo()` must lower to pose_expr == None"
    );
}

/// Plain `sub a : T` (bare specialization) lowers to `pose_expr == None`.
///
/// Compile-error RED until step-4 adds `pose_expr` to `SubDecl`.
#[test]
fn plain_sub_specialization_has_no_pose() {
    let source = "structure S { sub a : T }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.pose_expr.is_none(),
        "plain `sub a : T` must lower to pose_expr == None"
    );
}

// ── Step-5: behavioral tests (GREEN after step-6, drive parser wiring) ────
// Added here so they compile as a unit with the default-shape tests above.
// These assert NON-default values — they are behavioral RED after step-4
// (fields exist, parser still returns defaults).

/// `aux let x = 5mm` lowers to `LetDecl.is_aux == true`.
///
/// Behavioral RED until step-6 wires `has_aux_keyword` in `lower_let`.
#[test]
fn aux_let_has_is_aux_true() {
    let source = "structure S { aux let x = 5mm }";
    let members = parse_first_structure_members(source);
    let let_decl = first_let(&members);
    assert!(
        let_decl.is_aux,
        "aux `let x = 5mm` must lower to is_aux == true"
    );
}

/// `pub aux let x = 5mm` lowers to `is_pub == true` AND `is_aux == true`.
///
/// Behavioral RED until step-6.
#[test]
fn pub_aux_let_has_both_flags() {
    let source = "structure S { pub aux let x = 5mm }";
    let members = parse_first_structure_members(source);
    let let_decl = first_let(&members);
    assert!(
        let_decl.is_pub,
        "pub aux let must have is_pub == true"
    );
    assert!(
        let_decl.is_aux,
        "pub aux let must have is_aux == true"
    );
}

/// `aux sub a : T` lowers to `SubDecl.is_aux == true`.
///
/// Behavioral RED until step-6.
#[test]
fn aux_sub_specialization_has_is_aux_true() {
    let source = "structure S { aux sub a : T }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.is_aux,
        "aux `sub a : T` must lower to is_aux == true"
    );
}

/// `sub b : T at frame3(o, b)` lowers to `pose_expr.is_some()` and
/// `is_aux == false`.
///
/// Behavioral RED until step-6.
#[test]
fn sub_with_at_pose_has_pose_expr() {
    let source = "structure S { sub b : T at frame3(o, b) }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.pose_expr.is_some(),
        "`sub b : T at frame3(o, b)` must lower to pose_expr.is_some()"
    );
    assert!(
        !sub.is_aux,
        "`sub b : T at frame3(o, b)` must lower to is_aux == false"
    );
}

/// `sub c : T { } at p` lowers to `pose_expr.is_some()` AND `body.is_some()`.
///
/// Behavioral RED until step-6.
#[test]
fn sub_with_body_and_at_has_both() {
    let source = "structure S { sub c : T { } at p }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.pose_expr.is_some(),
        "`sub c : T {{ }} at p` must lower to pose_expr.is_some()"
    );
    assert!(
        sub.body.is_some(),
        "`sub c : T {{ }} at p` must lower to body.is_some()"
    );
}

/// Instantiation-arm pose: `sub bolt = Foo() at p` lowers to `pose_expr.is_some()`.
///
/// Behavioral RED until step-6.
#[test]
fn sub_instantiation_with_at_has_pose_expr() {
    let source = "structure S { sub bolt = Foo() at p }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.pose_expr.is_some(),
        "`sub bolt = Foo() at p` must lower to pose_expr.is_some()"
    );
}

/// `aux sub a = Foo()` (instantiation arm) lowers to `SubDecl.is_aux == true`.
///
/// All three arms share the single `has_aux_keyword` call in `lower_sub`, but
/// this pin makes the instantiation-arm path explicit and catches any future
/// per-arm divergence early.
#[test]
fn aux_sub_instantiation_has_is_aux_true() {
    let source = "structure S { aux sub a = Foo() }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.is_aux,
        "`aux sub a = Foo()` must lower to is_aux == true"
    );
}

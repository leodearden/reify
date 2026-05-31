//! Lowering tests for `priv` member-level visibility modifier (task 3976).
//!
//! Step-3 (default-shape tests): assert that existing plain param/sub/port
//! forms lower to `is_priv == false`. These tests drive the mechanical
//! field-addition in step-4 and serve as regression pins that plain forms
//! are unaffected by the new field.
//!
//! Step-5 (behavioral tests): added later in this same file — assert that
//! `priv param`/`priv sub`/`priv port` lower to `is_priv == true`.
//!
//! Both sets reference `ParamDecl::is_priv`, `SubDecl::is_priv`, and
//! `PortDecl::is_priv` — fields that do NOT exist until step-4 lands, so
//! this file produces a **compile-error RED** until step-4 is complete.
//! This is the idiomatic TDD signal in this codebase (see
//! aux_at_lowering_tests.rs header for precedent).

use reify_ast::{Declaration, MemberDecl, ParamDecl, PortDecl, SubDecl};
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

/// Locate the first `MemberDecl::Param` in a member slice.
fn first_param(members: &[MemberDecl]) -> &ParamDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) => Some(p),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Param in the parsed structure")
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

/// Locate the first `MemberDecl::Port` in a member slice.
fn first_port(members: &[MemberDecl]) -> &PortDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Port(p) => Some(p),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Port in the parsed structure")
}

// ── Step-3: default-shape tests (GREEN after step-4, drive field addition) ──

/// Plain `param w : Length = 1mm` lowers to `ParamDecl.is_priv == false`.
///
/// Compile-error RED until step-4 adds `is_priv` to `ParamDecl`.
#[test]
fn plain_param_has_is_priv_false() {
    let source = "structure S { param w : Length = 1mm }";
    let members = parse_first_structure_members(source);
    let param = first_param(&members);
    assert!(
        !param.is_priv,
        "plain `param w : Length = 1mm` must lower to is_priv == false"
    );
}

/// Plain `sub a = Foo()` lowers to `SubDecl.is_priv == false`.
///
/// Compile-error RED until step-4 adds `is_priv` to `SubDecl`.
#[test]
fn plain_sub_instantiation_has_is_priv_false() {
    let source = "structure S { sub a = Foo() }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        !sub.is_priv,
        "plain `sub a = Foo()` must lower to is_priv == false"
    );
}

/// Plain `port p : MechPort` lowers to `PortDecl.is_priv == false`.
///
/// Compile-error RED until step-4 adds `is_priv` to `PortDecl`.
#[test]
fn plain_port_has_is_priv_false() {
    let source = "structure S { port p : MechPort }";
    let members = parse_first_structure_members(source);
    let port = first_port(&members);
    assert!(
        !port.is_priv,
        "plain `port p : MechPort` must lower to is_priv == false"
    );
}

// ── Step-5: behavioral tests (GREEN after step-6, drive parser wiring) ────
// Added here so they compile as a unit with the default-shape tests above.
// These assert NON-default values — they are behavioral RED after step-4
// (fields exist, parser still returns defaults).

/// `priv param rated_torque : Torque = 5` lowers to `ParamDecl.is_priv == true`.
///
/// Behavioral RED until step-6 wires `has_priv_keyword` in `lower_param`.
#[test]
fn priv_param_has_is_priv_true() {
    let source = "structure S { priv param rated_torque : Torque = 5 }";
    let members = parse_first_structure_members(source);
    let param = first_param(&members);
    assert!(
        param.is_priv,
        "`priv param rated_torque : Torque = 5` must lower to is_priv == true"
    );
}

/// `priv sub inner = Inner()` lowers to `SubDecl.is_priv == true`.
///
/// Behavioral RED until step-6.
#[test]
fn priv_sub_instantiation_has_is_priv_true() {
    let source = "structure S { priv sub inner = Inner() }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.is_priv,
        "`priv sub inner = Inner()` must lower to is_priv == true"
    );
}

/// `priv port hidden : MechPort` lowers to `PortDecl.is_priv == true`.
///
/// Behavioral RED until step-6.
#[test]
fn priv_port_has_is_priv_true() {
    let source = "structure S { priv port hidden : MechPort }";
    let members = parse_first_structure_members(source);
    let port = first_port(&members);
    assert!(
        port.is_priv,
        "`priv port hidden : MechPort` must lower to is_priv == true"
    );
}

/// `priv aux sub a : T` lowers to `SubDecl.is_priv == true` AND `is_aux == true`.
///
/// Confirms the two bools are independent: both flags are set when both
/// modifiers are present. Behavioral RED until step-6.
#[test]
fn priv_aux_sub_has_both_flags_true() {
    let source = "structure S { priv aux sub a : T }";
    let members = parse_first_structure_members(source);
    let sub = first_sub(&members);
    assert!(
        sub.is_priv,
        "`priv aux sub a : T` must lower to is_priv == true"
    );
    assert!(
        sub.is_aux,
        "`priv aux sub a : T` must lower to is_aux == true"
    );
}


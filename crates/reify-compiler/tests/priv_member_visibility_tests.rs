//! Compiled-IR member-visibility wiring + enforcement tests (task #3978 δ —
//! `module-and-visibility-hardening.md` Slice C, steps 3–6).
//!
//! ## Part A — AST `is_priv` → compiled visibility contract (steps 3/4)
//!
//! Pins the invariant that `priv param`, `priv sub`, and `priv port` lower into
//! the compiled IR with the correct private-visibility markers:
//!
//! - `priv param p` → `ValueCellDecl.visibility == Visibility::Private`
//! - `param q`      → `ValueCellDecl.visibility == Visibility::Public`
//! - `priv sub a`   → `SubComponentDecl.visibility == Visibility::Private`
//! - `sub b`        → `SubComponentDecl.visibility == Visibility::Public`
//! - `priv port pt` → `CompiledPort.is_priv == true`   (field added in step-4)
//! - `port pu`      → `CompiledPort.is_priv == false`
//!
//! Step-3 RED: params/subs are hardcoded `Visibility::Public`, and `CompiledPort`
//! has no `is_priv` field (a hard compile error). Turns GREEN after step-4 wires
//! `is_priv` at the `entity.rs` construction sites + adds the `CompiledPort` field.
//!
//! ## Part B — E_PRIV_MEMBER_ACCESS single-module enforcement (steps 5/6)
//!
//! Appended in step-5: external dot-access on a `priv` member emits
//! `E_PRIV_MEMBER_ACCESS`, while internal (self-body) access and non-priv member
//! access stay clean. RED until step-6 wires the `expr.rs` enforcement.

use reify_compiler::{ValueCellKind, Visibility};
use reify_test_support::compile_source;

// ── Source fixture ───────────────────────────────────────────────────────────

/// `Motor` exercises all three priv / non-priv member pairs (param, sub, port).
fn motor_source() -> &'static str {
    r#"
trait SomeTrait {}

structure def Inner {}

structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
    priv sub a = Inner()
    sub b = Inner()
    priv port pt : SomeTrait {}
    port pu : SomeTrait {}
}
"#
}

/// Locate the `Motor` template in the compiled module.
fn motor_template(module: &reify_compiler::CompiledModule) -> &reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template not found in compiled module")
}

// ── Part A: AST is_priv → compiled visibility ─────────────────────────────────

/// `priv param p` must lower to `Visibility::Private`.
#[test]
fn priv_param_compiles_to_visibility_private() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let p_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "p" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'p' (Param kind) not found in Motor template");

    assert_eq!(
        p_cell.visibility,
        Visibility::Private,
        "priv param p must compile to Visibility::Private, got {:?}",
        p_cell.visibility
    );
}

/// Plain `param q` must stay `Visibility::Public` (no regression).
#[test]
fn plain_param_compiles_to_visibility_public() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let q_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "q" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'q' (Param kind) not found in Motor template");

    assert_eq!(
        q_cell.visibility,
        Visibility::Public,
        "plain param q must compile to Visibility::Public, got {:?}",
        q_cell.visibility
    );
}

/// `priv sub a = Inner()` must lower to `SubComponentDecl.visibility == Private`.
#[test]
fn priv_sub_compiles_to_visibility_private() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let sub_a = template
        .sub_components
        .iter()
        .find(|s| s.name == "a")
        .expect("sub_component 'a' not found in Motor template");

    assert_eq!(
        sub_a.visibility,
        Visibility::Private,
        "priv sub a must compile to Visibility::Private, got {:?}",
        sub_a.visibility
    );
}

/// Plain `sub b = Inner()` must stay `Visibility::Public` (no regression).
#[test]
fn plain_sub_compiles_to_visibility_public() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let sub_b = template
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("sub_component 'b' not found in Motor template");

    assert_eq!(
        sub_b.visibility,
        Visibility::Public,
        "plain sub b must compile to Visibility::Public, got {:?}",
        sub_b.visibility
    );
}

/// `priv port pt` must lower to `CompiledPort.is_priv == true`.
#[test]
fn priv_port_compiles_to_is_priv_true() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let port_pt = template
        .ports
        .iter()
        .find(|p| p.name == "pt")
        .expect("port 'pt' not found in Motor template");

    assert!(port_pt.is_priv, "priv port pt must compile to is_priv == true");
}

/// Plain `port pu` must stay `is_priv == false` (no regression).
#[test]
fn plain_port_compiles_to_is_priv_false() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let port_pu = template
        .ports
        .iter()
        .find(|p| p.name == "pu")
        .expect("port 'pu' not found in Motor template");

    assert!(
        !port_pu.is_priv,
        "plain port pu must compile to is_priv == false"
    );
}

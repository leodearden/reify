//! Hinge-flexure PRB constructors (PRD §2.2 + §11 Phase-2 task ε):
//! living hinge (Howell §5.7 SLFP), cross-spring pivot (Haringx 1949), and
//! LET joint (Jacobsen et al. 2009) — all → revolute.
//!
//! All three constructors return a 1-DOF Revolute joint `Value::Map`
//! (`kind == "revolute"`) following the same closed-form pattern as
//! γ (beam.rs) and δ (notch.rs). No FEA call — pure closed form.
//! `damping = None` for all three (PRD §5.1 / §8.7 γ-scope contract).
//! Validation failure → `Value::Undef` with NO diagnostic emission
//! (W_Flexure*/E_Flexure* emission is λ/task-3821's responsibility).

use reify_ir::Value;

/// Evaluate a hinge-flexure constructor by name.
///
/// Returns `Some(Value)` for a recognised hinge name (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so the caller can fall through to the next module.
pub(crate) fn eval_hinge(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_living_hinge" => Some(prb_living_hinge(args)),
        "prb_cross_spring_pivot" => Some(prb_cross_spring_pivot(args)),
        "prb_let_joint" => Some(prb_let_joint(args)),
        _ => None,
    }
}

/// `prb_living_hinge(length, width, thickness, material, pivot, axis[, neutral])`
/// — Howell §5.7 small-length flexural pivot (SLFP) as a revolute joint.
///
/// Stub — returns `Value::Undef` until step-2 implements the closed form.
fn prb_living_hinge(_args: &[Value]) -> Value {
    Value::Undef
}

/// `prb_cross_spring_pivot(length, width, thickness, material, pivot, axis[, neutral])`
/// — Haringx 1949 crossed-leaf pivot as a revolute joint.
///
/// Stub — returns `Value::Undef` until step-4 implements the closed form.
fn prb_cross_spring_pivot(_args: &[Value]) -> Value {
    Value::Undef
}

/// `prb_let_joint(length, width, thickness, n_blades, material, pivot, axis[, neutral])`
/// — Jacobsen et al. 2009 lamina-emergent torsion (multi-blade torsion) as a
/// revolute joint.
///
/// Stub — returns `Value::Undef` until step-6 implements the closed form.
fn prb_let_joint(_args: &[Value]) -> Value {
    Value::Undef
}

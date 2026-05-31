//! Compound-flexure PRB constructors: parallelogram and double-parallelogram
//! stages (Compliant-Joints PRD §6.1/§6.2).
//!
//! ## Physical model
//!
//! A parallelogram flexure stage consists of four fixed-guided blades (γ_pp = 12,
//! Howell §5 / PRD §6.1) arranged in two pairs, constraining a moving platform
//! to translate along the motion axis. Because the blades are fixed-guided
//! (both ends remain oriented), the stiffness model is identical to
//! `beam::prb_fixed_fixed_beam`.
//!
//! ### Parasitic error — Roberts approximation (PRD §6.1)
//! A translating parallelogram stage exhibits a second-order vertical (parasitic)
//! displacement modelled by the Roberts-approximation arc:
//!   δ_rot = L·(1 − cos(δ_max/L))
//!
//! ### Mirror-cancellation in the double stage (PRD §6.2)
//! Two single stages in mirror-symmetric series cancel the first-order parasitic
//! term; the residual scales as (δ/L)³ instead of (δ/L):
//!   δ_rot_double = δ_rot_single · (δ_max/L)²

use reify_ir::Value;

/// Evaluate a compound-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including `Some(Value::Undef)` on
/// validation failure) and `None` for any unknown name, so `eval_builtin` falls
/// through to the next module.
pub(crate) fn eval_compound(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_util::*;
    // Tests will be added in subsequent TDD steps.
}

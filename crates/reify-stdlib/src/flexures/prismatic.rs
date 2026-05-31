//! Prismatic-blade and two-axis-pivot PRB constructors (Howell §6.2; Henein 2010).
//!
//! Two constructors, sharing the positional argument layout
//! `(length, width, thickness, material, pivot, axis[, neutral])` for parser
//! symmetry:
//!
//!  - `prb_prismatic_blade` → `kind = "prismatic"` (Howell §6.2 single-
//!    cantilever-blade). Transverse stiffness `k_trans = 3·E·I/L³` (γ = 3,
//!    intentionally distinct from `beam::prb_fixed_fixed_beam`'s γ = 12).
//!
//!  - `prb_two_axis_pivot` → `kind = "spherical"` (Henein 2010 two-axis pivot).
//!    Per-axis rotational stiffness `k_axis = E·I/L` (γ = 1, slender-blade).
//!    `spring_rate = None` (PRD §8.6/§13.1 multi-DOF invariant); stiffness is
//!    surfaced only via `effective_stiffness`. Axis is validated for signature
//!    symmetry but NOT stored — the spherical joint is axis-isotropic.
//!
//! Dispatch via [`eval_prismatic`], mirroring the sibling modules.

use reify_ir::Value;

/// Evaluate a prismatic-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so `eval_builtin` falls through to the next module.
pub(crate) fn eval_prismatic(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

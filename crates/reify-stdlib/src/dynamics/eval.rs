//! Eval-side `Value`↔core dispatch for the RBD-η stdlib dynamics entry points
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5 / task RBD-η, Phase 4).
//!
//! This module is the `Value`-marshalling half of the dynamics surface: it
//! extracts `Value`s into the pure-`f64` `RneaLink` / KKT inputs consumed by
//! the [`crate::dynamics::rnea`] and [`crate::dynamics::closed_chain`] cores,
//! invokes them, and reshapes the result `τ` back into registry-free
//! `JointForce` / `MotionTrajectory` `Value::StructureInstance`s.
//!
//! **Why this lives in `reify-stdlib`, not `reify-eval/src/dynamics_ops.rs`.**
//! `joints::motion_subspace_columns` is `pub(crate)` and the RNEA / closed-chain
//! cores are crate-internal, so the marshalling MUST be in-crate to reach them.
//! `inverse_dynamics` needs no `GeometryKernel` (mass comes from `body.solid`),
//! so the engine-post-process path used by `body_mass_props` is unnecessary.
//! Registered through `lib.rs::eval_builtin`, dispatched via the gcode_import
//! delegate-to-intrinsic pattern: the `dynamics.ri` surface fns delegate to
//! `*_lower` intrinsics with no `.ri` declaration, which resolve
//! `NoUserFunctions → FunctionCall → eval_builtin → eval_dynamics`.
//!
//! The recognised intrinsic names are:
//!   * `ramp_profile_lower`                  — trajectory generator (step-2)
//!   * `inverse_dynamics_at_snapshot_lower`  — open-chain snapshot RNEA (step-6)
//!   * `inverse_dynamics_lower`              — trajectory variant (step-8)
//!     (closed-chain routing layered into the snapshot core, step-10)

use reify_ir::Value;

/// Evaluate an RBD-η dynamics intrinsic by name.
///
/// Returns `Some(Value)` for the dynamics `*_lower` intrinsics this module owns
/// (including `Some(Value::Undef)` on malformed input, matching the
/// mechanism/snapshot/body eval_builtin convention), or `None` for any other
/// name so that `eval_builtin` can fall through to the next module.
pub(crate) fn eval_dynamics(name: &str, args: &[Value]) -> Option<Value> {
    // Implemented incrementally by RBD-η steps 1-10. Until the dispatch arms
    // land, every name returns `None` (eval_builtin falls through → Undef).
    let _ = (name, args);
    None
}

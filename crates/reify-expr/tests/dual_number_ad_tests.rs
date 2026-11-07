//! Forward-mode dual-number automatic differentiation — task #6672
//! (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7 ("Derivatives — three sources").  This file is a *separate test
//! binary*, so it exercises only the *published public surface* — the
//! `reify_expr` crate ROOT, which since the AD modules were made private is the
//! only path to them, so this is exactly the surface the downstream consumers
//! see: η (#6675, the Gauss-Newton/LM step and its
//! `‖Jᵀr‖` certificate), μ (#6680, the reduced gradient) and λ (#6679, kink
//! chatter detection).
//!
//! The chain rules themselves are stated once, in `dual_eval`, and are
//! exercised here only through the traversal that applies them: a rule that
//! no evaluation reaches is a rule nothing depends on.
//!
//! One module per subject: `traversal`, `central_differences`, `user_functions`,
//! `refusals`, `field_reductions`, `contribution_masking`, `active_branch_tangents`,
//! `call_intercepts` and `seed_set`; `support` holds the probe vocabulary they share.

#[path = "dual_number_ad_tests/support.rs"]
mod support;

#[path = "dual_number_ad_tests/traversal.rs"]
mod traversal;

#[path = "dual_number_ad_tests/central_differences.rs"]
mod central_differences;

#[path = "dual_number_ad_tests/user_functions.rs"]
mod user_functions;

#[path = "dual_number_ad_tests/refusals.rs"]
mod refusals;

#[path = "dual_number_ad_tests/field_reductions.rs"]
mod field_reductions;

#[path = "dual_number_ad_tests/contribution_masking.rs"]
mod contribution_masking;

#[path = "dual_number_ad_tests/active_branch_tangents.rs"]
mod active_branch_tangents;

#[path = "dual_number_ad_tests/call_intercepts.rs"]
mod call_intercepts;

#[path = "dual_number_ad_tests/seed_set.rs"]
mod seed_set;

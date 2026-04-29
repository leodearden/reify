//! Loop-closure machinery: value-level helpers operating on joint-Map `Value`s.
//!
//! This module provides the building blocks the kinematic snapshot evaluator
//! (future task 2585) and the generic Newton solver in
//! `reify_constraints::loop_closure` use to drive closed-chain mechanisms to
//! consistency.  It is the value-side companion to `reify-constraints::loop_closure`.
//!
//! Public API surface (filled in by the TDD steps that follow):
//!   * `chain_transform(chain, values) -> Option<Value>`
//!   * `loop_residual_twist(chain_a, vals_a, chain_b, vals_b) -> Option<[f64; 6]>`
//!   * `joint_range_midpoint(joint) -> Option<f64>`
//!   * `per_joint_jacobian_local(joint) -> Option<[f64; 6]>`
//!   * `chain_jacobian_fd(chain, values, free_indices, eps) -> Option<Vec<[f64; 6]>>`
//!
//! Twist convention: `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear last)
//! mirroring the `Map { angular, linear }` shape emitted by `transform_log` and
//! `joint_jacobian`.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! design rationale and convergence-tolerance defaults.

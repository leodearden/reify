//! Featherstone Recursive Newton-Euler Algorithm (RNEA) for open kinematic chains.
//!
//! Implements the inverse-dynamics core from
//! `docs/prds/v0_3/rigid-body-dynamics.md` §5.2 (task RBD-ε, Phase 2).
//!
//! **Pure-Rust `f64` numerics — no Reify-level `Value` dispatch.**
//! All `Value`↔core wiring (extracting `SpatialInertia` from `MassProperties`,
//! computing the configuration-dependent `X_{p→i}` from joint `Value`s plus a
//! snapshot, calling `motion_subspace_columns`, and reshaping τ into a
//! `JointForce` list) is deferred to task RBD-η (Phase 4 eval-side dispatch).
//! The end-to-end `examples/dynamics/pendulum_idyn.ri` execution is also η.
//!
//! # Reference
//! Featherstone, *Rigid Body Dynamics Algorithms* (2008), §5.2.

#[cfg(test)]
mod tests {}

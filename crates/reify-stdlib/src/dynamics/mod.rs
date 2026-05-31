//! Rigid-body dynamics primitives (Featherstone 6D spatial-vector algebra).
//!
//! This module tree ships the Phase-1 RBD core consumed by downstream
//! articulated-body inverse-dynamics tasks (RBD-δ motion subspace, RBD-ε
//! RNEA). All math is pure-Rust `f64` numerics — no Reify-level `Value`
//! dispatch and no heavyweight linalg dependency.

pub mod mass_props;
pub mod spatial;

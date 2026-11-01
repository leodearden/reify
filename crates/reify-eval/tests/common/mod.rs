//! The as-printed FDM fixture root, declared by a bare `mod common;` from
//! `harness_fea_solver_e2e` only: that unit holds every consumer of
//! `common::as_printed`.
//!
//! Every other helper in this directory is deliberately NOT declared here. Each is
//! `#[path]`-included by exactly the roots that use it, so no binary compiles
//! helpers it never calls.

pub mod as_printed;

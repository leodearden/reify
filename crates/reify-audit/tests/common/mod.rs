//! Shared test helpers for reify-audit integration tests.
//!
//! Each consuming test binary adds `mod common;` at its top level; Cargo
//! treats `tests/common/mod.rs` as a module file (not a separate test
//! binary). `#[allow(dead_code)]` on individual items suppresses warnings
//! when a given test binary consumes only a subset of the helpers.

pub mod schema;
// pub mod fixtures; — added in step-4

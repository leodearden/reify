//! Meaning-free primitive vocabulary for Reify.
//!
//! This is the leaf crate introduced in Phase 1 of
//! `docs/prds/core-ast-ir-layering.md` (task γ). It contains the eight
//! modules that carry no semantic meaning (no `reify-*` dependencies) and
//! can therefore sit at the bottom of the dependency graph.
//!
//! # B1 invariant
//!
//! This crate MUST have zero `reify-*` dependencies. The structural invariant
//! is locked in by `crates/reify-core/tests/dag_invariant.rs`, which shells
//! out to `cargo metadata` and asserts that no dependency name starts with
//! `"reify-"`. The workspace-wide assertion (`scripts/assert-crate-dag.sh`)
//! arrives under task η per PRD §10.

// `BTreeMap<Value, _>` in downstream crates can trigger this lint; we copy
// the attribute from reify-types to keep the two crates' lint preludes
// structurally identical for downstream reasoning.
#![allow(clippy::mutable_key_type)]

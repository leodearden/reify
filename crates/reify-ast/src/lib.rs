//! Parsed expression/type AST for Reify.
//!
//! This is the Phase 2 crate introduced in
//! `docs/prds/core-ast-ir-layering.md` (task δ). It contains the parsed
//! AST types that model *source as written* — identifiers unresolved,
//! operators as strings, no types attached.
//!
//! # B2 invariant
//!
//! This crate MUST have exactly one `reify-*` dependency: `reify-core`.
//! No tree-sitter dependency is permitted. The structural invariant is
//! locked in by `crates/reify-ast/tests/dag_invariant.rs`, which reads
//! `Cargo.toml` directly and asserts both conditions. The workspace-wide
//! assertion (`scripts/assert-crate-dag.sh`) arrives under task η per PRD §10.

// `BTreeMap<Value, _>` in downstream crates can trigger this lint; we copy
// the attribute from reify-core/reify-types to keep the crates' lint preludes
// structurally identical for downstream reasoning.
#![allow(clippy::mutable_key_type)]

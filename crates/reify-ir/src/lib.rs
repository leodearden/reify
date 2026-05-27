//! Compiled IR and runtime vocabulary for Reify.
//!
//! This is the Phase 3 crate introduced in
//! `docs/prds/core-ast-ir-layering.md` (task ζ). It contains the 15 modules
//! that model *compiled, runtime-level* representations — resolved identifiers,
//! typed values, geometry handles, constraint solvers, warm-start state, etc.
//!
//! # B3 invariant
//!
//! This crate MUST have exactly two `reify-*` dependencies: `reify-core` and
//! `reify-ast`. No other intra-workspace `reify-*` dependency is permitted.
//! The structural invariant is locked in by
//! `crates/reify-ir/tests/dag_invariant.rs`, which reads `Cargo.toml` directly
//! and asserts both conditions. The workspace-wide assertion
//! (`scripts/assert-crate-dag.sh`) arrives under task η per PRD §10.

// `Value` carries a `SampledField` whose `oob_emitted: AtomicBool` introduces
// interior mutability that does NOT participate in `PartialEq`/`Ord`/`Hash`/
// `content_hash`. `BTreeMap<Value, _>` (notably `Value::Map`) therefore preserves
// its ordering invariants, but `clippy::mutable_key_type` still fires. See
// `value.rs::SampledField` for the full rationale.
#![allow(clippy::mutable_key_type)]

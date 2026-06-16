//! ζ (task 4359) differential safety-gate — the corpus SWEEP.
//!
//! Builds a representative corpus under BOTH `BuildScheduler::{LegacyMultiPass,
//! UnifiedDag}` and asserts `BuildResult` equivalence on the overlap domain,
//! gated by a per-case REASONED allow-list (`assert_equivalent_or_allowed`). It
//! also runs unified 2× → byte-identical (determinism) and adds the Stage-1
//! `residue == ∅` gate on every acyclic legacy-passing case (PRD
//! `docs/prds/v0_6/engine-unified-build-dag.md` §6, decomposition §8-ζ).
//!
//! The §6 boundary cases a plain legacy-vs-unified diff cannot surface live in
//! the sibling binary `unified_dag_boundary_cases.rs`.
//!
//! The shared harness is `#[path]`-included (NOT via `tests/common/mod.rs`) so
//! this safety-gate lands with zero edits to existing shared test files.

#[path = "common/differential.rs"]
mod differential;

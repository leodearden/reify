//! ζ (task 4359) differential safety-gate — the §6 expanded BOUNDARY cases.
//!
//! These are the cases a plain legacy-vs-unified `BuildResult` diff CANNOT
//! surface (because legacy degrades identically, or the property is about a
//! scheduler-internal ordering / a directly-asserted unified-only diagnostic):
//!   * auto + geometry-backed constraint → `EvalUnresolved` (unified) /
//!     Indeterminate (legacy);
//!   * cross-sub multi-body assembly with a lexicographically-early parent →
//!     byte-equivalent multi-body export under both schedulers;
//!   * the 4275 single-instance `let proc = FdmPrinter()` definite-verdict form;
//!   * multi-realization export equivalence + a warm scheduler-agnostic
//!     regression guard (warm stays scheduler-agnostic until θ #4361).
//!
//! The corpus SWEEP (equivalence-or-reasoned, 2× byte-identical, residue==∅)
//! lives in the sibling binary `unified_dag_differential_corpus.rs`.
//!
//! The shared harness is `#[path]`-included (NOT via `tests/common/mod.rs`) so
//! this safety-gate lands with zero edits to existing shared test files.

#[path = "common/differential.rs"]
mod differential;

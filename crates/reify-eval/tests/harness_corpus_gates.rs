//! Consolidated integration-test harness for corpus-wide constraint-satisfaction
//! gates over curated `.ri` example directories.
//!
//! Task #6215: the new `examples/best_practices/` constraint-satisfaction gate is
//! landed HERE, as a C1-sanctioned compile unit, rather than as a top-level
//! standalone `tests/best_practices_constraint_gate.rs`. That standalone form was
//! flagged `reason=unregistered-standalone` by
//! `scripts/check-harness-baseline-registration.sh`; the sanctioned remedy is
//! consolidation, NOT a new `tests/infra/harness-layout-baseline.manifest`
//! grandfather row (SUPERSEDED — Leo 2026-07-22, esc-5056-11: the baseline is a
//! shrinking ratchet, not an allow-list to grow). Mirrors the precedent set by
//! `crates/reify-compiler/tests/harness_geometry_kinds.rs` (task #5754) for the
//! same class of gate failure. No `#[test]` fn is added or removed relative to
//! the standalone form (invariant I3); the file keeps its original stem, so its
//! post-consolidation selector is `best_practices_constraint_gate::<test>`.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test
//! crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_corpus_gates/` subdir. As in
//! `harness_geometry_kinds.rs`, the shared `common` helper module is
//! deliberately NOT declared here: the absorbed file declares no `mod` and uses
//! no helper, so declaring it would pull `tests/common/mod.rs` into this compile
//! unit for nothing, in a PRD whose whole point is cutting merge-gate compile
//! cost.
//!
//! Future corpus-wide gates (over other curated `.ri` directories, or further
//! `examples/best_practices/` assertions) belong in this unit.
//!
//! Task #7431 took that invitation: `eval_invariant_corpus_sweep` is the second
//! corpus-wide gate here, and the first over reify-eval's `.ri` INVARIANT corpus
//! (fixtures + examples + one explicit prd-gate leaf) rather than a single
//! curated examples directory. It unifies two formerly separate 24-shard sweeps
//! into one — see its own header. Unlike the absorbed standalone above, it DOES
//! need a helper, so this root declares `common/eval_gate_support.rs` by
//! `#[path]`; that narrow file, not the 312-line `common/mod.rs`, is what gets
//! charged to this unit, so the no-`mod common;` reasoning above still holds.
#[path = "harness_corpus_gates/best_practices_constraint_gate.rs"]
mod best_practices_constraint_gate;

#[path = "common/eval_gate_support.rs"]
mod eval_gate_support;

#[path = "harness_corpus_gates/eval_invariant_corpus_sweep.rs"]
mod eval_invariant_corpus_sweep;

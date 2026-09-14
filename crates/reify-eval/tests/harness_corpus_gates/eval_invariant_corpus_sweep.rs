//! The unified corpus-wide invariant sweep: ONE compile+eval per `.ri` corpus
//! file, asserting BOTH post-eval invariants off that single evaluation.
//!
//! Task #7431 merged two independently-sharded 24-way sweeps that were each
//! doing their own full compile+eval of an overlapping corpus:
//!
//! - `no_stale_undef_invariant_gate::broad_corpus_sweep_shard_NN` — INV-EVAL-5,
//!   the no-stale-Undef invariant (task α, PRD
//!   `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.1), over the UNION
//!   of reify-eval's `tests/fixtures/`, `examples/`, and the one explicit
//!   `tests/prd-gate/fixtures/geometry_let_selector_consumer.ri` leaf.
//! - `harness_cache::snapshot_cache_divergence_gate::snapshot_cache_sweep_shard_NN`
//!   — INV-EVAL-4, the snapshot↔cache content-hash divergence audit (task ι, PRD
//!   `docs/prds/v0_6/eval-cell-commit-substrate.md` §2.6 / §3 P3 / §7 B4), over
//!   `examples/` only.
//!
//! The merge is sound because `Engine::check_no_stale_undef` (`invariants.rs`)
//! and `Engine::check_snapshot_cache_divergence` (`cache_divergence.rs`) both
//! take `&self` and both read the SAME retained `eval_state()` snapshot the
//! preceding `eval()` installed; neither mutates the engine. One `eval()` can
//! therefore feed both checkers, in either order, with no order-dependent
//! result. What is shared is the corpus EVALUATION — never the checking.
//!
//! # The two CHECKERS stay distinct (task 5060)
//!
//! Task 5060 is explicit that the snapshot↔cache divergence audit is a DISTINCT
//! invariant and the checkers must NOT be merged. They are not: each invariant
//! keeps its own adapter fn, its own corpus SCOPE, its own residual-exemption
//! list and its own failure POLICY, all carried as data on a per-invariant
//! descriptor. This unit shares the expensive part (compile + eval) and nothing
//! else.
//!
//! # Anti-silent-accept guards
//!
//! A corpus sweep asserting "zero findings" is vacuous unless the checkers are
//! independently observed to FIRE. Those seeded-violation self-tests are
//! deliberately RETAINED in the two original units rather than moved here, and
//! they run in the same nextest pass:
//!
//! - `no_stale_undef_invariant_gate::seeded_stale_undef_violation_is_reported`
//! - `no_stale_undef_invariant_gate::seeded_stale_undef_composition_violation_is_reported`
//! - `no_stale_undef_invariant_gate::seeded_composition_over_unresolved_cross_cell_operand_is_exempted_by_dependency_clause`
//! - `no_stale_undef_invariant_gate::seeded_solid_boolean_union_undef_is_exempted_by_geometry_clause`
//! - `no_stale_undef_invariant_gate::seeded_kind_mismatch_composition_undef_is_unexempted_without_caller_discipline`
//! - `harness_cache::snapshot_cache_divergence_gate::seeded_divergence_is_reported`
//! - `harness_cache::snapshot_cache_divergence_gate::seeded_skip_committed_divergence_is_exempted`

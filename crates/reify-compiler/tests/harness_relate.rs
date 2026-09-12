//! Consolidated integration-test harness for the `relate` / geometric-relations
//! subsystem's compile-time checks.
//!
//! Layout mandated by PRD docs/prds/merge-gate-compile-cost.md §Contract C1 and
//! enforced by `scripts/check-harness-baseline-registration.sh`: a new test in a
//! consolidatable crate joins a `harness_<subsystem>.rs` compile unit rather than
//! re-accreting as another standalone `tests/<file>.rs` binary
//! (`tests/infra/harness-layout-baseline.manifest` is a shrinking ratchet, not an
//! allow-list to grow — Leo 2026-07-22, esc-5056-11).
//!
//! Each member is included as a stem-named module, so its selector is
//! `<file>::<test_name>` going forward. Explicit `#[path]` is required: this
//! harness root is an integration-test crate root, where a bare `mod <file>;`
//! would resolve to a sibling `tests/<file>.rs`, not the `harness_relate/`
//! subdir (guard: `tests/infra/test_harness_kloc_cap.sh` §6).
//!
//! Task #5695 (PRD §5 C1, leaf CMP-5) was that dedicated consolidation leaf: it folded
//! the grandfathered `relate_block_check_tests`, `relate_threading_tests` and
//! `relation_check_tests` standalones in and removed their baseline rows. The
//! designation is spent; this unit now holds the whole subsystem.
#[path = "harness_relate/relate_block_check_tests.rs"]
mod relate_block_check_tests;
#[path = "harness_relate/relate_threading_tests.rs"]
mod relate_threading_tests;
#[path = "harness_relate/relation_check_tests.rs"]
mod relation_check_tests;
#[path = "harness_relate/tangent_operand_check_tests.rs"]
mod tangent_operand_check_tests;

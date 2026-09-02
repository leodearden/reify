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
//! The pre-existing grandfathered `tests/relate_*.rs` standalones are the
//! natural future members of this unit; folding them in is a layout-only move
//! left to a dedicated consolidation leaf, not to a task that merely adds a test.
#[path = "harness_relate/tangent_operand_check_tests.rs"]
mod tangent_operand_check_tests;

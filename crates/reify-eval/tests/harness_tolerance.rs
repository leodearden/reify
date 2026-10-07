//! Consolidated integration-test harness for the value-semantics subsystem
//! (tolerance / undef / value / result).
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/<file>.rs` binaries for the `tolerance_`/`undef_`/
//! `value_`/`result_` clusters into this single compile unit to cut the merge-gate link
//! count. Those four prefixes are grouped here because they are one subsystem: what a
//! cell's value *is* (`value_`), what it degrades to when unresolvable (`undef_`), how
//! that degradation is recovered or matched (`result_`), and the tolerance algebra
//! carried alongside it (`tolerance_`). Layout-only — no `#[test]` fn is added or
//! removed. Each former file is included as a stem-named module so its `<file>::<test>`
//! module path (and thus every `test(/^<file>::/)` filterset) resolves unchanged.
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_tolerance/` subdir.
//!
//! `include_str!` is resolved relative to the *including source file*, so the extra
//! subdirectory level this consolidation introduces means every call site in the modules
//! below was deepened by one `../`: `fixtures/…` → `../fixtures/…` in
//! `undef_boundary_suite`, `undef_cause_capture`, `undef_cause_op_contract` and
//! `undef_tracer`, and `../../../examples/…` → `../../../../examples/…` in
//! `tolerance_member_access_e2e`. `crates/reify-eval/tests/fixtures/` itself did not
//! move. A wrong `../` depth is a compile error rather than a silent skip, but these
//! modules were RUN as well as compiled so the fixture *content* is confirmed to still
//! reach the assertions. The `tolerance_member_access_e2e` site is the one place where
//! the extra `../` had a knock-on effect: it pushed that line past rustfmt's `max_width`,
//! so the `const` was wrapped onto two lines.
//!
//! Whole-unit size (this root plus every `harness_tolerance/*.rs` module below) is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
#[path = "harness_tolerance/result_carried_topology.rs"]
mod result_carried_topology;
#[path = "harness_tolerance/result_fallback_e2e.rs"]
mod result_fallback_e2e;
#[path = "harness_tolerance/result_fallback_orthogonality_boundary.rs"]
mod result_fallback_orthogonality_boundary;
#[path = "harness_tolerance/result_match_e2e.rs"]
mod result_match_e2e;
#[path = "harness_tolerance/result_recovery_e2e.rs"]
mod result_recovery_e2e;
#[path = "harness_tolerance/tolerance_combine.rs"]
mod tolerance_combine;
#[path = "harness_tolerance/tolerance_dispatch_budget.rs"]
mod tolerance_dispatch_budget;
#[path = "harness_tolerance/tolerance_import_promise.rs"]
mod tolerance_import_promise;
#[path = "harness_tolerance/tolerance_member_access_e2e.rs"]
mod tolerance_member_access_e2e;
#[path = "harness_tolerance/tolerance_scope.rs"]
mod tolerance_scope;
#[path = "harness_tolerance/tolerance_wiring_e2e.rs"]
mod tolerance_wiring_e2e;
#[path = "harness_tolerance/undef_boundary_suite.rs"]
mod undef_boundary_suite;
#[path = "harness_tolerance/undef_cause_capture.rs"]
mod undef_cause_capture;
#[path = "harness_tolerance/undef_cause_op_contract.rs"]
mod undef_cause_op_contract;
#[path = "harness_tolerance/undef_tracer.rs"]
mod undef_tracer;
#[path = "harness_tolerance/value_cell_type_invariants.rs"]
mod value_cell_type_invariants;
#[path = "harness_tolerance/value_eval_consumes_minted_selector.rs"]
mod value_eval_consumes_minted_selector;
#[path = "harness_tolerance/value_eval_unresolved_consumer.rs"]
mod value_eval_unresolved_consumer;
#[path = "harness_tolerance/value_pow_eval_tests.rs"]
mod value_pow_eval_tests;

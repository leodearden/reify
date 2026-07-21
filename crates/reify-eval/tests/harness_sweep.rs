//! Consolidated integration-test harness for the sweep subsystem.
//!
//! Task #5278 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-1):
//! folds the former standalone sweep_/swept_ tests/*.rs binaries into this single compile
//! unit to cut the merge-gate link count. Layout-only — no `#[test]` fn added or removed.
//! Each former file is included as a stem-named module (explicit `#[path]`, required at a
//! crate root) so its `<file>::<test>` module path resolves unchanged.
#[path = "harness_sweep/sweep_api_smoke.rs"]
mod sweep_api_smoke;
#[path = "harness_sweep/sweep_e2e.rs"]
mod sweep_e2e;
#[path = "harness_sweep/sweep_guided_e2e.rs"]
mod sweep_guided_e2e;
#[path = "harness_sweep/swept_kind_classifier_e2e.rs"]
mod swept_kind_classifier_e2e;

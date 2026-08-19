//! Consolidated integration-test harness for the stdlib unit-declaration subsystem
//! (`stdlib/units.ri` and the `UnitRegistry` it populates).
//!
//! Task #5786 (angle-units θ; C1 contract in tests/infra/test_harness_kloc_cap.sh, PRD
//! docs/prds/merge-gate-compile-cost.md §5): θ's `Nm` torque-unit pin is landed HERE, as a
//! C1-sanctioned compile unit, rather than as a top-level standalone
//! `tests/torque_unit_tests.rs`. That standalone form was flagged
//! `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh; the
//! sanctioned remedy is consolidation, NOT a new
//! tests/infra/harness-layout-baseline.manifest grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to
//! grow). No `#[test]` fn is added or removed relative to the standalone form
//! (invariant I3); the file keeps its original stem, so its post-consolidation selector
//! is `torque_unit_tests::<test>`.
//!
//! This is also the pre-registered home for task μ (#5789)'s round-trip property test —
//! `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3 already
//! bound μ to `crates/reify-compiler/tests/harness_units/unit_label_round_trip.rs` behind
//! this same `harness_units.rs` root, so μ should add its module here rather than create
//! a second unit-subsystem harness.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_units/` subdir. The shared `common` helper module is declared here (via
//! `#[path = "common/mod.rs"]`, the harness_langcore.rs / harness_patterns.rs precedent)
//! because torque_unit_tests.rs uses it; absorbed modules import it as `use
//! crate::common::…` rather than declaring their own `mod common;` (which would load the
//! same source file twice in this one compile unit — `clippy::duplicate_mod` rejects that).
#[path = "common/mod.rs"]
mod common;

#[path = "harness_units/torque_unit_tests.rs"]
mod torque_unit_tests;

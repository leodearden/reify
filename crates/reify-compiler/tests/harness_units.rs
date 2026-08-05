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
//! Routing vs. the sibling `harness_units_materials.rs` root — read this before adding a
//! unit test. The two unit-flavoured roots exist by concurrent construction, not by design:
//! task #5284 (leaf CMP-2, compiler test-harness consolidation batch 2 of 6) swept the
//! pre-existing `unit_declaration_tests.rs` / `unit_registry_tests.rs` into
//! `harness_units_materials.rs`, a separate root, because this file did not exist at that
//! batch's fork base — #5786 created it on main while the sweep was in flight. The line
//! between the two roots is subsystem, not filename prefix:
//!   - THIS root is the angle-units cluster-C home, bound by
//!     `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3. It pins
//!     the stdlib unit *surface* — which symbols `stdlib/units.ri` ships (θ's `Nm`), and how
//!     the four display-label surfaces round-trip (#5789, pre-bound to
//!     `harness_units/unit_label_round_trip.rs`).
//!   - `harness_units_materials.rs` holds the compiler-side unit *machinery* —
//!     `UnitEntry` / `UnitRegistry`, the `unit`-declaration pre-pass, dimension resolution —
//!     beside the materials / money / cost / affine clusters it was swept with.
//! The "μ should add its module here rather than create a second unit-subsystem harness"
//! steer above is scoped to #5789's angle-units work only — it is NOT an invitation to move
//! `unit_declaration_tests.rs` / `unit_registry_tests.rs` (or any future compiler-machinery
//! test) here. Do NOT fold the two roots together: #5789's M1 binding and this root's
//! `harness_units` delivered_check both require this file to keep existing.
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

// Task #5095 (ai-native-editing β): the `.ri` source-literal serializer's
// parse-back round-trip property. Consolidated here rather than landed as a
// standalone `tests/ri_literal_roundtrip.rs` — that form is
// `reason=unregistered-standalone` to
// scripts/check-harness-baseline-registration.sh, and the sanctioned remedy is
// consolidation, not a new baseline grandfather row. `harness_units` is the
// right root because the property under test is exactly that every unit symbol
// the serializer emits re-parses as the same bare built-in; it needs no
// `common` helper (it drives `reify_test_support::eval_source` directly), so it
// does not re-declare one.
#[path = "harness_units/ri_literal_roundtrip.rs"]
mod ri_literal_roundtrip;

#[path = "harness_units/unit_middot_mul_tests.rs"]
mod unit_middot_mul_tests;

#[path = "harness_units/volume_unit_tests.rs"]
mod volume_unit_tests;

//! Consolidated integration-test harness for the M8/M9 milestone acceptance corpora.
//!
//! Every module here takes `.ri` source through the full parse → compile → eval/check
//! pipeline and asserts on the resulting values, types and diagnostics. Seven read real
//! `examples/*.ri` design files as their corpus; `m8_m11_regression_checkpoint` drives
//! one inline source that exercises a feature from each of M8 through M11. A test that
//! instead pins an engine-level entry point against inputs it constructs belongs in
//! `harness_engine`, which this unit was split out of (#7654). Grouping follows
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1: one harness per subsystem module
//! prefix, here `m8_`/`m9_`. The other milestone corpora (`m5_`, `m6_`, `m10_`, `m11_`)
//! are top-level standalones grandfathered in
//! `tests/infra/harness-layout-baseline.manifest`, not members of this unit.
//!
//! Each module is included under its own file stem, so its `<file>::<test>` paths and
//! `test(/^<file>::/)` filtersets do not depend on which harness binary holds it.
//! Explicit `#[path]` is required: this root is an integration-test crate root, where a
//! bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_milestone_integration/` subdir.
//!
//! Whole-unit size — this root plus every `harness_milestone_integration/*.rs` module
//! below — is measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
//! Re-measure with that guard's `harness_layout_unit_lines` rather than trusting a number
//! pinned here.
//!
//! Module order: alphabetical by stem; no module here uses another.
#[path = "harness_milestone_integration/m8_3_stdlib_integration.rs"]
mod m8_3_stdlib_integration;
#[path = "harness_milestone_integration/m8_4_stdlib_integration.rs"]
mod m8_4_stdlib_integration;
#[path = "harness_milestone_integration/m8_m11_regression_checkpoint.rs"]
mod m8_m11_regression_checkpoint;
#[path = "harness_milestone_integration/m8_stdlib_integration.rs"]
mod m8_stdlib_integration;
#[path = "harness_milestone_integration/m9_combined.rs"]
mod m9_combined;
#[path = "harness_milestone_integration/m9_constraint_def.rs"]
mod m9_constraint_def;
#[path = "harness_milestone_integration/m9_integration.rs"]
mod m9_integration;
#[path = "harness_milestone_integration/m9_trait_conformance.rs"]
mod m9_trait_conformance;

//! Consolidated integration-test harness for the units / materials subsystem
//! (materials / money / unit / cost / affine) — the former standalone
//! `tests/{materials,money,unit,cost,affine}_*.rs` binaries.
//! Task #5284 (leaf CMP-2, batch 2 of 6).
//!
//! Routing vs. the sibling `harness_units.rs` root — read this before adding a unit test.
//! The line between the two is subsystem, not filename:
//!   - `harness_units/` pins the stdlib unit *surface* — which symbols `stdlib/units.ri`
//!     ships, and how the display-label surfaces round-trip. It is bound by
//!     `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3 and must
//!     keep existing; do NOT fold the two roots together.
//!   - THIS root holds the compiler-side unit *machinery* — `UnitEntry` / `UnitRegistry`,
//!     the `unit`-declaration pre-pass, dimension resolution — beside the materials / money
//!     / cost / affine clusters it was swept with.
//!
//! Recorded one-sidedly for now: `harness_units.rs`'s own header still carries pre-split
//! wording that reads as covering this root's `unit_{declaration,registry}_tests.rs`. The
//! reciprocal narrowing is outside this leaf's lock set; task #6019 (ticket
//! tkt_0RS43PW3WV70QHNJT878VTWXCR) owns it.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout (no `#[test]` fn added or removed, invariant I3) — see
//! `tests/infra/test_harness_kloc_cap.sh` C1.
//!
//! Crate-local: this harness owns the shared `common` helper — declared ONCE here, because
//! a per-file `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`). Its seven consumers reach it as `use crate::common[::…]`.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_units_materials/affine_algebra_typing_tests.rs"]
mod affine_algebra_typing_tests;
#[path = "harness_units_materials/affine_composition_order_tests.rs"]
mod affine_composition_order_tests;
#[path = "harness_units_materials/affine_constructor_typing_tests.rs"]
mod affine_constructor_typing_tests;
#[path = "harness_units_materials/cost_aggregation_tests.rs"]
mod cost_aggregation_tests;
#[path = "harness_units_materials/cost_robustness_tradeoff_lowering.rs"]
mod cost_robustness_tradeoff_lowering;
#[path = "harness_units_materials/cost_subtree_aggregate_compile_tests.rs"]
mod cost_subtree_aggregate_compile_tests;
#[path = "harness_units_materials/materials_chemical_tests.rs"]
mod materials_chemical_tests;
#[path = "harness_units_materials/materials_electrical_optionality_tests.rs"]
mod materials_electrical_optionality_tests;
#[path = "harness_units_materials/materials_electrical_tests.rs"]
mod materials_electrical_tests;
#[path = "harness_units_materials/materials_fea_tests.rs"]
mod materials_fea_tests;
#[path = "harness_units_materials/materials_mechanical_tests.rs"]
mod materials_mechanical_tests;
#[path = "harness_units_materials/materials_optical_tests.rs"]
mod materials_optical_tests;
#[path = "harness_units_materials/materials_param_surface_tests.rs"]
mod materials_param_surface_tests;
#[path = "harness_units_materials/materials_thermal_optical_optionality_tests.rs"]
mod materials_thermal_optical_optionality_tests;
#[path = "harness_units_materials/materials_thermal_tests.rs"]
mod materials_thermal_tests;
#[path = "harness_units_materials/money_acceptance_sweep_tests.rs"]
mod money_acceptance_sweep_tests;
#[path = "harness_units_materials/money_arithmetic_tests.rs"]
mod money_arithmetic_tests;
#[path = "harness_units_materials/money_force_diagnostic_tests.rs"]
mod money_force_diagnostic_tests;
#[path = "harness_units_materials/money_units_tests.rs"]
mod money_units_tests;
#[path = "harness_units_materials/unit_declaration_tests.rs"]
mod unit_declaration_tests;
#[path = "harness_units_materials/unit_registry_tests.rs"]
mod unit_registry_tests;

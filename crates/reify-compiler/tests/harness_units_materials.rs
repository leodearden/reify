//! Consolidated integration-test harness for the units / materials subsystem
//! (materials / money / unit / cost / affine) — the former standalone
//! `tests/{materials,money,unit,cost,affine}_*.rs` binaries.
//! Task #5284 (leaf CMP-2, batch 2 of 6).
//!
//! Routing vs. the sibling `harness_units.rs` root — read this before adding a unit test.
//! The two unit-flavoured roots exist by concurrent construction, not by design:
//! `harness_units.rs` did not exist at this batch's fork base (7a21980c88) — task #5786
//! (angle-units θ) created it on main while this sweep was in flight, and it reached this
//! branch via the sync-main step. The line between them is subsystem, not filename:
//!   - `harness_units/` is the angle-units cluster-C home, bound by
//!     `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3. It pins
//!     the stdlib unit *surface* — which symbols `stdlib/units.ri` ships (θ's `Nm`), and how
//!     the four display-label surfaces round-trip (#5789, pre-bound to
//!     `harness_units/unit_label_round_trip.rs`).
//!   - THIS root holds the compiler-side unit *machinery* — `UnitEntry` / `UnitRegistry`,
//!     the `unit`-declaration pre-pass, dimension resolution — beside the materials / money
//!     / cost / affine clusters it was swept with.
//!
//! Do NOT fold the two roots together: #5789's M1 binding and its `harness_units`
//! delivered_check both require `harness_units.rs` to keep existing. That file's "μ should
//! add its module here rather than create a second unit-subsystem harness" steer is aimed at
//! #5789, not at these pre-existing `unit_*_tests.rs`.
//!
//! This boundary is currently recorded ONE-SIDEDLY, and knowingly so: `harness_units.rs`'s own
//! header still declares its scope as "the stdlib unit-declaration subsystem (`stdlib/units.ri`
//! and the `UnitRegistry` it populates)" — wording that literally covers the
//! `unit_declaration_tests.rs` / `unit_registry_tests.rs` modules declared BELOW, in this root.
//! So a reader who opens that file first gets the pre-split rule with no pointer back here.
//! The reciprocal edit is outside this leaf's lock set and lands in a file #5789 is pre-bound
//! to extend, so it is deferred rather than dropped: follow-up ticket
//! tkt_0RS43PW3WV70QHNJT878VTWXCR (escalation id `agent-followup-5284`) narrows that header to
//! the unit *surface* and adds the cross-reference. Until it lands, THIS note is the
//! authoritative routing rule for the pair.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout, though no `#[test]` fn is added or removed: the binary
//! id is now `reify-compiler::harness_units_materials` (was `reify-compiler::<file>`) and the
//! nextest test name gained a module prefix, `<file>::<test>` (was bare `<test>` — every
//! `#[test]` fn here sits at file top level). So a hand-written `binary(…)`/`test(=…)`
//! selector naming a former id must be updated; verify.sh's failed-only retry is unaffected
//! — it derives `test(=…)` at run time from its own attempt-0 and refuses on tree drift (see
//! `scripts/verify.sh` retry_failed_only).
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

//! Consolidated integration-test harness for the result / annotation subsystem
//! (result / objective / expected-type-pushdown / cross-module + cross-sub / annotation /
//! ds-sentinel) — the former standalone
//! `tests/{result,objective,expected,cross,annotation,ds}_*.rs` binaries.
//! Task #5284 (leaf CMP-2, batch 2 of 6).
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout (no `#[test]` fn added or removed, invariant I3) — see
//! `tests/infra/test_harness_kloc_cap.sh` C1.
//!
//! Crate-local: this harness deliberately does NOT declare the shared `common` helper — no
//! member consumes it, and declaring it would charge this unit 363 external lines, which
//! rule (a) counts against the C2 cap, for a module nothing references. (Of the three CMP-2
//! roots only `harness_units_materials` includes it, for its seven real consumers.)
#[path = "harness_result_annotation/annotation_compile_tests.rs"]
mod annotation_compile_tests;
#[path = "harness_result_annotation/annotation_materialization_args_tests.rs"]
mod annotation_materialization_args_tests;
#[path = "harness_result_annotation/annotation_schema_registry_parity.rs"]
mod annotation_schema_registry_parity;
#[path = "harness_result_annotation/cross_module_alias_propagation_tests.rs"]
mod cross_module_alias_propagation_tests;
#[path = "harness_result_annotation/cross_sub_geometry_diagnostic_tests.rs"]
mod cross_sub_geometry_diagnostic_tests;
#[path = "harness_result_annotation/cross_sub_geometry_lowering_tests.rs"]
mod cross_sub_geometry_lowering_tests;
#[path = "harness_result_annotation/ds_sentinel_l0_poison_tests.rs"]
mod ds_sentinel_l0_poison_tests;
#[path = "harness_result_annotation/ds_sentinel_l1_poison_tests.rs"]
mod ds_sentinel_l1_poison_tests;
#[path = "harness_result_annotation/ds_sentinel_l5_boundary_tests.rs"]
mod ds_sentinel_l5_boundary_tests;
#[path = "harness_result_annotation/expected_type_arg_pushdown_tests.rs"]
mod expected_type_arg_pushdown_tests;
#[path = "harness_result_annotation/expected_type_pushdown_integration.rs"]
mod expected_type_pushdown_integration;
#[path = "harness_result_annotation/expected_type_pushdown_let_tests.rs"]
mod expected_type_pushdown_let_tests;
#[path = "harness_result_annotation/objective_conflict.rs"]
mod objective_conflict;
#[path = "harness_result_annotation/objective_dimension_coherence.rs"]
mod objective_dimension_coherence;
#[path = "harness_result_annotation/objective_set_lowering.rs"]
mod objective_set_lowering;
#[path = "harness_result_annotation/result_combinator_overload_tests.rs"]
mod result_combinator_overload_tests;
#[path = "harness_result_annotation/result_combinator_resolution_tests.rs"]
mod result_combinator_resolution_tests;
#[path = "harness_result_annotation/result_fallback_resolution_tests.rs"]
mod result_fallback_resolution_tests;
#[path = "harness_result_annotation/result_match_binder_tests.rs"]
mod result_match_binder_tests;
#[path = "harness_result_annotation/result_prelude_enum_tests.rs"]
mod result_prelude_enum_tests;

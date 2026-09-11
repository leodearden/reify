//! Consolidated integration-test harness for the geometry / solver subsystem
//! (geometry / structural / solver / fdm / surface-finish) — the former standalone
//! `tests/{geometry,structural,solver,fdm,surface}_*.rs` binaries.
//! Task #5284 (leaf CMP-2, batch 2 of 6).
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout (no `#[test]` fn added or removed, invariant I3) — see
//! `tests/infra/test_harness_kloc_cap.sh` C1.
//!
//! Crate-local: this unit deliberately does NOT include the shared `tests/common/` helper.
//! Its one former consumer, `structural_physical_tests`, now calls
//! `reify_test_support::compile_source_with_stdlib` directly (`common`'s wrapper was a pure
//! delegate to it, and that file already imported it for nine other call sites) and inlines
//! its single `expect_binop` use — so the unit no longer pays 363 external lines, which
//! rule (a) counts against the C2 cap, for two call sites.
#[path = "harness_geometry_solver/fdm_as_printed_stdlib_compile.rs"]
mod fdm_as_printed_stdlib_compile;
#[path = "harness_geometry_solver/fdm_correlations_stdlib_compile.rs"]
mod fdm_correlations_stdlib_compile;
#[path = "harness_geometry_solver/fdm_stdlib_compile.rs"]
mod fdm_stdlib_compile;
#[path = "harness_geometry_solver/geometry_arg_count_span_tests.rs"]
mod geometry_arg_count_span_tests;
#[path = "harness_geometry_solver/geometry_centered_primitives_tests.rs"]
mod geometry_centered_primitives_tests;
#[path = "harness_geometry_solver/geometry_let_value_cell_lowering.rs"]
mod geometry_let_value_cell_lowering;
#[path = "harness_geometry_solver/geometry_profile_precondition_tests.rs"]
mod geometry_profile_precondition_tests;
#[path = "harness_geometry_solver/geometry_query_inline_arg_tests.rs"]
mod geometry_query_inline_arg_tests;
#[path = "harness_geometry_solver/geometry_traits_inference_tests.rs"]
mod geometry_traits_inference_tests;
#[path = "harness_geometry_solver/geometry_traits_tests.rs"]
mod geometry_traits_tests;
#[path = "harness_geometry_solver/geometry_traits_user_asserted_tests.rs"]
mod geometry_traits_user_asserted_tests;
#[path = "harness_geometry_solver/solver_elastic_static_stdlib_compile.rs"]
mod solver_elastic_static_stdlib_compile;
#[path = "harness_geometry_solver/solver_elastic_tests.rs"]
mod solver_elastic_tests;
#[path = "harness_geometry_solver/solver_hint_payload_tests.rs"]
mod solver_hint_payload_tests;
#[path = "harness_geometry_solver/solver_hint_tests.rs"]
mod solver_hint_tests;
#[path = "harness_geometry_solver/structural_physical_spec_shape.rs"]
mod structural_physical_spec_shape;
#[path = "harness_geometry_solver/structural_physical_tests.rs"]
mod structural_physical_tests;
#[path = "harness_geometry_solver/structural_query_compile_tests.rs"]
mod structural_query_compile_tests;
#[path = "harness_geometry_solver/structural_query_filter_compile_tests.rs"]
mod structural_query_filter_compile_tests;
#[path = "harness_geometry_solver/surface_finish_cost_tests.rs"]
mod surface_finish_cost_tests;
#[path = "harness_geometry_solver/surface_finish_stdlib_compile.rs"]
mod surface_finish_stdlib_compile;

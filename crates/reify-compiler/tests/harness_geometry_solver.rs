//! Consolidated integration-test harness for the geometry / solver subsystem
//! (geometry / structural / solver / fdm / surface-finish) — the former standalone
//! `tests/{geometry,structural,solver,fdm,surface}_*.rs` binaries.
//! Task #5284 (leaf CMP-2, batch 2 of 6).
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout, though no `#[test]` fn is added or removed: the binary
//! id is now `reify-compiler::harness_geometry_solver` (was `reify-compiler::<file>`) and the
//! nextest test name gained a module prefix, `<file>::<test>` (was bare `<test>` — every
//! `#[test]` fn here sits at file top level). So a hand-written `binary(…)`/`test(=…)`
//! selector naming a former id must be updated; verify.sh's failed-only retry is unaffected
//! — it derives `test(=…)` at run time from its own attempt-0 and refuses on tree drift (see
//! `scripts/verify.sh` retry_failed_only).
//!
//! Crate-local: this harness owns the shared `common` helper — declared ONCE here, because
//! a per-file `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`). Only `structural_physical_tests` consumes it, as
//! `use crate::common;`.
//!
//! Crate-local carve-out — the `geometry*` prefix sweep below has ONE deliberate gap:
//! `tests/geometry_chunk_smoke.rs` stays a top-level standalone binary. It belongs to task
//! #5477's `harness_doc_chunks/` (`docs/prds/v0_6/doc-chunk-truth-enforcement.md` §α), whose
//! own signal is the removal of its baseline row — sweeping it here would satisfy that
//! precondition silently. The gap is intentional; do not "fix" it.
#[path = "common/mod.rs"]
mod common;
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

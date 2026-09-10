//! Consolidated integration-test harness for the FEA/solver-e2e subsystem.
//!
//! Task #5281 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-2):
//! folds the former standalone `tests/<file>.rs` binaries for the fea_/tensegrity_/
//! stress_/objective_/multi_/process_/as_printed_/kinematic_ subsystems into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is added
//! or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) resolves
//! unchanged. Explicit `#[path]` is required: this harness root is an integration-test
//! crate root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`,
//! not the `harness_fea_solver_e2e/` subdir.
//!
//! `common` (tests/common/mod.rs) is declared ONCE here rather than once per includer: 5
//! of the submodules below reference it, and each carrying its own file-backed copy would
//! multiply the compile unit's line count for no behavioral difference. Submodules reach
//! it via `use crate::common::...`.
mod common;

#[path = "harness_fea_solver_e2e/as_printed_body_realization_e2e.rs"]
mod as_printed_body_realization_e2e;
#[path = "harness_fea_solver_e2e/as_printed_material_e2e.rs"]
mod as_printed_material_e2e;
#[path = "harness_fea_solver_e2e/as_printed_r0_trampoline.rs"]
mod as_printed_r0_trampoline;
#[path = "harness_fea_solver_e2e/as_printed_trampoline.rs"]
mod as_printed_trampoline;
// Task #5025's edit-path dispatch guards. A DELIBERATELY separate stem from
// `fea_in_the_loop_producer`, whose test-scoped heavy atom would otherwise sweep
// these two cheap guards off the task/merge gate along with the ~490s producer
// (task #6630; pinned by tests/infra/test_heavy_filter_atoms.sh Assertion G).
#[path = "harness_fea_solver_e2e/edit_path_optimized_dispatch.rs"]
mod edit_path_optimized_dispatch;
#[path = "harness_fea_solver_e2e/fea_bracket_minimize_mass_e2e.rs"]
mod fea_bracket_minimize_mass_e2e;
#[path = "harness_fea_solver_e2e/fea_cold_start_heuristic_e2e.rs"]
mod fea_cold_start_heuristic_e2e;
// Carries no `#[test]` — shared wiring for the FEA-in-the-loop design-optimisation
// modules (see its own doc). Declared here like every other submodule so the
// harness's `#[path]` layout contract holds uniformly.
#[path = "harness_fea_solver_e2e/fea_design_loop_support.rs"]
mod fea_design_loop_support;
#[path = "harness_fea_solver_e2e/fea_face_selector_bc_e2e.rs"]
mod fea_face_selector_bc_e2e;
#[path = "harness_fea_solver_e2e/fea_in_the_loop_producer.rs"]
mod fea_in_the_loop_producer;
#[path = "harness_fea_solver_e2e/fea_loads_stdlib_smoke.rs"]
mod fea_loads_stdlib_smoke;
#[path = "harness_fea_solver_e2e/fea_stress_reductions_smoke.rs"]
mod fea_stress_reductions_smoke;
#[path = "harness_fea_solver_e2e/fea_structured_detail_e2e.rs"]
mod fea_structured_detail_e2e;
#[path = "harness_fea_solver_e2e/jacobian_column_member_access.rs"]
mod jacobian_column_member_access;
#[path = "harness_fea_solver_e2e/kinematic_diagnostics_e2e.rs"]
mod kinematic_diagnostics_e2e;
#[path = "harness_fea_solver_e2e/kinematic_examples_e2e.rs"]
mod kinematic_examples_e2e;
#[path = "harness_fea_solver_e2e/kinematic_loop_closure_machinery.rs"]
mod kinematic_loop_closure_machinery;
#[path = "harness_fea_solver_e2e/kinematic_singularity_snapshot_e2e.rs"]
mod kinematic_singularity_snapshot_e2e;
#[path = "harness_fea_solver_e2e/kinematic_stdlib_smoke.rs"]
mod kinematic_stdlib_smoke;
#[path = "harness_fea_solver_e2e/kinematic_sweep_closed_chain.rs"]
mod kinematic_sweep_closed_chain;
#[path = "harness_fea_solver_e2e/multi_aspect_objective_example_e2e.rs"]
mod multi_aspect_objective_example_e2e;
#[path = "harness_fea_solver_e2e/multi_case_compute_node.rs"]
mod multi_case_compute_node;
#[path = "harness_fea_solver_e2e/multi_handle_engine_dispatch.rs"]
mod multi_handle_engine_dispatch;
#[path = "harness_fea_solver_e2e/multi_load_bracket_e2e.rs"]
mod multi_load_bracket_e2e;
#[path = "harness_fea_solver_e2e/multi_load_case_stdlib_smoke.rs"]
mod multi_load_case_stdlib_smoke;
#[path = "harness_fea_solver_e2e/multi_load_case_typed_envelope.rs"]
mod multi_load_case_typed_envelope;
#[path = "harness_fea_solver_e2e/objective_inherit_ambiguous.rs"]
mod objective_inherit_ambiguous;
#[path = "harness_fea_solver_e2e/objective_inheritance.rs"]
mod objective_inheritance;
#[path = "harness_fea_solver_e2e/objective_provenance.rs"]
mod objective_provenance;
#[path = "harness_fea_solver_e2e/objective_set_signal.rs"]
mod objective_set_signal;
#[path = "harness_fea_solver_e2e/stress_dimensional_chains.rs"]
mod stress_dimensional_chains;
#[path = "harness_fea_solver_e2e/stress_error_messages.rs"]
mod stress_error_messages;
#[path = "harness_fea_solver_e2e/stress_large_assembly.rs"]
mod stress_large_assembly;
#[path = "harness_fea_solver_e2e/stress_pattern_composition.rs"]
mod stress_pattern_composition;
#[path = "harness_fea_solver_e2e/stress_query_consistency.rs"]
mod stress_query_consistency;
#[path = "harness_fea_solver_e2e/stress_sweep_degenerate.rs"]
mod stress_sweep_degenerate;
#[path = "harness_fea_solver_e2e/stress_trait_hierarchy.rs"]
mod stress_trait_hierarchy;
#[path = "harness_fea_solver_e2e/tensegrity_delta_combined_form_find_e2e.rs"]
mod tensegrity_delta_combined_form_find_e2e;
#[path = "harness_fea_solver_e2e/tensegrity_force_density_gauge.rs"]
mod tensegrity_force_density_gauge;
#[path = "harness_fea_solver_e2e/tensegrity_membrane_load.rs"]
mod tensegrity_membrane_load;
#[path = "harness_fea_solver_e2e/tensegrity_pavilion_e2e.rs"]
mod tensegrity_pavilion_e2e;
#[path = "harness_fea_solver_e2e/tensegrity_t1a_form_find.rs"]
mod tensegrity_t1a_form_find;
#[path = "harness_fea_solver_e2e/tensegrity_t1b_form_find_e2e.rs"]
mod tensegrity_t1b_form_find_e2e;
#[path = "harness_fea_solver_e2e/tensegrity_t3b_load.rs"]
mod tensegrity_t3b_load;

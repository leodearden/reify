//! Consolidated integration-test harness for the auto-binding subsystem.
//!
//! Task #5283 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-1,
//! batch 1 of 2): folds the former standalone `tests/auto_*.rs` binaries into this
//! single compile unit to cut the merge-gate link count. Layout-only — no `#[test]`
//! fn is added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) resolves
//! unchanged. Explicit `#[path]` is required: this harness root is an integration-test
//! crate root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`,
//! not the `harness_auto_binding/` subdir.
#[path = "harness_auto_binding/auto_backjumping_real_source.rs"]
mod auto_backjumping_real_source;
#[path = "harness_auto_binding/auto_binding_sites_remaining_tests.rs"]
mod auto_binding_sites_remaining_tests;
#[path = "harness_auto_binding/auto_fallback_soundness.rs"]
mod auto_fallback_soundness;
#[path = "harness_auto_binding/auto_not_at_binding_site_tests.rs"]
mod auto_not_at_binding_site_tests;
#[path = "harness_auto_binding/auto_sub_override_tests.rs"]
mod auto_sub_override_tests;
#[path = "harness_auto_binding/auto_type_arg_lowering_tests.rs"]
mod auto_type_arg_lowering_tests;
#[path = "harness_auto_binding/auto_type_param_backtracking_tests.rs"]
mod auto_type_param_backtracking_tests;
#[path = "harness_auto_binding/auto_type_param_checker_inject_tests.rs"]
mod auto_type_param_checker_inject_tests;
#[path = "harness_auto_binding/auto_type_param_member_access_tests.rs"]
mod auto_type_param_member_access_tests;
#[path = "harness_auto_binding/auto_type_param_monomorphize_tests.rs"]
mod auto_type_param_monomorphize_tests;
#[path = "harness_auto_binding/auto_type_param_multi_param_tests.rs"]
mod auto_type_param_multi_param_tests;
#[path = "harness_auto_binding/auto_type_param_per_candidate_valuemap_tests.rs"]
mod auto_type_param_per_candidate_valuemap_tests;
#[path = "harness_auto_binding/auto_type_param_phase_a_tests.rs"]
mod auto_type_param_phase_a_tests;
#[path = "harness_auto_binding/auto_type_param_phase_b_tests.rs"]
mod auto_type_param_phase_b_tests;
#[path = "harness_auto_binding/auto_type_param_phase_c_tests.rs"]
mod auto_type_param_phase_c_tests;
#[path = "harness_auto_binding/auto_type_param_template_literal_seed_tests.rs"]
mod auto_type_param_template_literal_seed_tests;
#[path = "harness_auto_binding/auto_type_param_unevaluated_constraint_tests.rs"]
mod auto_type_param_unevaluated_constraint_tests;
#[path = "harness_auto_binding/auto_type_params_max_depth_config.rs"]
mod auto_type_params_max_depth_config;

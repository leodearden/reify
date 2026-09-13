//! Consolidated integration-test harness for COMPILER DIAGNOSTICS AND ROBUSTNESS — the
//! remainder leaf: diagnostic coverage and unresolved-code auditing, guard and termination
//! checking, silent-default and ambient-default injection, implicit conversion and
//! param-default/selector-coercion mismatches, `cfg` entry and import gating, cross-module
//! boundary producer/consumer pairs, deep dot chains, determinacy and prepass consolidation,
//! optimized-annotation lowering, and the multi-load / robustness worked examples.
//!
//! Task #5696 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-6,
//! batch 6 of 6): folds the last 22 standalone `tests/*.rs` binaries in this crate into
//! this single compile unit to cut the merge-gate link count. With this leaf,
//! `crates/reify-compiler/tests/` holds no standalone test binaries at all and the CMP-*
//! series closes.
//!
//! Layout contract C1 — stem-named modules, why the `#[path]` on every declaration below
//! is mandatory rather than stylistic, the kLOC cap, the baseline ratchet, and the
//! by-design test-id change (no `#[test]` fn added or removed, invariant I3): see
//! `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, kept there, not restated here.
//!
//! Crate-local: this harness deliberately does NOT declare the shared `common` helper — no
//! member consumes it, and declaring it would charge this unit the 391 external lines of
//! `tests/common/mod.rs`, which rule (a) counts against the C2 cap, for a module nothing
//! references. `harness_result_annotation` makes the same call for the same reason.
#[path = "harness_diagnostics_robustness/ambient_default_injection_tests.rs"]
mod ambient_default_injection_tests;
#[path = "harness_diagnostics_robustness/ambient_default_material_integration_gate.rs"]
mod ambient_default_material_integration_gate;
#[path = "harness_diagnostics_robustness/boundary1_consumer.rs"]
mod boundary1_consumer;
#[path = "harness_diagnostics_robustness/boundary2_producer.rs"]
mod boundary2_producer;
#[path = "harness_diagnostics_robustness/cfg_check_entry_tests.rs"]
mod cfg_check_entry_tests;
#[path = "harness_diagnostics_robustness/cfg_import_gating_tests.rs"]
mod cfg_import_gating_tests;
#[path = "harness_diagnostics_robustness/deep_dot_chain_tests.rs"]
mod deep_dot_chain_tests;
#[path = "harness_diagnostics_robustness/determinacy_compile_tests.rs"]
mod determinacy_compile_tests;
#[path = "harness_diagnostics_robustness/diagnostic_coverage_checkpoint.rs"]
mod diagnostic_coverage_checkpoint;
#[path = "harness_diagnostics_robustness/guard_compilation.rs"]
mod guard_compilation;
#[path = "harness_diagnostics_robustness/implicit_conversion_tests.rs"]
mod implicit_conversion_tests;
#[path = "harness_diagnostics_robustness/m9_error_cases.rs"]
mod m9_error_cases;
#[path = "harness_diagnostics_robustness/multi_load_bracket_example_tests.rs"]
mod multi_load_bracket_example_tests;
#[path = "harness_diagnostics_robustness/multi_load_case_stdlib_tests.rs"]
mod multi_load_case_stdlib_tests;
#[path = "harness_diagnostics_robustness/optimized_annotation_tests.rs"]
mod optimized_annotation_tests;
#[path = "harness_diagnostics_robustness/param_binding_selector_coercion_tests.rs"]
mod param_binding_selector_coercion_tests;
#[path = "harness_diagnostics_robustness/param_default_type_mismatch_tests.rs"]
mod param_default_type_mismatch_tests;
#[path = "harness_diagnostics_robustness/prepass_consolidation_tests.rs"]
mod prepass_consolidation_tests;
#[path = "harness_diagnostics_robustness/robustness_examples_tests.rs"]
mod robustness_examples_tests;
#[path = "harness_diagnostics_robustness/selector_composition_tests.rs"]
mod selector_composition_tests;
#[path = "harness_diagnostics_robustness/silent_defaults_tests.rs"]
mod silent_defaults_tests;
#[path = "harness_diagnostics_robustness/termination_check_tests.rs"]
mod termination_check_tests;
#[path = "harness_diagnostics_robustness/unresolved_diagnostic_code_audit_tests.rs"]
mod unresolved_diagnostic_code_audit_tests;

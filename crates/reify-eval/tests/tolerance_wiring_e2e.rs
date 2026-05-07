//! End-to-end engine-level integration tests for task 2874 — exercises the
//! production-wired tolerance subsystem: dispatcher emission of import-promise
//! + zero-promise diagnostics on `build()`, `RealizationCache` population /
//! short-circuit keyed on demanded tolerance, and `per_stage_tolerance_for_plan`
//! consumption from the realization loop.
//!
//! Imports use the established test fixture surface
//! (`reify_test_support::{make_engine, step_input_template, step_output_template,
//! my_design_template, manufacturing_purpose}` + `CompiledModuleBuilder`).
//! Per-step tests are added by the subsequent TDD steps.

#[allow(unused_imports)]
use reify_test_support::builders::CompiledModuleBuilder;
#[allow(unused_imports)]
use reify_test_support::{
    make_engine, manufacturing_purpose, my_design_template, step_input_template,
    step_output_template,
};

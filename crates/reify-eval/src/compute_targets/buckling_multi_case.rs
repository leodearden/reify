//! Trampoline for `solver::buckling_multi_case` — the
//! `fn solve_buckling_load_cases` @optimized target (PRD §13 task η,
//! docs/prds/v0_5/buckling-eigensolver.md §7).
//!
//! # Contract
//!
//! Receives the 6 `value_inputs` matching the `fn solve_buckling_load_cases`
//! signature:
//!
//! ```text
//! [0] material : ElasticMaterial   (Value::StructureInstance)
//! [1] length   : Length            (Value::Scalar { dimension: LENGTH })
//! [2] width    : Length            (Value::Scalar { dimension: LENGTH })
//! [3] height   : Length            (Value::Scalar { dimension: LENGTH })
//! [4] cases    : List<LoadCase>    (Value::List of LoadCase StructureInstances)
//! [5] options  : BucklingOptions   (Value::StructureInstance — shared default)
//! ```
//!
//! Arity < 6 → `Failed`. Otherwise `load_cases::run_load_cases` sweeps the
//! cases through `buckling::solve_buckling_trampoline` and owns the shared
//! contract: LoadCase extraction, error propagation and the
//! `Value::Map{"cases" -> Value::Map<String, BucklingResult>}` result consumed by
//! `extract_cases_map`, `worst_buckling_case` and `envelope_critical_load` in
//! `crates/reify-stdlib/src/fea.rs`.
//!
//! Each case is dispatched as
//! `[material, length, width, height, loads, supports, shared_options]`.
//! `LoadCase.options` carries `Option<ElasticOptions>` — wrong type for
//! buckling — so per-case option overrides are not applicable; the shared
//! `BucklingOptions` (`value_inputs[5]`) governs every case uniformly (design
//! decision DD-4; task #7082 owns honouring or rejecting the override).

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use super::load_cases::{LoadCaseSweep, run_load_cases};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

const SWEEP: LoadCaseSweep = LoadCaseSweep {
    label: "solve_buckling_load_cases (solver::buckling_multi_case)",
    empty_cases_message: "Multi-load-case buckling analysis requires at least one LoadCase. \
                          Use solve_buckling for single-case analysis.",
    solve_case: super::buckling::solve_buckling_trampoline,
};

/// Trampoline for `solver::buckling_multi_case`.
///
/// Sweeps the runtime `List<LoadCase>` in `value_inputs[4]` through the
/// `buckling` trampoline with the shared `BucklingOptions` (`value_inputs[5]`),
/// and collects the per-case `Completed.result` into a `Value::Map{"cases"→Map}`
/// matching the `MultiCaseBucklingResult` runtime shape.
///
/// See module-level docs for the full contract.
pub fn solve_buckling_multi_case_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    if value_inputs.len() < 6 {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!(
                "{}: expected 6 value_inputs, got {} — possible arity mismatch at dispatch site",
                SWEEP.label,
                value_inputs.len()
            ))],
            structured_detail: vec![],
        };
    }

    let material_and_dims = &value_inputs[0..4];
    let shared_options = &value_inputs[5];
    run_load_cases(
        &SWEEP,
        &value_inputs[4],
        |parts| {
            let mut inputs = material_and_dims.to_vec();
            inputs.extend([parts.loads, parts.supports, shared_options.clone()]);
            inputs
        },
        realization_inputs,
        cancellation,
    )
}

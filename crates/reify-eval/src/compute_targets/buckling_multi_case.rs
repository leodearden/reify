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
//! For each `LoadCase` in `cases`:
//!   1. Extracts `name` (String), `loads` (Value), `supports` (Value).
//!   2. Builds 7 per-case inputs:
//!      `[material, length, width, height, loads, supports, shared_options]`.
//!      `LoadCase.options` carries `Option<ElasticOptions>` — wrong type for
//!      buckling — so per-case option overrides are not applicable; the shared
//!      `BucklingOptions` (`value_inputs[5]`) governs every case uniformly
//!      (design decision DD-4 in .task/plan.json).
//!   3. Calls `super::buckling::solve_buckling_trampoline` for this case.
//!   4. Collects `Completed.result` (a `BucklingResult` `StructureInstance`)
//!      into a `BTreeMap<String→Value>`.
//!
//! Returns `ComputeOutcome::Completed` with:
//! - `result`         — `Value::Map{"cases" -> Value::Map<String, BucklingResult>}`
//!   matching the shape expected by `extract_cases_map`, `worst_buckling_case`,
//!   and `envelope_critical_load` in `crates/reify-stdlib/src/fea.rs`.
//! - `new_warm_state` — `None` (no cross-case warm-state reuse in this slice).
//! - `cost_per_byte`  — `None`.
//!
//! # Error propagation
//!
//! - Arity < 6 → `Failed`.
//! - `cases` is not `Value::List` → `Failed`.
//! - Empty `cases` list → `Failed` (mirrors `solve_multi_case_trampoline`).
//! - A case is not `Value::StructureInstance` → `Failed`.
//! - `LoadCase.name` is not `Value::String` → `Failed`.
//! - `LoadCase.loads` or `LoadCase.supports` absent → `Failed`.
//! - Duplicate `LoadCase.name` → accumulated warning, later result overwrites
//!   earlier (mirrors `solve_multi_case_trampoline`).
//! - Sub-`Cancelled` → propagate `Cancelled` immediately.
//! - Sub-`Failed` → propagate `Failed` immediately (no partial results).
//!
//! # Structural template
//!
//! Mirrors `compute_targets/multi_case.rs::solve_multi_case_trampoline` with
//! two changes:
//!   - Per-case dispatch calls `super::buckling::solve_buckling_trampoline`
//!     instead of `super::elastic_static::solve_elastic_static_trampoline`.
//!   - The shared `BucklingOptions` (`value_inputs[5]`) is passed directly to
//!     every case; there is no per-case option resolution (LoadCase.options
//!     is typed `Option<ElasticOptions>`, the wrong knob type for buckling).

use std::collections::BTreeMap;

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::buckling_multi_case`.
///
/// Iterates the runtime `List<LoadCase>` in `value_inputs[4]`, dispatches
/// the `buckling` trampoline per case with the shared `BucklingOptions`
/// (`value_inputs[5]`), and collects the per-case `Completed.result` into a
/// `Value::Map{"cases"→Map}` matching the `MultiCaseBucklingResult` runtime
/// shape.
///
/// See module-level docs for the full contract.
pub fn solve_buckling_multi_case_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (1) Validate arity ─────────────────────────────────────────────────────
    if value_inputs.len() < 6 {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!(
                "solve_buckling_load_cases (solver::buckling_multi_case): \
                 expected 6 value_inputs, got {} — possible arity mismatch at dispatch site",
                value_inputs.len()
            ))],
        };
    }

    let material = &value_inputs[0];
    let length = &value_inputs[1];
    let width = &value_inputs[2];
    let height = &value_inputs[3];
    let cases_val = &value_inputs[4];
    let shared_options = &value_inputs[5];

    // ── (2) Unwrap cases list ──────────────────────────────────────────────────
    let cases = match cases_val {
        Value::List(v) => v,
        other => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "solve_buckling_load_cases (solver::buckling_multi_case): \
                     cases argument must be Value::List, got {:?}",
                    std::mem::discriminant(other)
                ))],
            };
        }
    };

    if cases.is_empty() {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(
                "Multi-load-case buckling analysis requires at least one LoadCase. \
                 Use solve_buckling for single-case analysis.",
            )],
        };
    }

    // ── (3) Iterate cases, dispatch buckling trampoline per case ───────────────
    let mut inner: BTreeMap<Value, Value> = BTreeMap::new();
    let mut accumulated_diagnostics: Vec<Diagnostic> = Vec::new();

    for case_val in cases.iter() {
        // Cooperative cancellation: allow long batches to be interrupted between
        // sub-solves.
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }

        // ── Extract LoadCase fields ────────────────────────────────────────────
        let data = match case_val {
            Value::StructureInstance(d) => d,
            other => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_buckling_load_cases (solver::buckling_multi_case): \
                         each case must be a LoadCase Value::StructureInstance, got {:?}",
                        std::mem::discriminant(other)
                    ))],
                };
            }
        };

        let name = match data.fields.get(&"name".to_string()) {
            Some(Value::String(s)) => s.clone(),
            other => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_buckling_load_cases (solver::buckling_multi_case): \
                         LoadCase.name must be Value::String, got {:?}",
                        other.map(std::mem::discriminant)
                    ))],
                };
            }
        };

        let loads = match data.fields.get(&"loads".to_string()) {
            Some(v) => v.clone(),
            None => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_buckling_load_cases (solver::buckling_multi_case): \
                         LoadCase \"{name}\" is missing the \"loads\" field"
                    ))],
                };
            }
        };

        let supports = match data.fields.get(&"supports".to_string()) {
            Some(v) => v.clone(),
            None => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_buckling_load_cases (solver::buckling_multi_case): \
                         LoadCase \"{name}\" is missing the \"supports\" field"
                    ))],
                };
            }
        };

        // ── Per-case inputs for solve_buckling_trampoline ─────────────────────
        //
        // The trampoline expects 7 value_inputs:
        //   [0] material, [1] length, [2] width, [3] height,
        //   [4] loads, [5] supports, [6] options (BucklingOptions).
        //
        // `LoadCase.options` carries `Option<ElasticOptions>` — the wrong type
        // for buckling — so we always use the shared `BucklingOptions`
        // (`value_inputs[5]`) for every case (design decision DD-4).
        let per_case_inputs: Vec<Value> = vec![
            material.clone(),
            length.clone(),
            width.clone(),
            height.clone(),
            loads,
            supports,
            shared_options.clone(),
        ];

        let outcome = super::buckling::solve_buckling_trampoline(
            &per_case_inputs,
            realization_inputs,
            &Value::Undef,
            None, // cold — no cross-case warm-state reuse in this slice
            cancellation,
        );

        match outcome {
            ComputeOutcome::Completed {
                result,
                diagnostics: case_diags,
                ..
            } => {
                accumulated_diagnostics.extend(case_diags);
                let key = Value::String(name.clone());
                if inner.contains_key(&key) {
                    accumulated_diagnostics.push(Diagnostic::warning(format!(
                        "solve_buckling_load_cases (solver::buckling_multi_case): \
                         duplicate LoadCase name \"{name}\" — the earlier result for \
                         this case is overwritten; this is almost certainly a user error"
                    )));
                }
                inner.insert(key, result);
            }
            ComputeOutcome::Cancelled => return ComputeOutcome::Cancelled,
            ComputeOutcome::Failed {
                diagnostics: fail_diags,
            } => {
                let mut all_diags = accumulated_diagnostics;
                all_diags.extend(fail_diags);
                return ComputeOutcome::Failed {
                    diagnostics: all_diags,
                };
            }
        }
    }

    // ── (4) Wrap as MultiCaseBucklingResult shape ──────────────────────────────
    //
    // Emits Value::Map{"cases" -> Map<String, BucklingResult>} — the runtime
    // MultiCaseBucklingResult contract consumed by extract_cases_map,
    // worst_buckling_case, and envelope_critical_load in reify-stdlib/src/fea.rs.
    let mut outer: BTreeMap<Value, Value> = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(inner));

    ComputeOutcome::Completed {
        result: Value::Map(outer),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: accumulated_diagnostics,
    }
}

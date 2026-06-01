//! Trampoline for `solver::multi_case` — the `fn solve_load_cases`
//! @optimized target (task 4088, esc-4088-231 decision A).
//!
//! # Contract
//!
//! Receives the 6 `value_inputs` matching the `fn solve_load_cases` signature:
//!
//! ```text
//! [0] material : ConstitutiveLaw   (Value::StructureInstance)
//! [1] length   : Length            (Value::Scalar { dimension: LENGTH })
//! [2] width    : Length            (Value::Scalar { dimension: LENGTH })
//! [3] height   : Length            (Value::Scalar { dimension: LENGTH })
//! [4] cases    : List<LoadCase>    (Value::List of LoadCase StructureInstances)
//! [5] options  : ElasticOptions    (Value::StructureInstance — shared default)
//! ```
//!
//! For each `LoadCase` in `cases`:
//!   1. Extracts `name` (String), `loads` (Value), `supports` (Value).
//!   2. Resolves effective options: `LoadCase.options == Option(Some(X))` → `X`;
//!      otherwise inherits `value_inputs[5]` (mirrors `resolve_load_case_options`
//!      in `crates/reify-expr/src/lib.rs`).
//!   3. Calls `elastic_static::solve_elastic_static_trampoline` directly with
//!      `[material, length, width, height, loads, supports, effective_options]`.
//!   4. Collects `Completed.result` (a real Sampled-field `ElasticResult`
//!      `Value::StructureInstance`) into an inner `BTreeMap<String→Value>`.
//!
//! Returns `ComputeOutcome::Completed` with:
//! - `result`         — `Value::Map{"cases" -> Value::Map<String, ElasticResult>}`
//!   matching the shape expected by `detect_multi_case_result`,
//!   `extract_cases_map`, `multi_case_result_value`, and all existing consumers.
//! - `new_warm_state` — `None` (cross-case warm-state reuse re-homed to task 4152,
//!   esc-4088-231 decision A).
//! - `cost_per_byte`  — `None`.
//!
//! # Per-case warm state
//!
//! Each sub-solve is cold (`prior_warm_state = None`). Cross-case warm-state
//! and realization-cache reuse (the B9 intent) are explicitly deferred to task
//! 4152 (gated on 4088 + 4091).  In v1 every case invokes the `elastic_static`
//! trampoline fresh.
//!
//! # Error propagation
//!
//! - Empty `cases` list → `Failed` with a descriptive diagnostic (best-effort
//!   parity with `eval_solve_load_cases`).
//! - A case is not a `Value::StructureInstance` → `Failed`.
//! - `LoadCase.name` is not a `Value::String` → `Failed`.
//! - `LoadCase.loads` or `LoadCase.supports` absent → `Failed`.
//! - Sub-`Cancelled` → propagate `Cancelled` immediately.
//! - Sub-`Failed` → propagate `Failed` immediately (no partial results).
//!
//! # Placement rationale
//!
//! See `compute_targets/mod.rs` for why trampolines live in `reify-eval` rather
//! than `reify-stdlib` (cycle-free dependency graph).

use std::collections::BTreeMap;

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::multi_case`.
///
/// Iterates the runtime `List<LoadCase>` in `value_inputs[4]`, dispatches
/// the `elastic_static` trampoline per case, and collects the per-case
/// `Completed.result` into a `Value::Map{"cases"→Map}` matching the
/// `MultiCaseResult` runtime shape.
///
/// See module-level docs for the full contract.
pub fn solve_multi_case_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (1) Validate arity ────────────────────────────────────────────────────
    if value_inputs.len() < 6 {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!(
                "solve_load_cases (solver::multi_case): expected 6 value_inputs, \
                 got {} — possible arity mismatch at dispatch site",
                value_inputs.len()
            ))],
        };
    }

    let material       = &value_inputs[0];
    let length         = &value_inputs[1];
    let width          = &value_inputs[2];
    let height         = &value_inputs[3];
    let cases_val      = &value_inputs[4];
    let shared_options = &value_inputs[5];

    // ── (2) Unwrap cases list ─────────────────────────────────────────────────
    let cases = match cases_val {
        Value::List(v) => v,
        other => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "solve_load_cases (solver::multi_case): cases argument must be \
                     Value::List, got {:?}",
                    std::mem::discriminant(other)
                ))],
            };
        }
    };

    if cases.is_empty() {
        return ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(
                "Multi-load case analysis requires at least one LoadCase. \
                 Use solve_elastic_static for single-case analysis.",
            )],
        };
    }

    // ── (3) Iterate cases, dispatch elastic_static per case ──────────────────
    let mut inner: BTreeMap<Value, Value> = BTreeMap::new();
    let mut accumulated_diagnostics: Vec<Diagnostic> = Vec::new();

    for case_val in cases.iter() {
        // ── Extract LoadCase fields ───────────────────────────────────────────
        let data = match case_val {
            Value::StructureInstance(d) => d,
            other => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): each case must be a \
                         LoadCase Value::StructureInstance, got {:?}",
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
                        "solve_load_cases (solver::multi_case): LoadCase.name must be \
                         Value::String, got {:?}",
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
                        "solve_load_cases (solver::multi_case): LoadCase \"{name}\" \
                         is missing the \"loads\" field"
                    ))],
                };
            }
        };

        let supports = match data.fields.get(&"supports".to_string()) {
            Some(v) => v.clone(),
            None => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(format!(
                        "solve_load_cases (solver::multi_case): LoadCase \"{name}\" \
                         is missing the \"supports\" field"
                    ))],
                };
            }
        };

        // ── Resolve effective options (mirrors resolve_load_case_options in
        //    crates/reify-expr/src/lib.rs) ─────────────────────────────────────
        // Option(Some(x)) → per-case override; anything else → shared default.
        let effective_options = match data.fields.get(&"options".to_string()) {
            Some(Value::Option(Some(per_case_opts))) => (**per_case_opts).clone(),
            _ => shared_options.clone(),
        };

        // ── Dispatch elastic_static for this case ─────────────────────────────
        // Each case is cold (prior_warm_state = None).  Cross-case warm-state
        // and realization-cache reuse are re-homed to task 4152.
        let per_case_inputs: Vec<Value> = vec![
            material.clone(),
            length.clone(),
            width.clone(),
            height.clone(),
            loads,
            supports,
            effective_options,
        ];

        let outcome = super::elastic_static::solve_elastic_static_trampoline(
            &per_case_inputs,
            realization_inputs,
            &Value::Undef,
            None, // cold — cross-case warm-state reuse re-homed to task 4152
            cancellation,
        );

        match outcome {
            ComputeOutcome::Completed { result, diagnostics: case_diags, .. } => {
                accumulated_diagnostics.extend(case_diags);
                inner.insert(Value::String(name), result);
            }
            ComputeOutcome::Cancelled => return ComputeOutcome::Cancelled,
            ComputeOutcome::Failed { diagnostics: fail_diags } => {
                return ComputeOutcome::Failed { diagnostics: fail_diags };
            }
        }
    }

    // ── (4) Wrap as MultiCaseResult shape ─────────────────────────────────────
    //
    // Emits Value::Map{"cases" -> Map<String, ElasticResult>} — the de-facto
    // runtime MultiCaseResult contract consumed by detect_multi_case_result,
    // extract_cases_map, multi_case_result_value, and all existing consumers.
    let mut outer: BTreeMap<Value, Value> = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(inner));

    ComputeOutcome::Completed {
        result: Value::Map(outer),
        new_warm_state: None, // cross-case warm-state donation re-homed to task 4152
        cost_per_byte: None,
        diagnostics: accumulated_diagnostics,
    }
}

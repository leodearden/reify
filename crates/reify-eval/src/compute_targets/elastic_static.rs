//! Trampoline for `solver::elastic_static` — the `fn solve_elastic_static`
//! @optimized target (PRD §8 task η, docs/prds/v0_3/compute-node-contract.md).
//!
//! This file is a SKELETON that returns `Value::Undef`. The real FEA pipeline
//! (mesh + assemble + BC + CG solve + stress recovery + von Mises) lands in
//! step-8 of task 3426.
//!
//! See `compute_targets/mod.rs` for the placement-vs-PRD deviation rationale.

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Skeleton trampoline for `solver::elastic_static`.
///
/// Accepts the seven `value_inputs` corresponding to:
///   [0] material  : ElasticMaterial (Value::StructureInstance)
///   [1] length    : Length          (Value::Scalar { dimension: LENGTH })
///   [2] width     : Length          (Value::Scalar { dimension: LENGTH })
///   [3] height    : Length          (Value::Scalar { dimension: LENGTH })
///   [4] loads     : List<Real>      (Value::List of StructureInstances at runtime)
///   [5] supports  : List<Real>      (Value::List of StructureInstances at runtime)
///   [6] options   : ElasticOptions  (Value::StructureInstance)
///
/// Returns `ComputeOutcome::Completed { result: Value::Undef, ... }` in this
/// skeleton. Step-8 replaces this with a real P1-tet FEA solve.
pub fn solve_elastic_static_trampoline(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // SKELETON — returns Undef until step-8 wires the real FEA pipeline.
    // Step-5 test assertions allow Value::Undef here; step-7 will turn RED
    // until step-8 replaces this with the real solve.
    ComputeOutcome::Completed {
        result: Value::Undef,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

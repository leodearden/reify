//! Compute-node dispatch contract types: `ComputeFn`, `ComputeOutcome`,
//! `StructuredComputeDetail`, `DispatchError`.
//!
//! Moved from `reify-eval`'s `engine_compute.rs` (task A / #4934); the
//! `ComputeDispatchRegistry` consumer and its `Engine::run_compute_dispatch`
//! caller stay behind in `reify-eval` (Engine-bound, not part of this
//! OCCT-free contract crate).
//!
//! See `docs/prds/v0_3/compute-node-contract.md` §4 and §8-γ for the full spec.

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::CancellationHandle;
use crate::realization::RealizationReadHandle;

/// Function-pointer type for a synchronous compute trampoline.
///
/// Signature (PRD §4):
/// - `value_inputs`: resolved scalar/tensor inputs for this invocation
/// - `realization_inputs`: resolved geometry inputs (read-only handles)
/// - `options`: per-invocation option map (`Value::Map` or `Value::Undef`)
/// - `prior_warm_state`: warm-start state from the previous invocation, if any
/// - `cancellation`: cooperative-cancellation handle; implementations should
///   poll `is_cancelled()` at coarse-grained intervals
///
/// Returns a [`ComputeOutcome`] describing the result, any new warm state,
/// cost metadata, and diagnostics.
///
/// This is a plain function-pointer (`fn`) type, not a boxed trait object,
/// to keep dispatch registration zero-allocation and enable `Copy` semantics
/// (a registry lookup returns `Option<ComputeFn>` directly without a heap read).
pub type ComputeFn = fn(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome;

/// Typed structured-detail overlay for a single compute outcome.
///
/// Carries solver-specific diagnostic detail that cannot be expressed by the
/// flat `Vec<Diagnostic>` channel — e.g. exact rigid-body-mode axes or the
/// element IDs of a degenerate element.  Wrapped in a crate-level enum so
/// `reify-eval` remains the boundary owner and future non-FEA structured
/// details can be added without touching `reify-solver-elastic`.
///
/// `NO serde` — IPC serialisation is the responsibility of the R3b-2 consumer
/// (`gui/src-tauri` via task #4818); keeping this type serde-free keeps
/// `reify-eval` free of a `serde` feature flag.
///
/// See `docs/prds/v0_4/fea-result-model.md` §4.6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredComputeDetail {
    /// An FEA-specific typed overlay (rigid-body modes, problem elements, etc.).
    Fea(reify_solver_elastic::FeaDiagnosticDetail),
}

/// Outcome of a synchronous [`ComputeFn`] invocation.
///
/// See `docs/prds/v0_3/compute-node-contract.md` §4 and §5.
#[derive(Debug)]
pub enum ComputeOutcome {
    /// The computation completed successfully.
    Completed {
        /// The primary result value written to the output value cell.
        result: Value,
        /// Optional warm-start state to donate for the next invocation.
        /// `None` in γ (warm-state lifecycle is deferred to slice ζ/3425).
        new_warm_state: Option<OpaqueState>,
        /// Optional cost estimate in abstract units per byte of output.
        /// Intended for cache-eviction heuristics; `None` means "unknown".
        cost_per_byte: Option<f64>,
        /// Non-fatal diagnostics generated during computation.
        diagnostics: Vec<Diagnostic>,
        /// Typed structured-detail overlays (R3b-1/#4802).  Empty on all
        /// targets except FEA-elastic-static (where Unconstrained carries the
        /// 6-mode payload on the warned-but-Completed outcome).
        structured_detail: Vec<StructuredComputeDetail>,
    },
    /// The computation was cancelled via the [`CancellationHandle`].
    /// Cancellation lifecycle (`running` field management) is deferred to
    /// slice ε (3424); for γ the cancellation handle is created fresh and
    /// never polled externally.
    Cancelled,
    /// The computation failed; no result value is available.
    Failed {
        /// Diagnostics describing the failure. Should include at least one
        /// `Severity::Error` diagnostic.
        diagnostics: Vec<Diagnostic>,
        /// Typed structured-detail overlays (R3b-1/#4802).  Empty on all
        /// targets except FEA-elastic-static (where SingularStiffness carries
        /// ProblemElements on the hard-Failed outcome).
        structured_detail: Vec<StructuredComputeDetail>,
    },
}

/// Error returned by `Engine::run_compute_dispatch` when a dispatch does not
/// complete successfully.
///
/// Distinguishes the two terminal non-success outcomes so the lowering site
/// (and tests) can apply the correct cache transition:
///
/// - [`DispatchError::Cancelled`] — the trampoline observed cancellation via
///   its [`CancellationHandle`] and returned [`ComputeOutcome::Cancelled`].
///   The output VCs are **left `Freshness::Pending`** (prior best on display,
///   cache untouched) per PRD §2 / §7.1.  Callers must NOT call `mark_failed`
///   on this path.
///
/// - [`DispatchError::Failed`] — the trampoline returned
///   [`ComputeOutcome::Failed`], or the target string had no registered
///   trampoline.  The output VCs are also left `Pending` (from
///   `begin_compute_dispatch`); the caller owns the `mark_failed` transition.
///
/// See `docs/prds/v0_3/compute-node-contract.md` §2 / §7.1 / §8-ε.
#[derive(Debug)]
pub enum DispatchError {
    /// The trampoline observed `CancellationHandle::is_cancelled` and
    /// returned [`ComputeOutcome::Cancelled`].  Output VCs stay Pending; prior
    /// cache and warm-state are untouched.  The lowering site must NOT call
    /// `mark_failed`; it should journal a non-Changed event and `continue`.
    Cancelled,
    /// The trampoline returned [`ComputeOutcome::Failed`] or the target string
    /// had no registered trampoline.  The first `Vec<Diagnostic>` carries the
    /// trampoline's error diagnostics (or the "no registered trampoline"
    /// synthesised diagnostic); the second carries any typed structured-detail
    /// overlay (R3b-1/#4802).  The lowering site owns `mark_failed`.
    Failed(Vec<Diagnostic>, Vec<StructuredComputeDetail>),
}

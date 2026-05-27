//! ComputeNode trampoline and registration for the `"shell-extract::extract"`
//! target (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.
//!
//! # γ-only SampledField seam
//!
//! PRD §5 contract: `value_inputs=[options: ElasticOptions]`,
//! `realization_inputs=[body_geom: BRep or Mesh]`. However,
//! `RealizationReadHandle` content accessors are deferred to δ/ε/ζ per
//! `engine_compute.rs:104-110`. For γ the trampoline reads the geometry SDF
//! from `value_inputs[1]` (a `Value::SampledField`) with an inline
//! `// γ-only seam` comment. Tasks δ/ε will migrate it to
//! `realization_inputs[0]` once the realization-read API lands.
//!
//! # Cancellation granularity
//!
//! Per PRD §11 OQ-5 (decided during γ): cancellation is polled at each of
//! the five phase boundaries (medial-mask, mid-surface, prune, mesh, segment)
//! rather than per-voxel. Per-phase polling is sufficient for sub-100ms
//! synthetic-slab runs; tighter inner-loop granularity can land in ε or a
//! follow-up without interface breakage.

use reify_core::Diagnostic;
use reify_ir::{OpaqueState, Value};

use crate::engine_compute::{ComputeFn, ComputeOutcome, RealizationReadHandle};
use crate::graph::CancellationHandle;
use crate::Engine;

/// Synchronous compute trampoline for `"shell-extract::extract"`.
///
/// Skeleton implementation — returns `Failed` unconditionally. Full pipeline
/// wired in step-4 (task γ, #3834).
///
/// # Inputs (γ-only shape)
///
/// - `value_inputs[0]`: `Value::StructureInstance("ElasticOptions")` or
///   `Value::Undef` (use producer defaults)
/// - `value_inputs[1]`: `Value::SampledField` carrying the SDF of the body
///   geometry — **γ-only seam**; tasks δ/ε will migrate this to
///   `realization_inputs[0]` once `RealizationReadHandle` content accessors land.
///
/// # Cancellation
///
/// Polled at each of the five phase boundaries (medial-mask, mid-surface,
/// prune, mesh, segment). Per PRD §11 OQ-5: per-phase polling is sufficient
/// for synthetic-slab runtimes; tighter inner-loop granularity deferred to ε.
pub fn shell_extract_compute_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // Skeleton: step-4 wires the full pipeline.
    ComputeOutcome::Failed {
        diagnostics: vec![Diagnostic::error(
            "shell_extract_compute_fn: not yet implemented",
        )],
    }
}

/// Register the `"shell-extract::extract"` trampoline with `engine`.
///
/// Called by binary entry points (CLI, GUI, test harnesses) that wish to
/// enable the shell-extract pipeline. Panics if `"shell-extract::extract"` is
/// already registered (PRD §4 hard-error contract, propagated from
/// `Engine::register_compute_fn`).
///
/// # Design note
///
/// This is a stand-alone `pub fn` rather than a generic aggregator because no
/// workspace-wide `register_compute_fns` function exists today (PRD §4 and
/// γ design decision). Future task ι (end-to-end smoke binary) is the natural
/// point to introduce an aggregator if needed.
pub fn register_shell_extract_compute_fns(engine: &mut Engine) {
    engine.register_compute_fn("shell-extract::extract", shell_extract_compute_fn as ComputeFn);
}

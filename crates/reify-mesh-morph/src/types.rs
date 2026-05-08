//! Internal payload types for [`crate::MorphFailure`] and the [`BRep`] alias.
//!
//! Implements task #4 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): the consumer-neutral public API
//! surface for the morph engine.

// ── BRep alias ────────────────────────────────────────────────────────────────

/// Consumer-neutral name for the morph-engine's input snapshot.
///
/// Defined as a type alias over [`crate::eligibility::MorphSnapshot`] so that:
/// - morph-engine callers (`morph()`, `eligible()`) use the PRD name `BRep`
///   (per PRD §"Generic crate scope").
/// - eligibility-only callers (failure-mode visibility counters, GUI badge —
///   PRD tasks #11/#12) continue to use the more descriptive `MorphSnapshot`
///   name, which communicates the borrow-bundle semantics (graph + values +
///   topology_attributes + handle slices).
///
/// Both names are exported from `lib.rs`; both point at the same `Copy`
/// borrow-bundle. No translation between them is ever needed.
pub type BRep<'a> = crate::eligibility::MorphSnapshot<'a>;

// ── MorphFailure payload structs ──────────────────────────────────────────────

/// Payload for [`crate::MorphFailure::QualityHardFail`].
///
/// Carries the identity of the element that caused a hard inversion (negative
/// Jacobian) and the Jacobian value itself. Both fields are populated by the
/// quality-check pass in PRD task #9.
#[derive(Debug, Clone, PartialEq)]
pub struct InversionDetails {
    /// Zero-based index of the inverted element in the output `VolumeMesh`.
    pub element_index: usize,
    /// Scaled Jacobian of the inverted element (negative means inversion).
    pub jacobian: f64,
}

/// Payload for [`crate::MorphFailure::QualitySoftFail`].
///
/// Carries which quality thresholds from [`crate::MorphOptions`] were breached.
/// Fields are `None` when the corresponding check passed. Populated by the
/// quality-check pass in PRD task #9.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsBreached {
    /// Observed minimum scaled Jacobian if it fell below
    /// [`crate::MorphOptions::quality_floor_min_scaled_jacobian`].
    pub min_scaled_jacobian: Option<f64>,
    /// Observed fraction of elements below 0.25 if it exceeded
    /// [`crate::MorphOptions::quality_floor_pct_below_025`].
    pub pct_below_025: Option<f64>,
    /// Observed maximum aspect-ratio increase if it exceeded
    /// [`crate::MorphOptions::quality_aspect_ratio_increase_max`].
    pub max_aspect_ratio_increase: Option<f64>,
}

/// Opaque payload for [`crate::MorphFailure::SolverError`].
///
/// Wraps the error message in a named struct so future tasks can add fields
/// (e.g. a structured kernel-error code from `reify-solver-elastic`) without
/// a breaking API change. Use [`SolverErrorPayload::new`] to construct and
/// [`SolverErrorPayload::message`] to read the message text.
///
/// The `message` field is private; callers can read via `message()` and
/// construct via `new(...)`. When PRD task #7 lands a structured kernel-error
/// type, additional fields (e.g. `source: reify_solver_elastic::SolverError`)
/// can be added without breaking existing `SolverError(payload)` match arms.
#[derive(Debug, Clone)]
pub struct SolverErrorPayload {
    message: String,
}

impl SolverErrorPayload {
    /// Create a new payload from an error message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The error message text.
    pub fn message(&self) -> &str {
        &self.message
    }
}
